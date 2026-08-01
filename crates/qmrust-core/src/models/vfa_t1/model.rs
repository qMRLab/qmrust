//! VFA adapter onto the core `Model` trait.

use crate::core::model::{
    Aux, BidsMap, BidsSpec, BidsVolume, EntityRole, FitStrategy, InputSpec, Measurement,
    MeasurementKind, Meta, Model, ProtoParam, Protocol, Sample, Scope, Source,
};
use crate::models::vfa_t1::config::VfaT1Config;
use crate::models::vfa_t1::fit::{VfaT1Fitter, M0_BOUNDS, T1_BOUNDS};
use anyhow::{anyhow, Result};
use serde_json::json;
use std::collections::BTreeMap;

pub struct VfaT1Model {
    fitter: VfaT1Fitter,
}

/// One `{"FlipAngle": deg}` identity row per fitter flip angle, in canonical
/// order.
fn vfa_t1_rows(fitter: &VfaT1Fitter) -> Vec<BTreeMap<String, f64>> {
    fitter
        .flip_angles()
        .iter()
        .map(|&fa| BTreeMap::from([("FlipAngle".to_string(), fa)]))
        .collect()
}

impl VfaT1Model {
    pub fn new(cfg: VfaT1Config) -> Self {
        Self {
            fitter: VfaT1Fitter::new(&cfg),
        }
    }
}

const VFA_ENTITIES: &[EntityRole] = &[EntityRole::Flip];

/// Excitation repetition time in seconds. BIDS names it
/// `RepetitionTimeExcitation` for a VFA series, but a dataset converted from a
/// plain gradient-echo acquisition often carries only `RepetitionTime`; both
/// denote the same interval here, so either resolves the protocol.
fn repetition_time(meta: &dyn Meta) -> Result<f64> {
    meta.f64("RepetitionTimeExcitation")
        .or_else(|| meta.f64("RepetitionTime"))
        .ok_or_else(|| anyhow!("sidecar has neither RepetitionTimeExcitation nor RepetitionTime"))
}

