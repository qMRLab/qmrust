//! mono_t2 config, parsed from the top-level YAML keys (fields are not nested).

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

/// Fit algorithm. `Exponential` is the qMRLab default: unconstrained
/// Levenberg-Marquardt on the mono-exponential signal. `Linear` log-transforms
/// the signal and solves a 2-parameter least-squares regression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FitType {
    #[default]
    Exponential,
    Linear,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct MonoT2Config {
    /// Echo times in BIDS-native seconds.
    #[serde(default)]
    pub echo_times: Vec<f64>,
    #[serde(default)]
    pub fit_type: FitType,
    /// Drop the first (shortest-TE) echo, whose spin-echo refocusing is
    /// imperfect.
    #[serde(default)]
    pub drop_first_echo: bool,
    /// Fit an additive constant offset (`Exponential` only) to absorb residual
    /// imperfect-refocusing signal.
    #[serde(default)]
    pub offset_term: bool,
}

impl MonoT2Config {
    /// Config-intrinsic validation: options that make sense without a protocol.
    pub fn validate_options(&self) -> Result<()> {
        if self.offset_term && self.fit_type == FitType::Linear {
            bail!("offset_term is only supported by the exponential fit_type");
        }
        Ok(())
    }

    /// Protocol-completeness validation: run once the echo times are final
    /// (from `--config` for non-BIDS, or composed from sidecars for BIDS).
    pub fn validate_protocol(&mut self) -> Result<()> {
        // The exponential init and the linear regression both read the last two
        // and first echoes, so at least two fitted echoes are required; dropping
        // the first echo therefore needs at least three.
        let min = if self.drop_first_echo { 3 } else { 2 };
        if self.echo_times.len() < min {
            bail!(
                "at least {min} echo times required{}, got {}",
                if self.drop_first_echo {
                    " (drop_first_echo drops one)"
                } else {
                    ""
                },
                self.echo_times.len()
            );
        }
        self.echo_times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_top_level_keys_and_sorts() {
        let v: serde_yaml::Value = serde_yaml::from_str(
            "model: mono_t2\nfit_type: exponential\necho_times: [0.0384, 0.0128, 0.0256]\n",
        )
        .unwrap();
        let mut cfg: MonoT2Config = serde_yaml::from_value(v).unwrap();
        cfg.validate_options().unwrap();
        cfg.validate_protocol().unwrap();
        assert_eq!(cfg.fit_type, FitType::Exponential);
        assert_eq!(cfg.echo_times, vec![0.0128, 0.0256, 0.0384]);
    }

    #[test]
    fn fit_type_defaults_to_exponential() {
        let v: serde_yaml::Value =
            serde_yaml::from_str("model: mono_t2\necho_times: [0.0128, 0.0256]\n").unwrap();
        let cfg: MonoT2Config = serde_yaml::from_value(v).unwrap();
        assert_eq!(cfg.fit_type, FitType::Exponential);
    }

    #[test]
    fn offset_term_rejected_for_linear() {
        let cfg = MonoT2Config {
            echo_times: vec![0.0128, 0.0256],
            fit_type: FitType::Linear,
            drop_first_echo: false,
            offset_term: true,
        };
        assert!(cfg.validate_options().is_err());
    }

    #[test]
    fn drop_first_echo_needs_three() {
        let mut two = MonoT2Config {
            echo_times: vec![0.0128, 0.0256],
            fit_type: FitType::Exponential,
            drop_first_echo: true,
            offset_term: false,
        };
        assert!(two.validate_protocol().is_err());
        let mut three = MonoT2Config {
            echo_times: vec![0.0128, 0.0256, 0.0384],
            fit_type: FitType::Exponential,
            drop_first_echo: true,
            offset_term: false,
        };
        three.validate_protocol().unwrap();
    }
}
