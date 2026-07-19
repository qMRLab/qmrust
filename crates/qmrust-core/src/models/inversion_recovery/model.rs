//! IR adapter onto the core `Model` trait.

use crate::core::model::{
    Aux, BidsSpec, EntityRole, FitStrategy, InputSpec, Measurement, MeasurementKind, Model,
    Protocol, Sample,
};
use crate::models::inversion_recovery::config::IrConfig;
use crate::models::inversion_recovery::fit::IrFitter;
use anyhow::Result;
use std::collections::BTreeMap;

pub struct IrModel {
    fitter: IrFitter,
    output_names: Vec<String>,
}

/// The single axis an IR series is indexed by.
const IR_AXES: &[&str] = &["InversionTime"];

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
        MeasurementKind::Series { axes: IR_AXES }
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
        // sample's `InversionTime` to the expected TI by value — order-free.
        let samples = m.series();
        let signal: Vec<f64> = self
            .fitter
            .ti()
            .iter()
            .enumerate()
            .map(|(pos, &ti)| {
                samples
                    .iter()
                    .find(|s| s.params.get("InversionTime") == Some(&ti))
                    .map(|s| s.value)
                    .or_else(|| samples.get(pos).map(|s| s.value))
                    .unwrap_or(0.0)
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
}

/// Registry builder: parse `IrConfig` from the raw YAML tree, apply any
/// protocol override (e.g. `.mat` TI values), validate, and box the model.
pub fn build(v: &serde_yaml::Value, proto: &Protocol) -> Result<Box<dyn Model>> {
    let mut cfg: IrConfig = serde_yaml::from_value(v.clone())?;
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
    cfg.validate()?;
    Ok(Box::new(IrModel::new(cfg)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ir_value() -> serde_yaml::Value {
        serde_yaml::from_str(
            "model: inversion_recovery\nmethod: complex\ninversion_times: [350, 500, 650, 800, 950, 1100, 1250, 1400, 1700]\n",
        )
        .unwrap()
    }

    #[test]
    fn build_and_roundtrip_via_trait() {
        let m = build(&ir_value(), &Protocol::default()).unwrap();
        assert_eq!(m.param_names(), vec!["T1", "a", "b"]);
        let sig = m.forward(&[900.0, 500.0, -1000.0], &Aux::new());
        assert_eq!(sig.series().len(), 9);
        let fitted = m.fit(&sig, &Aux::new());
        // output_names[0] == "T1"
        assert!((fitted[0] - 900.0).abs() < 1.0, "T1: {}", fitted[0]);
    }

    #[test]
    fn roundtrip_is_order_free() {
        // Fit with the forward series, then again with its samples reversed:
        // matching by InversionTime must give byte-identical T1.
        let m = build(&ir_value(), &Protocol::default()).unwrap();
        let sig = m.forward(&[900.0, 500.0, -1000.0], &Aux::new());
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
        assert_eq!(a[0], b[0], "T1 must be identical under reordering");
    }

    #[test]
    fn mat_protocol_overrides_tis() {
        let mut proto = Protocol::default();
        for ti in [350.0, 500.0, 650.0] {
            let mut mm = std::collections::BTreeMap::new();
            mm.insert("InversionTime".to_string(), ti);
            proto.volumes.push(mm);
        }
        let m = build(&ir_value(), &proto).unwrap();
        let sig = m.forward(&[900.0, 500.0, -1000.0], &Aux::new());
        assert_eq!(sig.series().len(), 3);
    }

    #[test]
    fn declares_bids_irt1() {
        let m = build(&ir_value(), &Protocol::default()).unwrap();
        assert_eq!(m.bids().unwrap().suffix, "IRT1");
    }
}
