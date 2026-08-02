//! IR adapter onto the core `Model` trait.

use crate::core::model::{
    Aux, BidsSpec, BidsVolume, EntityRole, FitStrategy, InputSpec, Measurement, MeasurementKind,
    Model, ProtoParam, Protocol, Sample, Scope, Source,
};
use crate::models::inversion_recovery::config::IrConfig;
use crate::models::inversion_recovery::fit::IrFitter;
use anyhow::Result;
use serde_json::json;
use std::collections::BTreeMap;

pub struct IrModel {
    fitter: IrFitter,
    output_names: Vec<String>,
    repetition_time: Option<f64>,
}

/// One `{"InversionTime": ti}` identity row per fitter TI, in canonical order.
fn ir_rows(fitter: &IrFitter) -> Vec<BTreeMap<String, f64>> {
    fitter
        .ti()
        .iter()
        .map(|&ti| BTreeMap::from([("InversionTime".to_string(), ti)]))
        .collect()
}

impl IrModel {
    pub fn new(cfg: IrConfig) -> Self {
        let repetition_time = cfg.repetition_time;
        let fitter = IrFitter::new(&cfg);
        let output_names = fitter
            .output_names()
            .iter()
            .map(|s| s.to_string())
            .collect();
        Self {
            fitter,
            output_names,
            repetition_time,
        }
    }

    /// Assemble a measurement's signal in the fitter's own TI order by matching
    /// each expected TI to its sample by value. Identities must match: assembly
    /// is never positional. A TI with no matching sample is a mislabeled
    /// measurement → panic (the engine records the voxel as a failed fit).
    /// TIs are assumed unique; first match wins (values pass through
    /// unmodified, so a duplicate TI is a misconfiguration, not a hazard).
    fn assemble(&self, m: &Measurement) -> ndarray::Array1<f64> {
        let samples = m.series();
        self.fitter
            .ti()
            .iter()
            .map(|&ti| {
                samples
                    .iter()
                    .find(|s| s.params.get("InversionTime") == Some(&ti))
                    .map(|s| s.value)
                    .unwrap_or_else(|| panic!("measurement has no sample with InversionTime={ti}"))
            })
            .collect()
    }
}

const IR_ENTITIES: &[EntityRole] = &[EntityRole::Inv];

impl Model for IrModel {
    fn param_names(&self) -> Vec<&'static str> {
        IrFitter::param_names().to_vec()
    }
    fn output_names(&self) -> Vec<String> {
        self.output_names.clone()
    }
    fn param_bounds(&self) -> Vec<(f64, f64)> {
        // IR has no explicit bounds; report unbounded.
        vec![(f64::NEG_INFINITY, f64::INFINITY); 3]
    }
    fn fixed_mask(&self) -> Vec<bool> {
        vec![false; 3]
    }
    fn required_inputs(&self) -> Vec<InputSpec> {
        vec![]
    }
    fn measurement(&self) -> MeasurementKind {
        MeasurementKind::Series {
            rows: ir_rows(&self.fitter),
        }
    }
    fn strategy(&self) -> FitStrategy {
        FitStrategy::Voxelwise
    }
    fn forward(&self, params: &[f64], _aux: &Aux) -> Measurement {
        let values = self.fitter.forward(params[0], params[1], params[2]);
        let samples = self
            .fitter
            .ti()
            .iter()
            .zip(values)
            .map(|(&ti, value)| Sample {
                params: BTreeMap::from([("InversionTime".to_string(), ti)]),
                value,
            })
            .collect();
        Measurement::Series(samples)
    }
    fn fit(&self, m: &Measurement, _aux: &Aux) -> Vec<f64> {
        self.fitter.fit_voxel(&self.assemble(m))
    }
    fn fit_block(&self, ms: &[Measurement], _aux: &[Aux]) -> Vec<Vec<f64>> {
        let signals: Vec<ndarray::Array1<f64>> = ms.iter().map(|m| self.assemble(m)).collect();
        self.fitter.fit_block(&signals)
    }
    fn n_volumes(&self) -> usize {
        self.fitter.ti().len()
    }
    fn bids_volume(&self, index: usize) -> BidsVolume {
        let mut sidecar = BTreeMap::new();
        sidecar.insert("InversionTime".to_string(), json!(self.fitter.ti()[index]));
        if let Some(tr) = self.repetition_time {
            sidecar.insert("RepetitionTime".to_string(), json!(tr));
        }
        BidsVolume {
            entities: vec![("inv", (index + 1).to_string())],
            sidecar,
        }
    }
    fn bids(&self) -> Option<BidsSpec> {
        Some(BidsSpec {
            suffix: "IRT1",
            entities: IR_ENTITIES,
        })
    }
    fn protocol_schema(&self) -> Vec<ProtoParam> {
        vec![
            ProtoParam {
                name: "InversionTime",
                source: Source::Field("InversionTime"),
                scope: Scope::PerVolume,
                required: true,
            },
            // Not read by the fit (Barral fits a + b·exp(-TI/T1), no TR term);
            // recorded only so `bids_volume` can echo it into each volume's
            // sidecar. Optional: a dataset whose sidecars omit it must still fit.
            ProtoParam {
                name: "RepetitionTime",
                source: Source::Field("RepetitionTime"),
                scope: Scope::Global,
                required: false,
            },
        ]
    }
    fn bids_outputs(&self) -> Vec<(&'static str, &'static str, &'static str)> {
        // Only `T1` is a genuine qMRLab-convention quantitative map here: `a`
        // and `b` are the fit's offset/amplitude coefficients, not R1map or
        // M0map values (M0map would require a method-specific combination of
        // `a`/`b` qMRLab doesn't expose as a standalone output either), and
        // `res`/`idx` are diagnostics. Do not add R1map/M0map until the model
        // actually produces them.
        vec![("T1", "T1map", "s")]
    }
}

