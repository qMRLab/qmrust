//! b1_dam config, parsed from the top-level YAML keys.
//!
//! The double-angle method has no fit options: its estimate is closed form.
//! The only acquisition parameter is the pair of nominal flip angles, in
//! degrees, in acquisition order — `[alpha, 2*alpha]`.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

/// Relative tolerance on the `2*alpha` identity between the two flip angles.
/// Loose enough for a sidecar that rounds (e.g. 59.9998 / 120.0), tight enough
/// to reject a series that is not a double-angle pair at all.
const DOUBLE_ANGLE_RTOL: f64 = 1e-3;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct B1DamConfig {
    /// Nominal flip angles in degrees, in acquisition order: `[alpha, 2*alpha]`.
    #[serde(default)]
    pub flip_angles: Vec<f64>,
}

impl B1DamConfig {
    /// Config-intrinsic validation. The model has no fit options, so there is
    /// nothing to check without a protocol.
    pub fn validate_options(&self) -> Result<()> {
        Ok(())
    }

    /// Protocol-completeness validation: run once the flip angles are final
    /// (from `--config` for non-BIDS, or composed from sidecars for BIDS).
    /// The double-angle identity `S(2a)/(2*S(a)) = cos(a)` only holds when the
    /// second angle really is twice the first, so a series that violates it
    /// would yield a plausible-looking but wrong map.
    pub fn validate_protocol(&mut self) -> Result<()> {
        if self.flip_angles.len() != 2 {
            bail!(
                "b1_dam needs exactly 2 flip angles [alpha, 2*alpha], got {}",
                self.flip_angles.len()
            );
        }
        let (alpha, alpha2) = (self.flip_angles[0], self.flip_angles[1]);
        if alpha <= 0.0 {
            bail!("b1_dam flip angle alpha must be > 0 degrees, got {alpha}");
        }
        if (alpha2 - 2.0 * alpha).abs() > DOUBLE_ANGLE_RTOL * 2.0 * alpha {
            bail!(
                "b1_dam requires the second flip angle to be twice the first, \
                 got alpha={alpha} and {alpha2} degrees"
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_top_level_keys() {
        let v: serde_yaml::Value =
            serde_yaml::from_str("model: b1_dam\nflip_angles: [60, 120]\n").unwrap();
        let mut cfg: B1DamConfig = serde_yaml::from_value(v).unwrap();
        cfg.validate_options().unwrap();
        cfg.validate_protocol().unwrap();
        assert_eq!(cfg.flip_angles, vec![60.0, 120.0]);
    }

    #[test]
    fn validate_options_passes_without_an_acquisition() {
        B1DamConfig::default().validate_options().unwrap();
    }

    #[test]
    fn validate_protocol_requires_a_double_angle_pair() {
        let mut wrong_count = B1DamConfig {
            flip_angles: vec![60.0],
        };
        assert!(wrong_count.validate_protocol().is_err());

        let mut not_doubled = B1DamConfig {
            flip_angles: vec![60.0, 90.0],
        };
        assert!(not_doubled.validate_protocol().is_err());

        let mut nonpositive = B1DamConfig {
            flip_angles: vec![0.0, 0.0],
        };
        assert!(nonpositive.validate_protocol().is_err());

        // A sidecar that rounds still passes.
        let mut rounded = B1DamConfig {
            flip_angles: vec![59.9998, 120.0],
        };
        rounded.validate_protocol().unwrap();
    }
}
