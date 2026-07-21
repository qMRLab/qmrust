//! IR adapter onto the core `Model` trait.

use crate::core::model::{
    validate_against_protocol, Aux, BidsSpec, EntityRole, FitStrategy, InputSpec, Measurement,
    MeasurementKind, Model, ProtoParam, Protocol, Sample, Scope, Source,
};
use crate::models::inversion_recovery::config::IrConfig;
use crate::models::inversion_recovery::fit::IrFitter;
use anyhow::{Context, Result};
use std::collections::BTreeMap;

pub struct IrModel {
    fitter: IrFitter,
    output_names: Vec<String>,
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
        let fitter = IrFitter::new(&cfg);
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
        // Assemble the signal in the fitter's own TI order by matching each
        // expected TI to its sample by value. Identities must match: assembly
        // is never positional. A TI with no matching sample is a mislabeled
        // measurement → panic (the engine records the voxel as a failed fit).
        // TIs are assumed unique; first match wins (values pass through
        // unmodified, so a duplicate TI is a misconfiguration, not a hazard).
        let samples = m.series();
        let signal: Vec<f64> = self
            .fitter
            .ti()
            .iter()
            .map(|&ti| {
                samples
                    .iter()
                    .find(|s| s.params.get("InversionTime") == Some(&ti))
                    .map(|s| s.value)
                    .unwrap_or_else(|| panic!("measurement has no sample with InversionTime={ti}"))
            })
            .collect();
        self.fitter.fit_voxel(&ndarray::Array1::from_vec(signal))
    }
    fn bids(&self) -> Option<BidsSpec> {
        Some(BidsSpec {
            suffix: "IRT1",
            entities: IR_ENTITIES,
        })
    }
    fn protocol_schema(&self) -> Vec<ProtoParam> {
        vec![ProtoParam {
            name: "InversionTime",
            source: Source::Field("InversionTime"),
            scope: Scope::PerVolume,
        }]
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

/// Parse the config and run config-intrinsic validation, without composing or
/// validating a protocol. Used to read the model's structural declarations
/// (`protocol_schema`, `bids_outputs`, `required_inputs`) before a protocol
/// exists. Not fit-ready.
pub fn describe(v: &serde_yaml::Value) -> Result<Box<dyn Model>> {
    let cfg: IrConfig = serde_yaml::from_value(v.clone())?;
    cfg.validate_options()?;
    Ok(Box::new(IrModel::new(cfg)))
}

/// Registry builder: parse config, apply any protocol override (BIDS sidecars
/// or `.mat` TIs), validate the finalized protocol, and box a fit-ready model.
pub fn build(v: &serde_yaml::Value, proto: &Protocol) -> Result<Box<dyn Model>> {
    let mut cfg: IrConfig = serde_yaml::from_value(v.clone())?;
    cfg.validate_options()?;
    if !proto.volumes.is_empty() {
        let tis: Vec<f64> = proto
            .volumes
            .iter()
            .filter_map(|m| m.get("InversionTime").copied())
            .collect();
        if !tis.is_empty() {
            cfg.inversion_times = tis;
        }
    }
    cfg.validate_protocol()?;
    let model = IrModel::new(cfg);
    validate_against_protocol(&model.measurement(), proto)
        .context("inversion_recovery: protocol inconsistent with model's measurement")?;
    Ok(Box::new(model))
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // fourth is some unrelated key); the build overrides
        // `cfg.inversion_times` from the three matching values (enough to
        // pass the fitter's own minimum-TI-count check), so the model would
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
        let err = match build(&ir_value(), &proto) {
            Ok(_) => panic!("expected build to reject an inconsistent protocol"),
            Err(e) => e,
        };
        let msg = format!("{err:#}");
        assert!(msg.contains("inversion_recovery"), "{msg}");
        assert!(msg.contains("expected 3 volumes"), "{msg}");
        assert!(msg.contains("supplies 4"), "{msg}");
    }

    #[test]
    fn fit_assembles_by_identity_not_position() {
        // The samples are supplied in REVERSED order with distinct TIs, so a
        // positional assembly would feed the fitter a mirrored (wrong) signal
        // and miss T1; only value-matching recovers 0.9. This test fails if
        // `fit` ever assembles by position.
        let m = build(&ir_value(), &Protocol::default()).unwrap();
        let sig = m.forward(&[0.9, 500.0, -1000.0], &Aux::new());
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
        assert!((a[0] - 0.9).abs() < 1e-3, "in-order T1: {}", a[0]);
        assert!((b[0] - 0.9).abs() < 1e-3, "reordered T1: {}", b[0]);
        assert_eq!(a[0], b[0], "T1 must be identical under reordering");
    }

    #[test]
    #[should_panic(expected = "no sample with InversionTime")]
    fn fit_panics_on_unmatched_identity() {
        // A measurement whose sample identities match no expected TI must fail
        // loudly — never fall back to positional assembly.
        let m = build(&ir_value(), &Protocol::default()).unwrap();
        let bogus = Measurement::Series(vec![Sample {
            params: BTreeMap::from([("InversionTime".to_string(), 99999.0)]),
            value: 1.0,
        }]);
        let _ = m.fit(&bogus, &Aux::new());
    }

    #[test]
    fn mat_protocol_overrides_tis() {
        let mut proto = Protocol::default();
        for ti in [0.350, 0.500, 0.650] {
            let mut mm = std::collections::BTreeMap::new();
            mm.insert("InversionTime".to_string(), ti);
            proto.volumes.push(mm);
        }
        let m = build(&ir_value(), &proto).unwrap();
        let sig = m.forward(&[0.9, 500.0, -1000.0], &Aux::new());
        assert_eq!(sig.series().len(), 3);
    }

    #[test]
    fn declares_bids_irt1() {
        let m = build(&ir_value(), &Protocol::default()).unwrap();
        assert_eq!(m.bids().unwrap().suffix, "IRT1");
    }

    #[test]
    fn bids_outputs_reference_real_output_names() {
        let m = build(&ir_value(), &Protocol::default()).unwrap();
        let names = m.output_names();
        for (out, _suffix, _units) in m.bids_outputs() {
            assert!(
                names.iter().any(|n| n == out),
                "bids_outputs references '{out}', not in output_names {names:?}"
            );
        }
    }

    #[test]
    fn declares_inversion_time_protocol_schema() {
        let m = build(&ir_value(), &Protocol::default()).unwrap();
        let schema = m.protocol_schema();
        assert_eq!(schema.len(), 1);
        assert_eq!(schema[0].name, "InversionTime");
        assert!(matches!(schema[0].source, Source::Field("InversionTime")));
        assert!(matches!(schema[0].scope, Scope::PerVolume));
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