impl crate::core::model::ModelConfig for IrConfig {
    const NAME: &'static str = "inversion_recovery";
    const SUBKEY: Option<&'static str> = None;
    const PROTOCOL_KEYS: &'static [&'static str] = &["inversion_times", "repetition_time"];

    fn validate_options(&mut self) -> Result<()> {
        IrConfig::validate_options(self)
    }

    fn ingest_protocol(&mut self, proto: &Protocol) -> Result<()> {
        if !proto.volumes.is_empty() {
            let tis: Vec<f64> = proto
                .volumes
                .iter()
                .filter_map(|m| m.get("InversionTime").copied())
                .collect();
            if !tis.is_empty() {
                self.inversion_times = tis;
            }
        }
        if let Some(&tr) = proto.global.get("RepetitionTime") {
            self.repetition_time = Some(tr);
        }
        Ok(())
    }

    fn validate_protocol(&mut self) -> Result<()> {
        IrConfig::validate_protocol(self)
    }

    fn into_model(self) -> Box<dyn Model> {
        Box::new(IrModel::new(self))
    }
}

/// Structural interrogation entry point (see [`describe_model`]).
pub fn describe(v: &serde_yaml::Value) -> Result<Box<dyn Model>> {
    crate::core::model::describe_model::<IrConfig>(v)
}

/// Registry builder (see [`build_model`]): the shared parse → ingest protocol →
/// validate → construct pipeline.
pub fn build(v: &serde_yaml::Value, proto: &Protocol) -> Result<Box<dyn Model>> {
    crate::core::model::build_model::<IrConfig>(v, proto)
}

/// Registry dumper (see [`dump_model`](crate::core::model::dump_model)): prints
/// the fully-resolved effective config as YAML.
pub fn dump(v: &serde_yaml::Value) -> Result<String> {
    crate::core::model::dump_model::<IrConfig>(v)
}