impl Model for VfaT1Model {
    fn param_names(&self) -> Vec<&'static str> {
        VfaT1Fitter::param_names().to_vec()
    }
    fn output_names(&self) -> Vec<String> {
        VfaT1Fitter::output_names()
            .iter()
            .map(|s| s.to_string())
            .collect()
    }
    fn param_bounds(&self) -> Vec<(f64, f64)> {
        // qMRLab's lb/ub in BIDS-native units, in param_names order [T1, M0].
        // The linearized solve is closed form and does not enforce them.
        vec![T1_BOUNDS, M0_BOUNDS]
    }
    fn fixed_mask(&self) -> Vec<bool> {
        vec![false; 2]
    }
    fn required_inputs(&self) -> Vec<InputSpec> {
        // Normalized transmit field, used-if-present: it scales the nominal
        // flip angle (α_actual = B1 · α_nominal). Absent, the nominal angles
        // are taken at face value.
        vec![InputSpec {
            name: "B1map",
            required: false,
            bids: Some(BidsMap {
                suffix: "TB1map",
                entity: None,
            }),
        }]
    }
    fn measurement(&self) -> MeasurementKind {
        MeasurementKind::Series {
            rows: vfa_t1_rows(&self.fitter),
        }
    }
    fn strategy(&self) -> FitStrategy {
        FitStrategy::Voxelwise
    }
    fn forward(&self, params: &[f64], aux: &Aux) -> Measurement {
        let values = self
            .fitter
            .forward(params[0], params[1], aux.get("B1map").unwrap_or(1.0));
        let samples = self
            .fitter
            .flip_angles()
            .iter()
            .zip(values)
            .map(|(&fa, value)| Sample {
                params: BTreeMap::from([("FlipAngle".to_string(), fa)]),
                value,
            })
            .collect();
        Measurement::Series(samples)
    }
    fn fit(&self, m: &Measurement, aux: &Aux) -> Vec<f64> {
        // Assemble the signal in the fitter's own flip-angle order by matching
        // each expected angle to its sample by identity — never positionally. An
        // angle with no matching sample is a mislabeled measurement → panic (the
        // engine records the voxel as a failed fit). Angles are assumed unique;
        // first match wins.
        let samples = m.series();
        let signal: Vec<f64> = self
            .fitter
            .flip_angles()
            .iter()
            .map(|&fa| {
                samples
                    .iter()
                    .find(|s| s.params.get("FlipAngle") == Some(&fa))
                    .map(|s| s.value)
                    .unwrap_or_else(|| panic!("measurement has no sample with FlipAngle={fa}"))
            })
            .collect();
        self.fitter
            .fit_voxel(&signal, aux.get("B1map").unwrap_or(1.0))
    }
    fn n_volumes(&self) -> usize {
        self.fitter.flip_angles().len()
    }
    fn bids_volume(&self, index: usize) -> BidsVolume {
        BidsVolume {
            entities: vec![("flip", (index + 1).to_string())],
            sidecar: BTreeMap::from([
                (
                    "FlipAngle".to_string(),
                    json!(self.fitter.flip_angles()[index]),
                ),
                (
                    "RepetitionTimeExcitation".to_string(),
                    json!(self.fitter.repetition_time()),
                ),
            ]),
        }
    }
    fn bids(&self) -> Option<BidsSpec> {
        Some(BidsSpec {
            suffix: "VFA",
            entities: VFA_ENTITIES,
        })
    }
    fn protocol_schema(&self) -> Vec<ProtoParam> {
        // Flip angle is the acquisition axis, so it alone identifies a volume:
        // a `Series`' per-volume protocol rows *are* its volume identities
        // (`engine::build_volume_ids`), and `forward` tags its samples the same
        // way. TR is a constant shared by the series, so it is `Global` — as a
        // per-volume param it would join the identity and no forward sample
        // would match its volume.
        vec![
            ProtoParam {
                name: "FlipAngle",
                source: Source::Field("FlipAngle"),
                scope: Scope::PerVolume,
                required: true,
            },
            ProtoParam {
                name: "RepetitionTimeExcitation",
                source: Source::Derived(repetition_time),
                scope: Scope::Global,
                required: true,
            },
        ]
    }
    fn bids_outputs(&self) -> Vec<(&'static str, &'static str, &'static str)> {
        // T1map carries the quantitative time constant (seconds). M0map is the
        // fitted equilibrium signal amplitude — receive-coil dependent, not a
        // calibrated quantity — so its unit is left blank (arbitrary).
        vec![("T1", "T1map", "s"), ("M0", "M0map", "")]
    }
}

impl crate::core::model::ModelConfig for VfaT1Config {
    const NAME: &'static str = "vfa_t1";
    const SUBKEY: Option<&'static str> = None;
    const PROTOCOL_KEYS: &'static [&'static str] = &["flip_angles", "repetition_time"];

    fn validate_options(&mut self) -> Result<()> {
        VfaT1Config::validate_options(self)
    }

    fn ingest_protocol(&mut self, proto: &Protocol) -> Result<()> {
        let angles: Vec<f64> = proto
            .volumes
            .iter()
            .filter_map(|m| m.get("FlipAngle").copied())
            .collect();
        if !angles.is_empty() {
            self.flip_angles = angles;
        }
        // Global scope, so this is resolved once for the collection rather than
        // per volume — read it independently of the per-volume rows.
        if let Some(&tr) = proto.global.get("RepetitionTimeExcitation") {
            self.repetition_time = Some(tr);
        }
        Ok(())
    }

    fn validate_protocol(&mut self) -> Result<()> {
        VfaT1Config::validate_protocol(self)
    }

    fn into_model(self) -> Box<dyn Model> {
        Box::new(VfaT1Model::new(self))
    }
}

/// Structural interrogation entry point (see [`describe_model`](crate::core::model::describe_model)).
pub fn describe(v: &serde_yaml::Value) -> Result<Box<dyn Model>> {
    crate::core::model::describe_model::<VfaT1Config>(v)
}

