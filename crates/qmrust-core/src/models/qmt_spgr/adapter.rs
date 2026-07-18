//! qMT-SPGR adapter onto the core `Model` trait.

use crate::core::model::{
    Aux, BidsMap, BidsSpec, EntityRole, FitStrategy, InputSpec, Model, Protocol,
};
use crate::models::qmt_spgr::config::QmtSpgrConfig;
use crate::models::qmt_spgr::QmtSpgrFitter;
use anyhow::Result;

pub struct QmtModel {
    fitter: QmtSpgrFitter,
    n_rows: usize,
}

impl QmtModel {
    pub fn new(cfg: &QmtSpgrConfig) -> Self {
        Self {
            fitter: QmtSpgrFitter::new(cfg),
            n_rows: cfg.protocol.mtdata.len(),
        }
    }
}

const QMT_ENTITIES: &[EntityRole] = &[EntityRole::Mt, EntityRole::Flip];

impl Model for QmtModel {
    fn param_names(&self) -> Vec<&'static str> {
        vec!["F", "kr", "R1f", "R1r", "T2f", "T2r"]
    }
    fn output_names(&self) -> Vec<String> {
        self.fitter
            .output_names()
            .iter()
            .map(|s| s.to_string())
            .collect()
    }
    fn param_bounds(&self) -> Vec<(f64, f64)> {
        self.fitter.param_bounds().to_vec()
    }
    fn fixed_mask(&self) -> Vec<bool> {
        self.fitter.fixed_mask().to_vec()
    }
    fn required_inputs(&self) -> Vec<InputSpec> {
        vec![
            InputSpec {
                name: "R1map",
                required: false,
                bids: Some(BidsMap {
                    suffix: "T1map",
                    entity: None,
                }),
            },
            InputSpec {
                name: "B1map",
                required: false,
                bids: Some(BidsMap {
                    suffix: "TB1map",
                    entity: None,
                }),
            },
            InputSpec {
                name: "B0map",
                required: false,
                bids: Some(BidsMap {
                    suffix: "B0map",
                    entity: None,
                }),
            },
        ]
    }
    fn n_acquisitions(&self) -> usize {
        self.n_rows
    }
    fn strategy(&self) -> FitStrategy {
        FitStrategy::Voxelwise
    }
    fn forward(&self, params: &[f64], aux: &Aux) -> Vec<f64> {
        let x = [
            params[0], params[1], params[2], params[3], params[4], params[5],
        ];
        let b1 = aux.get("B1map").unwrap_or(1.0);
        let b0 = aux.get("B0map").unwrap_or(0.0);
        let r1 = aux.get("R1map");
        self.fitter.forward(&x, b1, b0, r1)
    }
    fn fit(&self, signal: &[f64], aux: &Aux) -> Vec<f64> {
        let r1 = aux.get("R1map");
        let b1 = aux.get("B1map");
        let b0 = aux.get("B0map");
        self.fitter.fit_voxel(signal, r1, b1, b0)
    }
    fn bids(&self) -> Option<BidsSpec> {
        Some(BidsSpec {
            suffix: "MTS",
            entities: QMT_ENTITIES,
        })
    }
}

/// Registry builder: parse `QmtSpgrConfig` from the `qmt_spgr` sub-key of the
/// raw YAML tree, validate, and box the model. `proto` is currently unused —
/// qMT reads its protocol from its own config; a BIDS protocol source may
/// populate it later.
pub fn build(v: &serde_yaml::Value, _proto: &Protocol) -> Result<Box<dyn Model>> {
    let mut cfg: QmtSpgrConfig = match v.get("qmt_spgr") {
        Some(sub) => serde_yaml::from_value(sub.clone())?,
        None => QmtSpgrConfig::default(),
    };
    cfg.validate()?;
    Ok(Box::new(QmtModel::new(&cfg)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn qmt_value() -> serde_yaml::Value {
        serde_yaml::from_str("model: qmt_spgr\n").unwrap()
    }

    #[test]
    fn build_defaults_and_shapes() {
        let m = build(&qmt_value(), &Protocol::default()).unwrap();
        assert_eq!(m.n_acquisitions(), 10);
        assert_eq!(m.param_names().len(), 6);
        assert_eq!(m.output_names().len(), 8);
        assert_eq!(m.fixed_mask(), vec![false, false, true, true, false, false]);
    }

    #[test]
    fn fit_shape_via_trait() {
        let m = build(&qmt_value(), &Protocol::default()).unwrap();
        let mut aux = Aux::new();
        aux.set("R1map", 1.0);
        let out = m.fit(&[0.5; 10], &aux);
        assert_eq!(out.len(), 8);
    }

    #[test]
    fn declares_bids_mts() {
        let m = build(&qmt_value(), &Protocol::default()).unwrap();
        assert_eq!(m.bids().unwrap().suffix, "MTS");
    }
}