/// Registry option-surface entry point (see
/// [`effective_model`](crate::core::model::effective_model)): every option this
/// model accepts, at its effective value, plus any validation complaint.
pub fn effective(
    v: &serde_yaml::Value,
    proto: &Protocol,
) -> Result<crate::core::model::EffectiveConfig> {
    crate::core::model::effective_model::<IrConfig>(v, proto)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A BIDS recipe: options only, the acquisition left to the sidecars. The
    /// build pipeline rejects a recipe that restates an acquisition a resolved
    /// protocol already supplies, so every protocol-driven test below starts
    /// from this rather than from `ir_value`.
    fn ir_bids_value() -> serde_yaml::Value {
        serde_yaml::from_str("model: inversion_recovery\nmethod: complex\n").unwrap()
    }

    fn ir_value() -> serde_yaml::Value {
        serde_yaml::from_str(
            "model: inversion_recovery\nmethod: complex\ninversion_times: [0.350, 0.500, 0.650, 0.800, 0.950, 1.100, 1.250, 1.400, 1.700]\n",
        )
        .unwrap()
    }

    #[test]
    fn build_and_roundtrip_via_trait() {
        let m = build(&ir_value(), &Protocol::default()).unwrap();
        assert_eq!(m.param_names(), vec!["T1", "a", "b"]);
        let sig = m.forward(&[0.9, 500.0, -1000.0], &Aux::new());
        assert_eq!(sig.series().len(), 9);
        let fitted = m.fit(&sig, &Aux::new());
        // output_names[0] == "T1" (seconds)
        assert!((fitted[0] - 0.9).abs() < 1e-3, "T1: {}", fitted[0]);
    }

    #[test]
    fn build_rejects_protocol_with_missing_identity_key() {
        // Four protocol volumes, but only three carry `InversionTime` (the
        // fourth is some unrelated key); the build takes `cfg.inversion_times`
        // from the three matching values (enough to pass the fitter's own
        // minimum-TI-count check), so the model would
        // expect 3 volumes while the protocol supplies 4 — an inconsistency
        // that must fail loudly at build, not per voxel.
        let proto = Protocol {
            volumes: vec![
                BTreeMap::from([("InversionTime".to_string(), 0.350)]),
                BTreeMap::from([("InversionTime".to_string(), 0.500)]),
                BTreeMap::from([("InversionTime".to_string(), 0.650)]),
                BTreeMap::from([("SomeOtherKey".to_string(), 1.0)]),
            ],
            global: BTreeMap::new(),
        };
        let err = match build(&ir_bids_value(), &proto) {
            Ok(_) => panic!("expected build to reject an inconsistent protocol"),
            Err(e) => e,
        };
        let msg = format!("{err:#}");
        assert!(msg.contains("inversion_recovery"), "{msg}");
        assert!(msg.contains("expected 3 volumes"), "{msg}");
        assert!(msg.contains("supplies 4"), "{msg}");
    }

    #[test]
    fn a_resolved_protocol_supplies_the_inversion_times() {
        let mut proto = Protocol::default();
        for ti in [0.350, 0.500, 0.650] {
            let mut mm = std::collections::BTreeMap::new();
            mm.insert("InversionTime".to_string(), ti);
            proto.volumes.push(mm);
        }
        let m = build(&ir_bids_value(), &proto).unwrap();
        let sig = m.forward(&[0.9, 500.0, -1000.0], &Aux::new());
        assert_eq!(sig.series().len(), 3);
    }

    #[test]
    fn declares_bids_irt1() {
        let m = build(&ir_value(), &Protocol::default()).unwrap();
        assert_eq!(m.bids().unwrap().suffix, "IRT1");
    }

    #[test]
    fn declares_inversion_time_protocol_schema() {
        let m = build(&ir_value(), &Protocol::default()).unwrap();
        let schema = m.protocol_schema();
        assert_eq!(schema.len(), 2);
        assert_eq!(schema[0].name, "InversionTime");
        assert!(matches!(schema[0].source, Source::Field("InversionTime")));
        assert!(matches!(schema[0].scope, Scope::PerVolume));
        assert!(schema[0].required);
        assert_eq!(schema[1].name, "RepetitionTime");
        assert!(matches!(schema[1].scope, Scope::Global));
        assert!(!schema[1].required, "TR is not needed to fit IR");
    }

    #[test]
    fn repetition_time_is_folded_from_the_global_protocol() {
        let mut proto = Protocol::default();
        for ti in [0.350, 0.500, 0.650] {
            let mut mm = std::collections::BTreeMap::new();
            mm.insert("InversionTime".to_string(), ti);
            proto.volumes.push(mm);
        }
        proto.global.insert("RepetitionTime".to_string(), 3.5);
        let m = build(&ir_bids_value(), &proto).unwrap();
        let vol = m.bids_volume(0);
        assert_eq!(vol.sidecar["RepetitionTime"], json!(3.5));
    }

    #[test]
    fn builds_fine_without_a_repetition_time_in_the_protocol() {
        // RepetitionTime is not required: a dataset whose sidecars omit it
        // must still fit, and its sidecar simply carries no RepetitionTime.
        let mut proto = Protocol::default();
        for ti in [0.350, 0.500, 0.650] {
            let mut mm = std::collections::BTreeMap::new();
            mm.insert("InversionTime".to_string(), ti);
            proto.volumes.push(mm);
        }
        let m = build(&ir_bids_value(), &proto).unwrap();
        let vol = m.bids_volume(0);
        assert!(!vol.sidecar.contains_key("RepetitionTime"));
    }

    #[test]
    fn describe_succeeds_without_inversion_times_and_exposes_schema() {
        let v: serde_yaml::Value =
            serde_yaml::from_str("model: inversion_recovery\nmethod: magnitude\n").unwrap();
        let m = super::describe(&v).unwrap(); // no inversion_times → still OK
        assert_eq!(m.protocol_schema()[0].name, "InversionTime");
    }

    #[test]
    fn build_still_requires_three_times_when_protocol_empty() {
        let v: serde_yaml::Value = serde_yaml::from_str(
            "model: inversion_recovery\nmethod: magnitude\ninversion_times: [0.35, 0.50]\n",
        )
        .unwrap();
        assert!(super::build(&v, &Protocol::default()).is_err()); // only 2 TIs, no sidecars
    }
}
