//! Bridge from a validated sim config to a `core::Model`, plus helpers to
//! order sim params and build the auxiliary bundle from the sim block.

use crate::config::{Config, SimConfig};
use crate::core::model::{Aux, Model};
use anyhow::{bail, Result};

/// Build the model named by `cfg.model` from the raw YAML tree.
pub fn build_model(cfg: &Config, raw: &serde_yaml::Value) -> Result<Box<dyn Model>> {
    let entry = crate::registry::by_name(&cfg.model)
        .ok_or_else(|| anyhow::anyhow!("sim not supported for model '{}'", cfg.model))?;
    (entry.build)(raw, &crate::core::model::Protocol::default())
}

/// Auxiliary scalars for sim (B1/B0/R1 from the sim block).
pub fn sim_aux(sim: &SimConfig) -> Aux {
    let mut a = Aux::new();
    a.set("B1map", sim.b1);
    a.set("B0map", sim.b0);
    if let Some(r1) = sim.r1 {
        a.set("R1map", r1);
    }
    a
}

/// Order the `sim.params` map into a vector matching `model.param_names()`.
pub fn param_vector(model: &dyn Model, sim: &SimConfig) -> Result<Vec<f64>> {
    let names = model.param_names();
    let mut v = Vec::with_capacity(names.len());
    for name in &names {
        match sim.params.get(*name) {
            Some(&val) => v.push(val),
            None => bail!("sim.params missing required parameter '{}'", name),
        }
    }
    for key in sim.params.keys() {
        if !names.contains(&key.as_str()) {
            eprintln!("warning: sim.params has unknown key '{}' (ignored)", key);
        }
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ir_cfg_raw() -> (Config, serde_yaml::Value) {
        let yaml = "model: inversion_recovery\nmethod: complex\ninversion_times: [350, 500, 650, 800, 950, 1100, 1250, 1400, 1700]\nsim:\n  params: { T1: 900.0, a: 500.0, b: -1000.0 }\n";
        let raw: serde_yaml::Value = serde_yaml::from_str(yaml).unwrap();
        let mut c: Config = serde_yaml::from_str(yaml).unwrap();
        c.validate().unwrap();
        (c, raw)
    }

    #[test]
    fn ir_roundtrip_via_trait() {
        let (cfg, raw) = ir_cfg_raw();
        let model = build_model(&cfg, &raw).unwrap();
        let p = param_vector(model.as_ref(), cfg.sim.as_ref().unwrap()).unwrap();
        assert_eq!(p, vec![900.0, 500.0, -1000.0]);
        let meas = model.forward(&p, &Aux::new());
        let fitted = model.fit(&meas, &Aux::new());
        assert!((fitted[0] - 900.0).abs() < 1.0, "T1: {}", fitted[0]);
    }

    #[test]
    fn missing_param_errors() {
        let yaml = "model: inversion_recovery\nmethod: complex\ninversion_times: [350, 500, 650, 800, 950, 1100, 1250, 1400, 1700]\nsim:\n  params: { T1: 900.0, a: 500.0 }\n";
        let raw: serde_yaml::Value = serde_yaml::from_str(yaml).unwrap();
        let mut c: Config = serde_yaml::from_str(yaml).unwrap();
        c.validate().unwrap();
        let model = build_model(&c, &raw).unwrap();
        let result = param_vector(model.as_ref(), c.sim.as_ref().unwrap());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("b"));
    }
}
