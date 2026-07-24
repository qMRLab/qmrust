//! mt_sat config.
//!
//! The acquisition is per-weighting: each of MTw, PDw, T1w carries a nominal
//! flip angle (degrees, BIDS `FlipAngle`) and a repetition time (seconds, BIDS
//! `RepetitionTimeExcitation`). Non-BIDS fits declare these here; BIDS fits fold
//! them from each role's sidecar via `ingest_protocol`. Plus two options: the
//! empirical B1 correction factor and whether to export the MTR map.

use crate::mtsat_b1::fitvalues::FitValues;
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

/// One weighting's acquisition: flip angle in degrees, TR in seconds.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
pub struct Weighting {
    #[serde(default)]
    pub flip_angle: f64,
    #[serde(default)]
    pub repetition_time: f64,
}

fn default_b1_correction_factor() -> f64 {
    0.4
}

fn default_export_mtr() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MtSatConfig {
    #[serde(default)]
    pub mtw: Weighting,
    #[serde(default)]
    pub pdw: Weighting,
    #[serde(default)]
    pub t1w: Weighting,
    /// Empirical transmit-RF correction factor for MTsat (Helms 2015); only
    /// applied when a B1 map is supplied. Default 0.4.
    #[serde(default = "default_b1_correction_factor")]
    pub b1_correction_factor: f64,
    /// Export an MTR map alongside MTsat/T1. Requires TR_MT == TR_PD (MTR is a
    /// ratio of the two same-TR volumes). Default true.
    #[serde(default = "default_export_mtr")]
    pub export_mtr: bool,
    /// Parsed TardifLab B1-correction artifact. `None` → fall back to the
    /// Helms factor (if a B1 map is present) or no correction; `Some` (and a
    /// B1 map present) → the Tardif correction factor instead of Helms. The
    /// recipe references the artifact by path (`{ fitvalues, b1_ref }`); the
    /// CLI resolves the file and inlines the parsed `FitValues` here before
    /// `build` — the core itself never reads files.
    #[serde(default)]
    pub b1_correction: Option<FitValues>,
}

impl Default for MtSatConfig {
    fn default() -> Self {
        Self {
            mtw: Weighting::default(),
            pdw: Weighting::default(),
            t1w: Weighting::default(),
            b1_correction_factor: default_b1_correction_factor(),
            export_mtr: default_export_mtr(),
            b1_correction: None,
        }
    }
}

impl MtSatConfig {
    /// Config-intrinsic validation (no protocol needed).
    pub fn validate_options(&self) -> Result<()> {
        // The B1 correction divides by (1 - factor·B1); keep it in [0, 1) so a
        // physical B1 ≈ 1 never drives the denominator to zero.
        if !(0.0..1.0).contains(&self.b1_correction_factor) {
            bail!(
                "b1_correction_factor must be in [0, 1), got {}",
                self.b1_correction_factor
            );
        }
        Ok(())
    }

    /// Protocol-completeness validation, run after `ingest_protocol`.
    pub fn validate_protocol(&self) -> Result<()> {
        for (name, w) in [("mtw", self.mtw), ("pdw", self.pdw), ("t1w", self.t1w)] {
            if w.flip_angle <= 0.0 {
                bail!(
                    "{name}.flip_angle must be > 0 (degrees), got {}",
                    w.flip_angle
                );
            }
            if w.repetition_time <= 0.0 {
                bail!(
                    "{name}.repetition_time must be > 0 (seconds), got {}",
                    w.repetition_time
                );
            }
        }
        // PDw and T1w must differ in flip angle — that difference is what
        // separates R1 from the signal amplitude. Which of the two is T1w is
        // decided later by flip-angle value (the higher one), not by the
        // flip-1/flip-2 label, so a mislabeled pair is corrected rather than
        // rejected (see `MtSatModel::new`); an equal pair is genuinely
        // ambiguous and cannot be.
        if self.pdw.flip_angle == self.t1w.flip_angle {
            bail!(
                "PDw and T1w must have different flip angles (both {}°); the pair cannot be \
                 disambiguated",
                self.pdw.flip_angle
            );
        }
        // MTR is a ratio of the MT-off/MT-on pair, so it needs the PD-weighted
        // image (the lower-flip-angle MT-off volume, acquired like MTw). Its TR
        // must match MTw's.
        let pd_tr = if self.pdw.flip_angle < self.t1w.flip_angle {
            self.pdw.repetition_time
        } else {
            self.t1w.repetition_time
        };
        if self.export_mtr && self.mtw.repetition_time != pd_tr {
            bail!(
                "export_mtr requires the PD-weighted volume's TR to match MTw's ({} s), got {} s",
                self.mtw.repetition_time,
                pd_tr
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_per_role_protocol_and_options() {
        let v: serde_yaml::Value = serde_yaml::from_str(
            "model: mt_sat\nmtw: {flip_angle: 6, repetition_time: 0.028}\npdw: {flip_angle: 6, repetition_time: 0.028}\nt1w: {flip_angle: 20, repetition_time: 0.018}\n",
        )
        .unwrap();
        let cfg: MtSatConfig = serde_yaml::from_value(v).unwrap();
        cfg.validate_options().unwrap();
        cfg.validate_protocol().unwrap();
        assert_eq!(cfg.b1_correction_factor, 0.4); // default
        assert!(cfg.export_mtr); // default
        assert_eq!(cfg.t1w.flip_angle, 20.0);
    }

    #[test]
    fn validate_options_passes_without_protocol() {
        let cfg = MtSatConfig::default();
        cfg.validate_options().unwrap(); // config-intrinsic only
        assert!(cfg.validate_protocol().is_err()); // zero flip angles rejected
    }

    #[test]
    fn export_mtr_requires_matching_tr() {
        let mut cfg = MtSatConfig {
            mtw: Weighting {
                flip_angle: 6.0,
                repetition_time: 0.028,
            },
            pdw: Weighting {
                flip_angle: 6.0,
                repetition_time: 0.030,
            },
            t1w: Weighting {
                flip_angle: 20.0,
                repetition_time: 0.018,
            },
            ..Default::default()
        };
        assert!(cfg.validate_protocol().is_err());
        cfg.export_mtr = false;
        cfg.validate_protocol().unwrap();
    }

    #[test]
    fn rejects_equal_pdw_t1w_flip_angles() {
        // Equal flip angles cannot be split into a PD/T1 pair (that difference
        // is what separates R1 from amplitude) — genuinely ambiguous, unlike a
        // merely swapped label, which `MtSatModel::new` corrects by FA value.
        let cfg = MtSatConfig {
            mtw: Weighting {
                flip_angle: 6.0,
                repetition_time: 0.028,
            },
            pdw: Weighting {
                flip_angle: 6.0,
                repetition_time: 0.028,
            },
            t1w: Weighting {
                flip_angle: 6.0,
                repetition_time: 0.018,
            },
            ..Default::default()
        };
        let err = cfg.validate_protocol().unwrap_err();
        assert!(err.to_string().contains("different flip angles"), "{err}");
    }

    #[test]
    fn rejects_out_of_range_b1_factor() {
        let cfg = MtSatConfig {
            b1_correction_factor: 1.0,
            ..Default::default()
        };
        assert!(cfg.validate_options().is_err());
    }
}
