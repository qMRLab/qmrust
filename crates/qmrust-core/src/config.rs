use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

/// Fitting method: complex or magnitude data.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FitMethod {
    Magnitude,
    Complex,
}

/// T1 grid search range configuration, in BIDS-native **seconds** (no
/// internal ms↔s conversion — see CLAUDE.md "Units — BIDS-native (SI)").
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct T1Range {
    #[serde(default = "default_t1_start")]
    pub start: f64,
    #[serde(default = "default_t1_stop")]
    pub stop: f64,
    #[serde(default = "default_t1_step")]
    pub step: f64,
}

fn default_t1_start() -> f64 {
    0.001
}
fn default_t1_stop() -> f64 {
    5.0
}
fn default_t1_step() -> f64 {
    0.001
}

impl Default for T1Range {
    fn default() -> Self {
        Self {
            start: default_t1_start(),
            stop: default_t1_stop(),
            step: default_t1_step(),
        }
    }
}

/// Zoom refinement configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ZoomConfig {
    #[serde(default = "default_zoom_iterations")]
    pub iterations: usize,
    #[serde(default = "default_zoom_points")]
    pub points: usize,
}

fn default_zoom_iterations() -> usize {
    2
}
fn default_zoom_points() -> usize {
    21
}

impl Default for ZoomConfig {
    fn default() -> Self {
        Self {
            iterations: default_zoom_iterations(),
            points: default_zoom_points(),
        }
    }
}

fn default_model() -> String {
    "inversion_recovery".to_string()
}

// ─── qmt_spgr configuration ───────────────────────────────────────────────

pub use crate::models::qmt_spgr::config::QmtSpgrConfig;

// ─── simulation configuration ─────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NoiseConfig {
    /// gaussian | rician | none
    #[serde(rename = "type", default = "def_noise_kind")]
    pub kind: String,
    #[serde(default = "def_snr")]
    pub snr: f64,
}
fn def_noise_kind() -> String {
    "none".to_string()
}
fn def_snr() -> f64 {
    100.0
}
impl Default for NoiseConfig {
    fn default() -> Self {
        Self {
            kind: def_noise_kind(),
            snr: def_snr(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SweepConfig {
    pub param: String,
    pub start: f64,
    pub stop: f64,
    pub steps: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DistConfig {
    pub mean: f64,
    pub std: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SimConfig {
    #[serde(default)]
    pub params: std::collections::BTreeMap<String, f64>,
    #[serde(default = "def_b1")]
    pub b1: f64,
    #[serde(default)]
    pub b0: f64,
    #[serde(default)]
    pub r1: Option<f64>,
    #[serde(default)]
    pub noise: NoiseConfig,
    #[serde(default)]
    pub seed: u64,
    #[serde(default = "def_trials")]
    pub trials: usize,
    #[serde(default)]
    pub sweep: Option<SweepConfig>,
    #[serde(default)]
    pub distributions: Option<std::collections::BTreeMap<String, DistConfig>>,
}
fn def_b1() -> f64 {
    1.0
}
fn def_trials() -> usize {
    1
}

impl SimConfig {
    /// Validate config-intrinsic sim settings (noise, trials). Model-specific
    /// sim-input requirements are enforced separately, via each model's
    /// `Model::sim_required_aux` (see `sim::model::validate_sim_inputs`).
    pub fn validate(&self) -> Result<()> {
        match self.noise.kind.as_str() {
            "none" | "gaussian" | "rician" => {}
            other => bail!(
                "sim.noise.type must be none|gaussian|rician, got '{}'",
                other
            ),
        }
        if self.noise.kind != "none" && self.noise.snr <= 0.0 {
            bail!("sim.noise.snr must be > 0 when noise is enabled");
        }
        if self.trials == 0 {
            bail!("sim.trials must be >= 1");
        }
        Ok(())
    }
}

/// Top-level YAML configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default)]
    pub inversion_times: Vec<f64>,
    #[serde(default)]
    pub method: Option<FitMethod>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sim: Option<SimConfig>,
}

impl Config {
    /// Check the top-level config: only that `model` names a registered model.
    /// Per-model validation lives in each model's own config/builder
    /// (`models::*::config`, `models::*::build`).
    pub fn validate(&mut self) -> Result<()> {
        if crate::registry::by_name(&self.model).is_none() {
            bail!("Unknown model: '{}'", self.model);
        }
        Ok(())
    }
}

#[cfg(test)]
mod config_tests {
    use super::*;

    #[test]
    fn existing_ir_config_still_parses() {
        let yaml = "inversion_times: [0.350, 0.500, 0.650]\nmethod: magnitude\n";
        let mut cfg: Config = serde_yaml::from_str(yaml).unwrap();
        cfg.validate().unwrap();
        assert_eq!(cfg.model, "inversion_recovery");
        assert_eq!(cfg.inversion_times.len(), 3);
    }

    #[test]
    fn parse_config_typed_parses_and_validates_from_text() {
        let yaml = "inversion_times: [0.350, 0.500, 0.650]\nmethod: magnitude\n";
        let cfg = parse_config_typed(yaml).unwrap();
        assert_eq!(cfg.model, "inversion_recovery");
        assert_eq!(cfg.inversion_times.len(), 3);
    }

    #[test]
    fn parse_config_typed_rejects_unknown_model() {
        let yaml = "model: not_a_real_model\n";
        let err = parse_config_typed(yaml).unwrap_err();
        assert!(err.to_string().contains("Unknown model"), "got: {}", err);
    }

    #[test]
    fn parse_config_returns_typed_and_raw_tree() {
        let yaml = "inversion_times: [0.350, 0.500, 0.650]\nmethod: magnitude\n";
        let (cfg, raw) = parse_config(yaml).unwrap();
        assert_eq!(cfg.model, "inversion_recovery");
        assert_eq!(
            raw.get("method").and_then(|v| v.as_str()),
            Some("magnitude")
        );
    }
}

/// Parse + validate a config from YAML text (no file I/O).
pub fn parse_config_typed(contents: &str) -> Result<Config> {
    let mut config: Config = serde_yaml::from_str(contents).context("parse config")?;
    config.validate()?;
    Ok(config)
}

/// Parse + validate, also returning the raw YAML tree for per-model builders.
pub fn parse_config(contents: &str) -> Result<(Config, serde_yaml::Value)> {
    let raw: serde_yaml::Value = serde_yaml::from_str(contents).context("parse config")?;
    let config = parse_config_typed(contents)?;
    Ok((config, raw))
}

#[cfg(test)]
mod sim_tests {
    use super::*;

    #[test]
    fn parses_sim_block() {
        let yaml = r#"
model: qmt_spgr
sim:
  params: { F: 0.16, kr: 30.0, R1f: 1.0, R1r: 1.0, T2f: 0.03, T2r: 1.3e-5 }
  noise: { type: rician, snr: 100.0 }
  seed: 7
  trials: 50
"#;
        let mut cfg: Config = serde_yaml::from_str(yaml).unwrap();
        cfg.validate().unwrap();
        let sim = cfg.sim.as_ref().expect("sim block present");
        assert_eq!(sim.seed, 7);
        assert_eq!(sim.trials, 50);
        assert_eq!(sim.noise.kind, "rician");
        assert!((sim.params["F"] - 0.16).abs() < 1e-12);
        assert!((sim.b1 - 1.0).abs() < 1e-12); // default
    }
}
