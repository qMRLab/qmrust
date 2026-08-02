//! b1_dam adapter onto the core `Model` trait.
//!
//! A `Series` of exactly two spoiled gradient-echo volumes, indexed by flip
//! angle, combined by the closed-form double-angle ratio — there is no
//! acquisition parameter beyond the angles themselves and no iterative fit.

use crate::core::model::{
    Aux, BidsSpec, BidsVolume, EntityRole, FitStrategy, InputSpec, Measurement, MeasurementKind,
    Model, ProtoParam, Protocol, Sample, Scope, Source,
};
use crate::models::b1_dam::config::B1DamConfig;
use crate::models::b1_dam::fit::{B1DamFitter, B1_BOUNDS};
use anyhow::Result;
use serde_json::json;
use std::collections::BTreeMap;

pub struct B1DamModel {
    fitter: B1DamFitter,
}

const B1DAM_ENTITIES: &[EntityRole] = &[EntityRole::Flip];

/// One `{"FlipAngle": deg}` identity row per volume, in acquisition order.
fn b1_dam_rows(fitter: &B1DamFitter) -> Vec<BTreeMap<String, f64>> {
    fitter
        .flip_angles()
        .iter()
        .map(|&fa| BTreeMap::from([("FlipAngle".to_string(), fa)]))
        .collect()
}

impl B1DamModel {
    pub fn new(cfg: B1DamConfig) -> Self {
        Self {
            fitter: B1DamFitter::new(&cfg),
        }
    }
}

impl Model for B1DamModel {
    fn param_names(&self) -> Vec<&'static str> {
        B1DamFitter::param_names().to_vec()
    }
    fn output_names(&self) -> Vec<String> {
        B1DamFitter::output_names()
            .iter()
            .map(|s| s.to_string())
            .collect()
    }
    fn param_bounds(&self) -> Vec<(f64, f64)> {
        // The amplitude is an uncalibrated signal level, so it is unbounded.
        vec![B1_BOUNDS, (f64::NEG_INFINITY, f64::INFINITY)]
    }
    fn fixed_mask(&self) -> Vec<bool> {
        vec![false; 2]
    }
    fn required_inputs(&self) -> Vec<InputSpec> {
        // B1+ is what this model measures; it consumes no auxiliary map.
        vec![]
    }
    fn measurement(&self) -> MeasurementKind {
        MeasurementKind::Series {
            rows: b1_dam_rows(&self.fitter),
        }
    }
    fn strategy(&self) -> FitStrategy {
        FitStrategy::Voxelwise
    }
    fn forward(&self, params: &[f64], _aux: &Aux) -> Measurement {
        let values = self.fitter.forward(params[0], params[1]);
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
    fn fit(&self, m: &Measurement, _aux: &Aux) -> Vec<f64> {
        // Assemble in the fitter's own angle order by matching each expected
        // angle to its sample by identity — never positionally. Swapping the
        // two volumes would otherwise invert the ratio silently.
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
        self.fitter.fit_voxel(&signal)
    }
    fn n_volumes(&self) -> usize {
        self.fitter.flip_angles().len()
    }
    fn bids_volume(&self, index: usize) -> BidsVolume {
        BidsVolume {
            entities: vec![("flip", (index + 1).to_string())],
            sidecar: BTreeMap::from([(
                "FlipAngle".to_string(),
                json!(self.fitter.flip_angles()[index]),
            )]),
        }
    }
    fn bids(&self) -> Option<BidsSpec> {
        Some(BidsSpec {
            suffix: "TB1DAM",
            entities: B1DAM_ENTITIES,
        })
    }
    fn protocol_schema(&self) -> Vec<ProtoParam> {
        // Flip angle is the whole acquisition axis, so it alone identifies a
        // volume: a `Series`' per-volume protocol rows *are* its volume
        // identities (`engine::build_volume_ids`), and `forward` tags its
        // samples the same way.
        vec![ProtoParam {
            name: "FlipAngle",
            source: Source::Field("FlipAngle"),
            scope: Scope::PerVolume,
            required: true,
        }]
    }
    fn bids_outputs(&self) -> Vec<(&'static str, &'static str, &'static str)> {
        // Dimensionless: the achieved flip angle as a fraction of the nominal
        // one, which is the scaling every `B1map` aux consumer expects. `A` is
        // deliberately absent: it is a receive-weighted signal level with no
        // BIDS suffix of its own, so it stays a diagnostic output rather than
        // becoming a written map.
        vec![("B1", "TB1map", "")]
    }
}

