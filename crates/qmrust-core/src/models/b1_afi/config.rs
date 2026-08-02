//! b1_afi config, parsed from the top-level YAML keys.
//!
//! Actual Flip-Angle Imaging has no fit options: its estimate is closed form.
//! The acquisition is the nominal flip angle plus the two interleaved
//! excitation repetition times.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct B1AfiConfig {
    /// The two excitation repetition times in seconds (BIDS/SI), in
    /// acquisition order. Their ratio is what the estimate depends on.
    #[serde(default)]
    pub repetition_times: Vec<f64>,
    /// Nominal excitation flip angle in degrees, shared by both volumes.
    #[serde(default)]
    pub flip_angle: Option<f64>,
}

impl B1AfiConfig {
    /// Config-intrinsic validation. The model has no fit options, so there is
    /// nothing to check without a protocol.
    pub fn validate_options(&self) -> Result<()> {
        Ok(())
    }

    /// Protocol-completeness validation: run once the acquisition is final
    /// (from `--config` for non-BIDS, or composed from sidecars for BIDS).
    pub fn validate_protocol(&mut self) -> Result<()> {
        if self.repetition_times.len() != 2 {
            bail!(
                "b1_afi needs exactly 2 repetition times [TR1, TR2], got {}",
                self.repetition_times.len()
            );
        }
        if self.repetition_times.iter().any(|&tr| tr <= 0.0) {
            bail!(
                "b1_afi repetition times must be > 0 seconds, got {:?}",
                self.repetition_times
            );
        }
        // The estimator divides by `n - r` with n = TR2/TR1; equal repetition
        // times collapse it to 0/0 for every voxel, and the acquisition would
        // not be an AFI pair in the first place.
        if self.repetition_times[0] == self.repetition_times[1] {
            bail!(
                "b1_afi needs two distinct repetition times, both are {}",
                self.repetition_times[0]
            );
        }
        match self.flip_angle {
            None => bail!("b1_afi requires a nominal 'flip_angle' in degrees"),
            Some(fa) if fa <= 0.0 => {
                bail!("b1_afi nominal flip angle must be > 0 degrees, got {fa}")
            }
            Some(_) => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_top_level_keys() {
        let v: serde_yaml::Value =
            serde_yaml::from_str("model: b1_afi\nrepetition_times: [0.02, 0.1]\nflip_angle: 60\n")
                .unwrap();
        let mut cfg: B1AfiConfig = serde_yaml::from_value(v).unwrap();
        cfg.validate_options().unwrap();
        cfg.validate_protocol().unwrap();
        assert_eq!(cfg.repetition_times, vec![0.02, 0.1]);
        assert_eq!(cfg.flip_angle, Some(60.0));
    }

    #[test]
    fn validate_options_passes_without_an_acquisition() {
        B1AfiConfig::default().validate_options().unwrap();
    }

    #[test]
    fn validate_protocol_requires_two_distinct_times_and_a_flip_angle() {
        let cfg = |trs: Vec<f64>, fa: Option<f64>| B1AfiConfig {
            repetition_times: trs,
            flip_angle: fa,
        };
        assert!(cfg(vec![0.02], Some(60.0)).validate_protocol().is_err());
        assert!(cfg(vec![0.0, 0.1], Some(60.0)).validate_protocol().is_err());
        assert!(cfg(vec![0.05, 0.05], Some(60.0))
            .validate_protocol()
            .is_err());
        assert!(cfg(vec![0.02, 0.1], None).validate_protocol().is_err());
        assert!(cfg(vec![0.02, 0.1], Some(0.0)).validate_protocol().is_err());
        // Either acquisition order is accepted; the fitter reads TR1/TR2 by
        // value, not by position.
        cfg(vec![0.1, 0.02], Some(60.0))
            .validate_protocol()
            .unwrap();
    }
}
