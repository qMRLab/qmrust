//! IR adapter onto the core `Model` trait.

use crate::core::model::{Aux, BidsSpec, EntityRole, FitStrategy, InputSpec, Model, Protocol};
use crate::models::inversion_recovery::config::IrConfig;
use crate::models::inversion_recovery::fit::IrFitter;
use anyhow::Result;

pub struct IrModel {
    fitter: IrFitter,
    output_names: Vec<String>,
    n_ti: usize,
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
            n_ti: cfg.inversion_times.len(),
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
    fn n_acquisitions(&self) -> usize {
        self.n_ti
    }
    fn strategy(&self) -> FitStrategy {
        FitStrategy::Voxelwise
    }
    fn forward(&self, params: &[f64], _aux: &Aux) -> Vec<f64> {
        self.fitter.forward(params[0], params[1], params[2])
    }
    fn fit(&self, signal: &[f64], _aux: &Aux) -> Vec<f64> {
        self.fitter
            .fit_voxel(&ndarray::Array1::from_vec(signal.to_vec()))
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
        assert_eq!(m.n_acquisitions(), 9);
        assert_eq!(m.param_names(), vec!["T1", "a", "b"]);
        let sig = m.forward(&[900.0, 500.0, -1000.0], &Aux::new());
        let fitted = m.fit(&sig, &Aux::new());
        // output_names[0] == "T1"
        assert!((fitted[0] - 900.0).abs() < 1.0, "T1: {}", fitted[0]);
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
        assert_eq!(m.n_acquisitions(), 3);
    }

    #[test]
    fn declares_bids_irt1() {
        let m = build(&ir_value(), &Protocol::default()).unwrap();
        assert_eq!(m.bids().unwrap().suffix, "IRT1");
    }
}