impl crate::core::model::ModelConfig for B1DamConfig {
    const NAME: &'static str = "b1_dam";
    const SUBKEY: Option<&'static str> = None;
    const PROTOCOL_KEYS: &'static [&'static str] = &["flip_angles"];

    fn validate_options(&mut self) -> Result<()> {
        B1DamConfig::validate_options(self)
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
        Ok(())
    }

    fn validate_protocol(&mut self) -> Result<()> {
        B1DamConfig::validate_protocol(self)
    }

    fn into_model(self) -> Box<dyn Model> {
        Box::new(B1DamModel::new(self))
    }
}

/// Structural interrogation entry point (see [`describe_model`](crate::core::model::describe_model)).
pub fn describe(v: &serde_yaml::Value) -> Result<Box<dyn Model>> {
    crate::core::model::describe_model::<B1DamConfig>(v)
}

/// Registry builder (see [`build_model`](crate::core::model::build_model)).
pub fn build(v: &serde_yaml::Value, proto: &Protocol) -> Result<Box<dyn Model>> {
    crate::core::model::build_model::<B1DamConfig>(v, proto)
}

/// Registry dumper (see [`dump_model`](crate::core::model::dump_model)).
pub fn dump(v: &serde_yaml::Value) -> Result<String> {
    crate::core::model::dump_model::<B1DamConfig>(v)
}

/// Registry option-surface entry point (see
/// [`effective_model`](crate::core::model::effective_model)): every option this
/// model accepts, at its effective value, plus any validation complaint.
pub fn effective(
    v: &serde_yaml::Value,
    proto: &Protocol,
) -> Result<crate::core::model::EffectiveConfig> {
    crate::core::model::effective_model::<B1DamConfig>(v, proto)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b1_dam_value() -> serde_yaml::Value {
        serde_yaml::from_str("model: b1_dam\nflip_angles: [60, 120]\n").unwrap()
    }

    /// A resolved protocol as the shell hands it over: one row per volume
    /// carrying the identifying axis.
    fn resolved_proto(angles: &[f64]) -> Protocol {
        Protocol {
            volumes: angles
                .iter()
                .map(|&fa| BTreeMap::from([("FlipAngle".to_string(), fa)]))
                .collect(),
            global: BTreeMap::new(),
        }
    }

    #[test]
    fn build_and_roundtrip_via_trait() {
        let m = build(&b1_dam_value(), &Protocol::default()).unwrap();
        assert_eq!(m.param_names(), vec!["B1", "A"]);
        assert_eq!(m.output_names(), vec!["B1".to_string(), "A".to_string()]);
        assert_eq!(m.n_volumes(), 2);
        let sig = m.forward(&[0.85, 13_500.0], &Aux::new());
        let fitted = m.fit(&sig, &Aux::new());
        assert!((fitted[0] - 0.85).abs() < 1e-12, "B1: {}", fitted[0]);
        assert!((fitted[1] - 13_500.0).abs() < 1e-6, "A: {}", fitted[1]);
    }

    #[test]
    fn bids_folds_flip_angles_from_protocol() {
        let proto = resolved_proto(&[45.0, 90.0]);
        // Config carries no acquisition; the sidecars supply it.
        let v: serde_yaml::Value = serde_yaml::from_str("model: b1_dam\n").unwrap();
        let m = build(&v, &proto).unwrap();
        assert_eq!(m.n_volumes(), 2);
        let second = m.bids_volume(1);
        assert_eq!(second.entities, vec![("flip", "2".to_string())]);
        assert_eq!(second.sidecar["FlipAngle"], json!(90.0));
    }

    #[test]
    fn build_rejects_a_series_that_is_not_a_double_angle_pair() {
        let proto = resolved_proto(&[60.0, 90.0]);
        let v: serde_yaml::Value = serde_yaml::from_str("model: b1_dam\n").unwrap();
        assert!(build(&v, &proto).is_err());
    }
}