/// Registry builder (see [`build_model`](crate::core::model::build_model)): the
/// shared parse → ingest protocol → validate → construct pipeline.
pub fn build(v: &serde_yaml::Value, proto: &Protocol) -> Result<Box<dyn Model>> {
    crate::core::model::build_model::<VfaT1Config>(v, proto)
}

/// Registry dumper (see [`dump_model`](crate::core::model::dump_model)): prints
/// the fully-resolved effective config as YAML.
pub fn dump(v: &serde_yaml::Value) -> Result<String> {
    crate::core::model::dump_model::<VfaT1Config>(v)
}

/// Registry option-surface entry point (see
/// [`effective_model`](crate::core::model::effective_model)): every option this
/// model accepts, at its effective value, plus any validation complaint.
pub fn effective(
    v: &serde_yaml::Value,
    proto: &Protocol,
) -> Result<crate::core::model::EffectiveConfig> {
    crate::core::model::effective_model::<VfaT1Config>(v, proto)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vfa_value() -> serde_yaml::Value {
        serde_yaml::from_str("model: vfa_t1\nflip_angles: [3, 20]\nrepetition_time: 0.015\n")
            .unwrap()
    }

    /// A resolved protocol as the shell hands it over: one row per volume
    /// carrying only the identifying axis, plus the collection-wide TR.
    fn resolved_proto(angles: &[f64], tr: f64) -> Protocol {
        Protocol {
            volumes: angles
                .iter()
                .map(|&fa| BTreeMap::from([("FlipAngle".to_string(), fa)]))
                .collect(),
            global: BTreeMap::from([("RepetitionTimeExcitation".to_string(), tr)]),
        }
    }

    #[test]
    fn build_and_roundtrip_via_trait() {
        let m = build(&vfa_value(), &Protocol::default()).unwrap();
        // The quantitative map leads, as in every relaxometry model here, and
        // `forward`/`fit` speak that same order.
        assert_eq!(m.param_names(), vec!["T1", "M0"]);
        assert_eq!(m.output_names(), vec!["T1", "M0"]);
        let sig = m.forward(&[0.9, 1000.0], &Aux::new());
        assert_eq!(sig.series().len(), 2);
        let fitted = m.fit(&sig, &Aux::new());
        assert!((fitted[0] - 0.9).abs() < 1e-9, "T1: {}", fitted[0]);
        assert!((fitted[1] - 1000.0).abs() < 1e-6, "M0: {}", fitted[1]);
    }

    #[test]
    fn fit_assembles_by_identity_not_position() {
        // Samples supplied in reversed order must give an identical fit; a
        // positional assembly would pair each signal with the wrong angle.
        let m = build(&vfa_value(), &Protocol::default()).unwrap();
        let sig = m.forward(&[0.9, 1000.0], &Aux::new());
        let mut reversed: Vec<Sample> = match &sig {
            Measurement::Series(s) => s
                .iter()
                .map(|s| Sample {
                    params: s.params.clone(),
                    value: s.value,
                })
                .collect(),
            _ => unreachable!(),
        };
        reversed.reverse();
        let a = m.fit(&sig, &Aux::new());
        let b = m.fit(&Measurement::Series(reversed), &Aux::new());
        assert_eq!(a, b, "fit must be invariant to sample order");
    }

    #[test]
    #[should_panic(expected = "no sample with FlipAngle")]
    fn fit_panics_on_unmatched_identity() {
        let m = build(&vfa_value(), &Protocol::default()).unwrap();
        let bogus = Measurement::Series(vec![Sample {
            params: BTreeMap::from([("FlipAngle".to_string(), 77.0)]),
            value: 1.0,
        }]);
        let _ = m.fit(&bogus, &Aux::new());
    }

    #[test]
    fn bids_folds_flip_angles_and_tr_from_protocol() {
        let proto = resolved_proto(&[4.0, 25.0], 0.018);
        // Config carries no acquisition; the sidecars supply it.
        let v: serde_yaml::Value = serde_yaml::from_str("model: vfa_t1\n").unwrap();
        let m = build(&v, &proto).unwrap();
        assert_eq!(m.n_volumes(), 2);
        let second = m.bids_volume(1);
        assert_eq!(second.entities, vec![("flip", "2".to_string())]);
        assert_eq!(second.sidecar["FlipAngle"], json!(25.0));
        assert_eq!(second.sidecar["RepetitionTimeExcitation"], json!(0.018));
    }

    #[test]
    fn forward_samples_carry_the_volume_identities_the_bids_path_builds() {
        // A `Series` model's volume identities come from the resolved
        // per-volume protocol (`engine::build_volume_ids`), while `forward`
        // tags its samples from the model's own rows. Any protocol param the
        // model does not also emit joins the identity on one side only, and
        // every predicted sample then fails to match its volume — the fit still
        // works (it queries one key), so the only symptom is a silently missing
        // forward curve. Both sides must agree exactly.
        let proto = resolved_proto(&[3.0, 20.0], 0.015);
        let v: serde_yaml::Value = serde_yaml::from_str("model: vfa_t1\n").unwrap();
        let m = build(&v, &proto).unwrap();

        let ids = crate::engine::build_volume_ids(m.measurement(), &proto, m.n_volumes()).unwrap();
        let sig = m.forward(&[0.9, 1000.0], &Aux::new());
        let samples = sig.series();
        assert_eq!(samples.len(), ids.len());
        for (id, sample) in ids.iter().zip(samples) {
            let crate::core::model::VolumeId::Params(row) = id else {
                panic!("vfa_t1 is a Series model; expected param-row identities")
            };
            assert_eq!(
                *row, sample.params,
                "volume identity {row:?} has no matching forward sample identity {:?}",
                sample.params
            );
        }
    }

    #[test]
    fn b1_map_from_aux_scales_the_flip_angles() {
        let m = build(&vfa_value(), &Protocol::default()).unwrap();
        let mut aux = Aux::new();
        aux.set("B1map", 1.2);
        // Signal generated at B1=1.2 and fitted with the same B1 recovers truth;
        // fitting it as if B1 were nominal does not.
        let sig = m.forward(&[0.9, 1000.0], &aux);
        assert!((m.fit(&sig, &aux)[0] - 0.9).abs() < 1e-9);
        assert!((m.fit(&sig, &Aux::new())[0] - 0.9).abs() > 1e-3);
    }

    #[test]
    fn declares_bids_vfa_and_b1_aux() {
        let m = build(&vfa_value(), &Protocol::default()).unwrap();
        assert_eq!(m.bids().unwrap().suffix, "VFA");
        let inputs = m.required_inputs();
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].name, "B1map");
        assert!(!inputs[0].required);
    }

    #[test]
    fn bids_outputs_reference_real_output_names() {
        let m = build(&vfa_value(), &Protocol::default()).unwrap();
        let names = m.output_names();
        for (out, _suffix, _unit) in m.bids_outputs() {
            assert!(names.iter().any(|n| n == out), "{out} not in {names:?}");
        }
    }

    #[test]
    fn describe_succeeds_without_an_acquisition_and_exposes_schema() {
        let v: serde_yaml::Value = serde_yaml::from_str("model: vfa_t1\n").unwrap();
        let m = describe(&v).unwrap();
        let schema = m.protocol_schema();
        assert_eq!(schema[0].name, "FlipAngle");
        assert_eq!(schema[1].name, "RepetitionTimeExcitation");
        // FlipAngle identifies a volume; TR is one value for the collection.
        assert!(matches!(schema[0].scope, Scope::PerVolume));
        assert!(matches!(schema[1].scope, Scope::Global));
    }

    #[test]
    fn build_still_requires_an_acquisition_when_protocol_empty() {
        let v: serde_yaml::Value =
            serde_yaml::from_str("model: vfa_t1\nflip_angles: [3]\n").unwrap();
        assert!(build(&v, &Protocol::default()).is_err());
    }
}
