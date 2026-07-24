//! The portable MTsat B1-correction artifact: the simulated surface, the
//! calibrated M0b-vs-R1 line, and the protocol it was built for (so a
//! correction run is self-describing). Serializes to YAML.

use crate::mtsat_b1::sim::{SeqParams, VfaParams};
use crate::mtsat_b1::surface::SsSurface;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct M0bVsR1 {
    pub slope: f64,
    pub intercept: f64,
}

impl M0bVsR1 {
    pub fn m0b(&self, r1: f64) -> f64 {
        self.slope * r1 + self.intercept
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FitValues {
    pub ss_surface: SsSurface,
    pub m0b_vs_r1: M0bVsR1,
    pub seq: SeqParams,
    pub vfa: VfaParams,
    pub b1_ref: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn m0b_line_evaluates() {
        let l = M0bVsR1 {
            slope: 0.1,
            intercept: 0.02,
        };
        assert!((l.m0b(1.0) - 0.12).abs() < 1e-12);
    }

    #[test]
    fn fitvalues_yaml_roundtrips() {
        let fv = FitValues {
            ss_surface: SsSurface { coeffs: [0.0; 64] },
            m0b_vs_r1: M0bVsR1 {
                slope: 0.1,
                intercept: 0.02,
            },
            seq: crate::mtsat_b1::sim::tests_sample_params(),
            vfa: VfaParams {
                fa1_deg: 5.0,
                fa2_deg: 20.0,
                tr1: 30e-3,
                tr2: 30e-3,
            },
            b1_ref: 6.8,
        };
        let s = serde_yaml::to_string(&fv).unwrap();
        let back: FitValues = serde_yaml::from_str(&s).unwrap();
        assert!((back.b1_ref - 6.8).abs() < 1e-12);
        assert!((back.m0b_vs_r1.slope - 0.1).abs() < 1e-12);
    }
}
