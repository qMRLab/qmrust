//! VFA adapter onto the core `Model` trait.

use crate::core::model::{
    Aux, BidsMap, BidsSpec, BidsVolume, EntityRole, FitStrategy, InputSpec, Measurement,
    MeasurementKind, Meta, Model, ProtoParam, Protocol, Scope, SeriesAxis, Source,
};
use crate::models::vfa_t1::config::VfaT1Config;
use crate::models::vfa_t1::fit::{VfaT1Fitter, M0_BOUNDS, T1_BOUNDS};
use anyhow::{anyhow, Result};
use serde_json::json;
use std::collections::BTreeMap;

pub struct VfaT1Model {
    fitter: VfaT1Fitter,
}

/// The acquisition axis: one volume per FlipAngle.
const AXIS: SeriesAxis = SeriesAxis::new("FlipAngle");

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
            rows: AXIS.rows(self.fitter.flip_angles()),
        }
    }
    fn strategy(&self) -> FitStrategy {
        FitStrategy::Voxelwise
    }
    fn forward(&self, params: &[f64], aux: &Aux) -> Measurement {
        let values = self
            .fitter
            .forward(params[0], params[1], aux.get("B1map").unwrap_or(1.0));
        AXIS.samples(self.fitter.flip_angles(), values)
    }
    fn fit(&self, m: &Measurement, aux: &Aux) -> Vec<f64> {
        let signal = AXIS.assemble(m, self.fitter.flip_angles());
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
        let angles = AXIS.ingest(proto);
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

crate::model_entry_points!(VfaT1Config);

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
}
