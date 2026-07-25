//! mt_ratio config.
//!
//! MTR is a fixed two-input ratio with no acquisition protocol and no fit
//! options, so its config carries no fields. It exists only to satisfy the
//! shared `ModelConfig` build pipeline; any shared top-level keys (`model`,
//! `sim`) are ignored.

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct MtRatioConfig {}

impl MtRatioConfig {
    /// No options to validate.
    pub fn validate_options(&self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_ignores_shared_keys() {
        let v: serde_yaml::Value = serde_yaml::from_str("model: mt_ratio\n").unwrap();
        let cfg: MtRatioConfig = serde_yaml::from_value(v).unwrap();
        cfg.validate_options().unwrap();
    }
}
