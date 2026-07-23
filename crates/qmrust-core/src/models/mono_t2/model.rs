//! mono_t2 adapter onto the core `Model` trait.

use crate::core::model::{
    Aux, BidsSpec, BidsVolume, EntityRole, FitStrategy, InputSpec, Measurement, MeasurementKind,
    Model, ProtoParam, Protocol, Sample, Scope, Source,
};
use crate::models::mono_t2::config::MonoT2Config;
use crate::models::mono_t2::fit::{MonoT2Fitter, M0_BOUNDS, T2_BOUNDS};
use anyhow::Result;
use serde_json::json;
use std::collections::BTreeMap;

pub struct MonoT2Model {
    fitter: MonoT2Fitter,
    output_names: Vec<String>,
}

/// One `{"EchoTime": te}` identity row per fitter echo, in canonical order.
fn mono_t2_rows(fitter: &MonoT2Fitter) -> Vec<BTreeMap<String, f64>> {
    fitter
        .te()
        .iter()
        .map(|&te| BTreeMap::from([("EchoTime".to_string(), te)]))
        .collect()
}

impl MonoT2Model {
    pub fn new(cfg: MonoT2Config) -> Self {
        let fitter = MonoT2Fitter::new(&cfg);
        let output_names = fitter
            .output_names()
            .iter()
            .map(|s| s.to_string())
            .collect();
        Self {
            fitter,
            output_names,
        }
    }
}

const MONO_T2_ENTITIES: &[EntityRole] = &[EntityRole::Echo];

impl Model for MonoT2Model {
    fn param_names(&self) -> Vec<&'static str> {
        MonoT2Fitter::param_names().to_vec()
    }
    fn output_names(&self) -> Vec<String> {
        self.output_names.clone()
    }
    fn param_bounds(&self) -> Vec<(f64, f64)> {
        // qMRLab's lb/ub (BIDS-native units), in output_names order [T2, M0].
        // The exponential path enforces them; the linear path is unconstrained
        // beyond clamping non-physical T2 to zero.
        vec![T2_BOUNDS, M0_BOUNDS]
    }
    fn fixed_mask(&self) -> Vec<bool> {
        vec![false; 2]
    }
    fn required_inputs(&self) -> Vec<InputSpec> {
        vec![]
    }
    fn measurement(&self) -> MeasurementKind {
        MeasurementKind::Series {
            rows: mono_t2_rows(&self.fitter),
        }
    }
    fn strategy(&self) -> FitStrategy {
        FitStrategy::Voxelwise
    }
    fn forward(&self, params: &[f64], _aux: &Aux) -> Measurement {
        let values = self.fitter.forward(params[0], params[1]);
        let samples = self
            .fitter
            .te()
            .iter()
            .zip(values)
            .map(|(&te, value)| Sample {
                params: BTreeMap::from([("EchoTime".to_string(), te)]),
                value,
            })
            .collect();
        Measurement::Series(samples)
    }
    fn fit(&self, m: &Measurement, _aux: &Aux) -> Vec<f64> {
        // Assemble the signal in the fitter's own echo order by matching each
        // expected TE to its sample by identity — never positionally. A TE with
        // no matching sample is a mislabeled measurement → panic (the engine
        // records the voxel as a failed fit). TEs are assumed unique; first
        // match wins.
        let samples = m.series();
        let signal: Vec<f64> = self
            .fitter
            .te()
            .iter()
            .map(|&te| {
                samples
                    .iter()
                    .find(|s| s.params.get("EchoTime") == Some(&te))
                    .map(|s| s.value)
                    .unwrap_or_else(|| panic!("measurement has no sample with EchoTime={te}"))
            })
            .collect();
        self.fitter.fit_voxel(&signal)
    }
    fn n_volumes(&self) -> usize {
        self.fitter.te().len()
    }
    fn bids_volume(&self, index: usize) -> BidsVolume {
        let mut sidecar = BTreeMap::new();
        sidecar.insert("EchoTime".to_string(), json!(self.fitter.te()[index]));
        BidsVolume {
            entities: vec![("echo", (index + 1).to_string())],
            sidecar,
        }
    }
    fn bids(&self) -> Option<BidsSpec> {
        Some(BidsSpec {
            suffix: "MESE",
            entities: MONO_T2_ENTITIES,
        })
    }
    fn protocol_schema(&self) -> Vec<ProtoParam> {
        vec![ProtoParam {
            name: "EchoTime",
            source: Source::Field("EchoTime"),
            scope: Scope::PerVolume,
        }]
    }
    fn bids_outputs(&self) -> Vec<(&'static str, &'static str, &'static str)> {
        // T2map carries the quantitative time constant (seconds). M0map is the
        // fitted raw signal amplitude — device-dependent, not a calibrated
        // quantity — so its unit is left blank (arbitrary).
        vec![("T2", "T2map", "s"), ("M0", "M0map", "")]
    }
}

impl crate::core::model::ModelConfig for MonoT2Config {
    const NAME: &'static str = "mono_t2";
    const SUBKEY: Option<&'static str> = None;

    fn validate_options(&mut self) -> Result<()> {
        MonoT2Config::validate_options(self)
    }

