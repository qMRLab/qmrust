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
    /// Config-intrinsic validation.
    ///
    /// The model has no fit options, but the shape of the acquisition array is
    /// intrinsic: `describe` runs only this, and a half-stated protocol would
    /// otherwise describe a one-volume collection this model can never fit.
    /// Empty is legitimate (nothing has been resolved yet); anything other than
    /// the pair the method is defined on is not.
    pub fn validate_options(&self) -> Result<()> {
        if !matches!(self.flip_angles.len(), 0 | 2) {
            bail!(
                "b1_dam is defined on exactly 2 flip angles [alpha, 2*alpha], got {}",
                self.flip_angles.len()
            );
        }
        // Stated positively so a non-finite value is rejected too: `fa <= 0.0`
        // is false for NaN, which would otherwise fit an all-NaN map, or be
        // written into a sidecar as `null` by a `bidsify` that never reaches
        // `validate_protocol`.
        if !self
            .flip_angles
            .iter()
            .all(|&fa| fa > 0.0 && fa.is_finite())
        {
            bail!(
                "b1_dam flip angles must be finite and > 0 degrees, got {:?}",
                self.flip_angles
            );
        }
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
        // Both values are already known finite and positive (`validate_options`);
        // what a resolved protocol adds is the relation between them.
        let (alpha, alpha2) = (self.flip_angles[0], self.flip_angles[1]);
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
    fn validate_options_rejects_values_no_protocol_could_make_sense_of() {
        // Properties of the values themselves, so they are checked by the gate
        // `describe` runs: `bidsify` never reaches `validate_protocol`, and
        // would otherwise write a NaN into a sidecar as `null`, or describe a
        // one-volume collection this model can never fit.
        let cfg = |fas: Vec<f64>| B1DamConfig { flip_angles: fas };
        assert!(cfg(vec![60.0]).validate_options().is_err());
        assert!(cfg(vec![0.0, 0.0]).validate_options().is_err());
        assert!(cfg(vec![f64::NAN, 120.0]).validate_options().is_err());
        assert!(cfg(vec![60.0, f64::INFINITY]).validate_options().is_err());
        // Nothing resolved yet is not an error: that is what `describe` sees.
        assert!(cfg(vec![]).validate_options().is_ok());
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

        // A sidecar that rounds still passes.
        let mut rounded = B1DamConfig {
            flip_angles: vec![59.9998, 120.0],
        };
        rounded.validate_protocol().unwrap();
    }
}
