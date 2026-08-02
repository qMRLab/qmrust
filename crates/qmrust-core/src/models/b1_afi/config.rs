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
    /// Config-intrinsic validation.
    ///
    /// The model has no fit options, but the shape of the acquisition array is
    /// intrinsic: `describe` runs only this, and a half-stated protocol would
    /// reach the fitter, which reads both repetition times by index. Empty is
    /// legitimate (nothing has been resolved yet); anything other than the pair
    /// the method is defined on is not.
    pub fn validate_options(&self) -> Result<()> {
        if !matches!(self.repetition_times.len(), 0 | 2) {
            bail!(
                "b1_afi is defined on exactly 2 repetition times [TR1, TR2], got {}",
                self.repetition_times.len()
            );
        }
        // Stated positively so a non-finite value is rejected too: `tr <= 0.0`
        // is false for NaN, which would otherwise fit an all-NaN map, or be
        // written into a sidecar as `null` by a `bidsify` that never reaches
        // `validate_protocol`.
        if !self
            .repetition_times
            .iter()
            .all(|&tr| tr > 0.0 && tr.is_finite())
        {
            bail!(
                "b1_afi repetition times must be finite and > 0 seconds, got {:?}",
                self.repetition_times
            );
        }
        if let Some(fa) = self.flip_angle {
            if !(fa > 0.0 && fa.is_finite()) {
                bail!("b1_afi nominal flip angle must be a finite value > 0 degrees, got {fa}");
            }
        }
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
        // The estimator divides by `n - r` with n = TR2/TR1; equal repetition
        // times collapse it to 0/0 for every voxel, and the acquisition would
        // not be an AFI pair in the first place.
        if self.repetition_times[0] == self.repetition_times[1] {
            bail!(
                "b1_afi needs two distinct repetition times, both are {}",
                self.repetition_times[0]
            );
        }
        // The value itself is checked by `validate_options`; what a resolved
        // protocol adds is that there has to be one.
        if self.flip_angle.is_none() {
            bail!("b1_afi requires a nominal 'flip_angle' in degrees");
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
    fn validate_options_rejects_values_no_protocol_could_make_sense_of() {
        // These are properties of the values themselves, so they are checked by
        // the gate `describe` runs: `bidsify` never reaches `validate_protocol`,
        // and would otherwise write a NaN into a sidecar as `null`, or describe
        // a one-volume collection whose fitter reads two by index.
        let cfg = |trs: Vec<f64>, fa: Option<f64>| B1AfiConfig {
            repetition_times: trs,
            flip_angle: fa,
        };
        assert!(cfg(vec![0.02], Some(60.0)).validate_options().is_err());
        assert!(cfg(vec![0.0, 0.1], Some(60.0)).validate_options().is_err());
        assert!(cfg(vec![f64::NAN, 0.1], Some(60.0))
            .validate_options()
            .is_err());
        assert!(cfg(vec![0.02, f64::INFINITY], Some(60.0))
            .validate_options()
            .is_err());
        assert!(cfg(vec![0.02, 0.1], Some(f64::NAN))
            .validate_options()
            .is_err());
        assert!(cfg(vec![0.02, 0.1], Some(0.0)).validate_options().is_err());
        // Nothing resolved yet is not an error: that is what `describe` sees.
        assert!(cfg(vec![], None).validate_options().is_ok());
    }

    #[test]
    fn validate_protocol_requires_two_distinct_times_and_a_flip_angle() {
        let cfg = |trs: Vec<f64>, fa: Option<f64>| B1AfiConfig {
            repetition_times: trs,
            flip_angle: fa,
        };
        assert!(cfg(vec![0.02], Some(60.0)).validate_protocol().is_err());
        assert!(cfg(vec![0.05, 0.05], Some(60.0))
            .validate_protocol()
            .is_err());
        assert!(cfg(vec![0.02, 0.1], None).validate_protocol().is_err());
        // Either acquisition order is accepted; the fitter reads TR1/TR2 by
        // value, not by position.
        cfg(vec![0.1, 0.02], Some(60.0))
            .validate_protocol()
            .unwrap();
    }
}
