//! IR config, parsed from the top-level YAML keys (IR fields are not nested).

use crate::config::{FitMethod, T1Range, ZoomConfig};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IrConfig {
    #[serde(default)]
    pub inversion_times: Vec<f64>,
    #[serde(default)]
    pub method: Option<FitMethod>,
    #[serde(default)]
    pub t1_range: T1Range,
    #[serde(default)]
    pub zoom: ZoomConfig,
}

impl IrConfig {
    /// Validate + normalize (sort TIs ascending), mirroring the old
    /// `Config::validate` inversion_recovery arm.
    pub fn validate(&mut self) -> Result<()> {
        if self.method.is_none() {
            bail!("inversion_recovery requires a 'method' (magnitude or complex)");
        }
        if self.inversion_times.len() < 3 {
            bail!(
                "At least 3 inversion times required, got {}",
                self.inversion_times.len()
            );
        }
        if self.t1_range.start == 0 {
            bail!("T1 range start must be > 0");
        }
        if self.t1_range.start >= self.t1_range.stop {
            bail!(
                "T1 range start ({}) must be less than stop ({})",
                self.t1_range.start,
                self.t1_range.stop
            );
        }
        if self.zoom.points < 3 {
            bail!("Zoom points must be >= 3, got {}", self.zoom.points);
        }
        self.inversion_times
            .sort_by(|a, b| a.partial_cmp(b).unwrap());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_top_level_ir_keys() {
        let v: serde_yaml::Value = serde_yaml::from_str(
            "model: inversion_recovery\nmethod: complex\ninversion_times: [650, 350, 500]\n",
        )
        .unwrap();
        let mut cfg: IrConfig = serde_yaml::from_value(v).unwrap();
        cfg.validate().unwrap();
        // sorted ascending
        assert_eq!(cfg.inversion_times, vec![350.0, 500.0, 650.0]);
    }

    #[test]
    fn requires_method() {
        let mut cfg = IrConfig {
            inversion_times: vec![1.0, 2.0, 3.0],
            method: None,
            t1_range: Default::default(),
            zoom: Default::default(),
        };
        assert!(cfg.validate().is_err());
    }
}