    fn ingest_protocol(&mut self, proto: &Protocol) -> Result<()> {
        if !proto.volumes.is_empty() {
            let tes: Vec<f64> = proto
                .volumes
                .iter()
                .filter_map(|m| m.get("EchoTime").copied())
                .collect();
            if !tes.is_empty() {
                self.echo_times = tes;
            }
        }
        Ok(())
    }

    fn validate_protocol(&mut self) -> Result<()> {
        MonoT2Config::validate_protocol(self)
    }

    fn into_model(self) -> Box<dyn Model> {
        Box::new(MonoT2Model::new(self))
    }
}

/// Structural interrogation entry point (see [`describe_model`]).
pub fn describe(v: &serde_yaml::Value) -> Result<Box<dyn Model>> {
    crate::core::model::describe_model::<MonoT2Config>(v)
}

/// Registry builder (see [`build_model`]): the shared parse → ingest protocol →
/// validate → construct pipeline.
pub fn build(v: &serde_yaml::Value, proto: &Protocol) -> Result<Box<dyn Model>> {
    crate::core::model::build_model::<MonoT2Config>(v, proto)
}

/// Registry dumper (see [`dump_model`](crate::core::model::dump_model)): prints
/// the fully-resolved effective config as YAML.
pub fn dump(v: &serde_yaml::Value) -> Result<String> {
    crate::core::model::dump_model::<MonoT2Config>(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mono_t2_value() -> serde_yaml::Value {
        serde_yaml::from_str(
            "model: mono_t2\necho_times: [0.0128, 0.0256, 0.0384, 0.0512, 0.064, 0.0768, 0.0896, 0.1024]\n",
        )
        .unwrap()
    }

    #[test]
    fn build_and_roundtrip_via_trait() {
        let m = build(&mono_t2_value(), &Protocol::default()).unwrap();
        assert_eq!(m.param_names(), vec!["T2", "M0"]);
        let sig = m.forward(&[0.08, 1000.0], &Aux::new());
        assert_eq!(sig.series().len(), 8);
        let fitted = m.fit(&sig, &Aux::new());
        // output_names order [T2 (seconds), M0]; both recovered from raw signal.
        assert!((fitted[0] - 0.08).abs() < 1e-4, "T2: {}", fitted[0]);
        assert!((fitted[1] - 1000.0).abs() < 1.0, "M0: {}", fitted[1]);
    }

    #[test]
    fn fit_assembles_by_identity_not_position() {
        // Reversed samples with distinct TEs: positional assembly would feed a
        // mirrored signal and miss T2; only value-matching recovers 0.08.
        let m = build(&mono_t2_value(), &Protocol::default()).unwrap();
        let sig = m.forward(&[0.08, 1000.0], &Aux::new());
        let mut reversed: Vec<Sample> = match sig {
            Measurement::Series(ref s) => s
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
        assert_eq!(a[0], b[0], "T2 must be identical under reordering");
    }

    #[test]
    #[should_panic(expected = "no sample with EchoTime")]
    fn fit_panics_on_unmatched_identity() {
        let m = build(&mono_t2_value(), &Protocol::default()).unwrap();
        let bogus = Measurement::Series(vec![Sample {
            params: BTreeMap::from([("EchoTime".to_string(), 99999.0)]),
            value: 1.0,
        }]);
        let _ = m.fit(&bogus, &Aux::new());
    }

    #[test]
    fn mat_protocol_overrides_echo_times() {
        let mut proto = Protocol::default();
        for te in [0.0128, 0.0256, 0.0384] {
            proto
                .volumes
                .push(BTreeMap::from([("EchoTime".to_string(), te)]));
        }
        let m = build(&mono_t2_value(), &proto).unwrap();
        let sig = m.forward(&[0.08, 1000.0], &Aux::new());
        assert_eq!(sig.series().len(), 3);
    }

    #[test]
    fn declares_bids_mese() {
        let m = build(&mono_t2_value(), &Protocol::default()).unwrap();
        assert_eq!(m.bids().unwrap().suffix, "MESE");
    }

    #[test]
    fn bids_outputs_reference_real_output_names() {
        let m = build(&mono_t2_value(), &Protocol::default()).unwrap();
        let names = m.output_names();
        for (out, _suffix, _units) in m.bids_outputs() {
            assert!(
                names.iter().any(|n| n == out),
                "bids_outputs references '{out}', not in output_names {names:?}"
            );
        }
    }

    #[test]
    fn declares_echo_time_protocol_schema() {
        let m = build(&mono_t2_value(), &Protocol::default()).unwrap();
        let schema = m.protocol_schema();
        assert_eq!(schema.len(), 1);
        assert_eq!(schema[0].name, "EchoTime");
        assert!(matches!(schema[0].source, Source::Field("EchoTime")));
        assert!(matches!(schema[0].scope, Scope::PerVolume));
    }

    #[test]
    fn describe_succeeds_without_echo_times_and_exposes_schema() {
        let v: serde_yaml::Value = serde_yaml::from_str("model: mono_t2\n").unwrap();
        let m = super::describe(&v).unwrap(); // no echo_times → still OK
        assert_eq!(m.protocol_schema()[0].name, "EchoTime");
    }

    #[test]
    fn build_still_requires_two_echoes_when_protocol_empty() {
        let v: serde_yaml::Value =
            serde_yaml::from_str("model: mono_t2\necho_times: [0.0128]\n").unwrap();
        assert!(super::build(&v, &Protocol::default()).is_err()); // only 1 TE, no sidecars
    }
}
