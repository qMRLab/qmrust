//! IR config, parsed from the top-level YAML keys (IR fields are not nested).

use crate::config::{FitMethod, T1Range, ZoomConfig};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct IrConfig {
    #[serde(default)]
    pub inversion_times: Vec<f64>,
    #[serde(default)]
    pub method: Option<FitMethod>,
    #[serde(default)]
    pub t1_range: T1Range,
    #[serde(default)]
    pub zoom: ZoomConfig,
    #[serde(default)]
    pub repetition_time: Option<f64>,
}

impl IrConfig {
    /// Config-intrinsic validation: options that make sense without a protocol.
    pub fn validate_options(&self) -> Result<()> {
        if self.method.is_none() {
            bail!("inversion_recovery requires a 'method' (magnitude or complex)");
        }
        if self.t1_range.start <= 0.0 {
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
        Ok(())
    }

    /// Protocol-completeness validation: run once the inversion times are final
    /// (from `--config` for non-BIDS, or composed from sidecars for BIDS).
    pub fn validate_protocol(&mut self) -> Result<()> {
        if self.inversion_times.len() < 3 {
            bail!(
                "At least 3 inversion times required, got {}",
                self.inversion_times.len()
            );
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
            "model: inversion_recovery\nmethod: complex\ninversion_times: [0.650, 0.350, 0.500]\n",
        )
        .unwrap();
        let mut cfg: IrConfig = serde_yaml::from_value(v).unwrap();
        cfg.validate_options().unwrap();
        cfg.validate_protocol().unwrap();
        // sorted ascending (seconds)
        assert_eq!(cfg.inversion_times, vec![0.350, 0.500, 0.650]);
    }

    #[test]
    fn requires_method() {
        let cfg = IrConfig {
            inversion_times: vec![1.0, 2.0, 3.0],
            method: None,
            t1_range: Default::default(),
            zoom: Default::default(),
            repetition_time: None,
        };
        assert!(cfg.validate_options().is_err());
    }

    #[test]
    fn validate_options_passes_without_inversion_times() {
        let cfg = IrConfig {
            inversion_times: vec![],
            method: Some(FitMethod::Magnitude),
            t1_range: Default::default(),
            zoom: Default::default(),
            repetition_time: None,
        };
        cfg.validate_options().unwrap(); // config-intrinsic only; no TI-count requirement
    }

    #[test]
    fn validate_protocol_requires_three_times_and_sorts() {
        let mut cfg = IrConfig {
            inversion_times: vec![0.65, 0.35, 0.50],
            method: Some(FitMethod::Magnitude),
            t1_range: Default::default(),
            zoom: Default::default(),
            repetition_time: None,
        };
        cfg.validate_protocol().unwrap();
        assert_eq!(cfg.inversion_times, vec![0.35, 0.50, 0.65]); // sorted
        let mut too_few = IrConfig {
            inversion_times: vec![0.35, 0.50],
            method: Some(FitMethod::Magnitude),
            t1_range: Default::default(),
            zoom: Default::default(),
            repetition_time: None,
        };
        assert!(too_few.validate_protocol().is_err());
    }
}
