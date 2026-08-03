//! mono_t2 adapter onto the core `Model` trait.

use crate::core::model::{
    Aux, BidsSpec, BidsVolume, FitStrategy, InputSpec, Measurement, MeasurementKind, Model,
    ProtoParam, Protocol, Scope, SeriesAxis, Source,
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

/// The acquisition axis: one volume per EchoTime.
const AXIS: SeriesAxis = SeriesAxis::new("EchoTime");

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
            rows: AXIS.rows(self.fitter.te()),
        }
    }
    fn strategy(&self) -> FitStrategy {
        FitStrategy::Voxelwise
    }
    fn forward(&self, params: &[f64], _aux: &Aux) -> Measurement {
        let values = self.fitter.forward(params[0], params[1]);
        AXIS.samples(self.fitter.te(), values)
    }
    fn fit(&self, m: &Measurement, _aux: &Aux) -> Vec<f64> {
        self.fitter.fit_voxel(&AXIS.assemble(m, self.fitter.te()))
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
        Some(BidsSpec { suffix: "MESE" })
    }
    fn protocol_schema(&self) -> Vec<ProtoParam> {
        vec![ProtoParam {
            name: "EchoTime",
            source: Source::Field("EchoTime"),
            scope: Scope::PerVolume,
            required: true,
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
    const PROTOCOL_KEYS: &'static [&'static str] = &["echo_times"];

    fn validate_options(&mut self) -> Result<()> {
        MonoT2Config::validate_options(self)
    }

    fn ingest_protocol(&mut self, proto: &Protocol) -> Result<()> {
        let tes = AXIS.ingest(proto);
        if !tes.is_empty() {
            self.echo_times = tes;
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

crate::model_entry_points!(MonoT2Config);

#[cfg(test)]
mod tests {
    use super::*;

    /// A BIDS recipe: options only, the echo times left to the sidecars.
    fn mono_t2_bids_value() -> serde_yaml::Value {
        serde_yaml::from_str("model: mono_t2\nfit_type: exponential\n").unwrap()
    }

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
    fn a_resolved_protocol_supplies_the_echo_times() {
        let mut proto = Protocol::default();
        for te in [0.0128, 0.0256, 0.0384] {
            proto
                .volumes
                .push(BTreeMap::from([("EchoTime".to_string(), te)]));
        }
        let m = build(&mono_t2_bids_value(), &proto).unwrap();
        let sig = m.forward(&[0.08, 1000.0], &Aux::new());
        assert_eq!(sig.series().len(), 3);
    }

    #[test]
    fn declares_bids_mese() {
        let m = build(&mono_t2_value(), &Protocol::default()).unwrap();
        assert_eq!(m.bids().unwrap().suffix, "MESE");
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
