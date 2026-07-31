//! VFA config, parsed from the top-level YAML keys (VFA fields are not nested).

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

/// Fit algorithm. `Linear` is qMRLab's method: the Fram linearization solved in
/// closed form. `Nonlinear` minimises residuals on the signal equation itself
/// by Levenberg-Marquardt.
///
/// The two differ in where the noise lives, not in the model. Dividing by
/// `sin`/`tan` to linearize also divides the noise, so each point is weighted by
/// an angle-dependent factor the least-squares solve does not know about —
/// biasing T1 when SNR is low or the flip angles are far apart. Fitting the
/// signal directly leaves the noise where it was, at the cost of an iterative
/// solve with no closed form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FitType {
    #[default]
    Linear,
    Nonlinear,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct VfaT1Config {
    /// Nominal excitation flip angles in BIDS-native degrees, one per volume.
    #[serde(default)]
    pub flip_angles: Vec<f64>,
    /// Excitation repetition time in seconds, shared by every volume. Both fit
    /// methods assume a single TR across the series.
    #[serde(default)]
    pub repetition_time: Option<f64>,
    #[serde(default)]
    pub fit_type: FitType,
}

impl VfaT1Config {
    /// Config-intrinsic validation: options that make sense without a protocol.
    /// VFA has no options — only an acquisition, checked once the protocol has
    /// been folded in.
    pub fn validate_options(&self) -> Result<()> {
        Ok(())
    }

    /// Protocol-completeness validation: run once the flip angles and TR are
    /// final (from `--config` for non-BIDS, or composed from sidecars for BIDS).
    pub fn validate_protocol(&mut self) -> Result<()> {
        if self.flip_angles.len() < 2 {
            bail!(
                "at least 2 flip angles required, got {}",
                self.flip_angles.len()
            );
        }
        if self.flip_angles.iter().any(|&a| a <= 0.0 || a >= 90.0) {
            bail!(
                "flip angles must lie in (0, 90) degrees, got {:?}",
                self.flip_angles
            );
        }
        match self.repetition_time {
            None => bail!("vfa_t1 requires a repetition_time (seconds)"),
            Some(tr) if tr <= 0.0 => bail!("repetition_time must be > 0, got {tr}"),
            Some(_) => {}
        }
        self.flip_angles.sort_by(|a, b| a.partial_cmp(b).unwrap());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_top_level_keys_and_sorts() {
        let v: serde_yaml::Value =
            serde_yaml::from_str("model: vfa_t1\nflip_angles: [20, 3]\nrepetition_time: 0.015\n")
                .unwrap();
        let mut cfg: VfaT1Config = serde_yaml::from_value(v).unwrap();
        cfg.validate_options().unwrap();
        cfg.validate_protocol().unwrap();
        assert_eq!(cfg.flip_angles, vec![3.0, 20.0]);
        assert_eq!(cfg.repetition_time, Some(0.015));
    }

    #[test]
    fn validate_options_passes_without_acquisition() {
        // `describe` runs options-only, before any sidecar is resolved.
        VfaT1Config::default().validate_options().unwrap();
    }

    #[test]
    fn validate_protocol_requires_two_angles_and_a_tr() {
        let mut one_angle = VfaT1Config {
            flip_angles: vec![3.0],
            repetition_time: Some(0.015),
            fit_type: FitType::Linear,
        };
        assert!(one_angle.validate_protocol().is_err());

        let mut no_tr = VfaT1Config {
            flip_angles: vec![3.0, 20.0],
            repetition_time: None,
            fit_type: FitType::Linear,
        };
        assert!(no_tr.validate_protocol().is_err());
    }

    #[test]
    fn validate_protocol_rejects_out_of_range_angles() {
        // The linearization divides by sin and tan of the flip angle, both
        // degenerate at 0 degrees; tan is singular at 90.
        let mut zero = VfaT1Config {
            flip_angles: vec![0.0, 20.0],
            repetition_time: Some(0.015),
            fit_type: FitType::Linear,
        };
        assert!(zero.validate_protocol().is_err());

        let mut ninety = VfaT1Config {
            flip_angles: vec![3.0, 90.0],
            repetition_time: Some(0.015),
            fit_type: FitType::Linear,
        };
        assert!(ninety.validate_protocol().is_err());
    }
}
