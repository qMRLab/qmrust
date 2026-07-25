//! The voxelwise MTsat B1 correction factor, ported from
//! <ref>/functions/MTsat_B1corr_factor_map.m: estimate M0b from R1, evaluate
//! the surface at the achieved vs nominal saturation amplitude, and form the
//! relative change. `MTsat_corr = MTsat·(1 + CF)`.

use crate::mtsat_b1::fitvalues::FitValues;

/// `b1_map` is the relative B1 (≈1 at nominal); `raobs` = R1 in 1/s.
pub fn correction_factor(fv: &FitValues, b1_map: f64, raobs: f64) -> f64 {
    let m0b = fv.m0b_vs_r1.m0b(raobs);
    let ss_act = fv.ss_surface.eval(m0b, fv.b1_ref * b1_map, raobs);
    let ss_nom = fv.ss_surface.eval(m0b, fv.b1_ref, raobs);
    (ss_nom - ss_act) / ss_act
}

pub fn correct(mtsat: f64, cf: f64) -> f64 {
    mtsat * (1.0 + cf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mtsat_b1::fitvalues::{FitValues, M0bVsR1};
    use crate::mtsat_b1::surface::SsSurface;

    fn fv() -> FitValues {
        // Surface depends on b1 so CF is nonzero off-nominal.
        let mut c = [0.0; 64];
        c[0] = 1.0; // const
        c[4] = 0.3; // b1^1  (idx = j*4 = 1*4)
        FitValues {
            ss_surface: SsSurface { coeffs: c },
            m0b_vs_r1: M0bVsR1 {
                slope: 0.05,
                intercept: 0.02,
            },
            seq: crate::mtsat_b1::sim::tests_sample_params(),
            vfa: crate::mtsat_b1::sim::VfaParams {
                fa1_deg: 5.0,
                fa2_deg: 20.0,
                tr1: 30e-3,
                tr2: 30e-3,
            },
            b1_ref: 6.8,
        }
    }

    #[test]
    fn cf_is_zero_at_nominal_b1() {
        // B1_map == 1 → actual == nominal → CF == 0 → MTsat unchanged.
        let cf = correction_factor(&fv(), 1.0, 1.0);
        assert!(cf.abs() < 1e-12, "cf {cf}");
        assert!((correct(3.2, cf) - 3.2).abs() < 1e-12);
    }

    #[test]
    fn cf_nonzero_off_nominal() {
        let cf = correction_factor(&fv(), 1.2, 1.0);
        assert!(cf.abs() > 1e-6, "cf {cf}");
    }
}
