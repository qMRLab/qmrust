//! Single-MTw self-calibration: per-voxel M0b inversion of the surface, then an
//! ordinary-least-squares M0b-vs-R1 line. Ports the single-point path of
//! <ref>/functions/CR_fit_M0b_v1.m and the poly1 regression in
//! sampleCode_calc_M0bappVsR1_1dataset.m.

use crate::mtsat_b1::fitvalues::M0bVsR1;
use crate::mtsat_b1::surface::SsSurface;

/// Find M0b ∈ [0, 0.5] with SS(M0b, b1, raobs) = mtsat_measured by bisection.
/// SS is monotone increasing in M0b over the physical range; if the target is
/// outside `[SS(0), SS(0.5)]` the nearest bound is returned.
pub fn fit_m0b(surface: &SsSurface, b1: f64, raobs: f64, mtsat_measured: f64) -> f64 {
    let g = |m: f64| surface.eval(m, b1, raobs) - mtsat_measured;
    let (mut lo, mut hi) = (0.0_f64, 0.5_f64);
    let (glo, ghi) = (g(lo), g(hi));
    if glo.signum() == ghi.signum() {
        return if glo.abs() < ghi.abs() { lo } else { hi };
    }
    for _ in 0..60 {
        let mid = 0.5 * (lo + hi);
        if g(mid).signum() == glo.signum() {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

/// OLS line m0b = slope·r1 + intercept over `(r1, m0b)` samples.
pub fn regress_m0b_vs_r1(samples: &[(f64, f64)]) -> M0bVsR1 {
    let n = samples.len() as f64;
    let (mut sx, mut sy, mut sxx, mut sxy) = (0.0, 0.0, 0.0, 0.0);
    for &(x, y) in samples {
        sx += x;
        sy += y;
        sxx += x * x;
        sxy += x * y;
    }
    let slope = (n * sxy - sx * sy) / (n * sxx - sx * sx);
    let intercept = (sy - slope * sx) / n;
    M0bVsR1 { slope, intercept }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mtsat_b1::surface::SsSurface;

    // A surface linear+monotone in M0b: MTsat = 10*M0b + 0.5 (ignores b1,raobs).
    fn linear_surface() -> SsSurface {
        let mut c = [0.0; 64];
        c[0] = 0.5; // M0b^0 b1^0 raobs^0
        c[16] = 10.0; // M0b^1 (idx = 1*16)
        SsSurface { coeffs: c }
    }

    #[test]
    fn fit_m0b_inverts_surface() {
        let s = linear_surface();
        let m = fit_m0b(&s, 1.0, 1.0, 10.0 * 0.123 + 0.5);
        assert!((m - 0.123).abs() < 1e-4, "M0b {m}");
    }

    #[test]
    fn regress_recovers_line() {
        let samples: Vec<(f64, f64)> = (0..50)
            .map(|i| {
                let r = 0.4 + i as f64 * 0.02;
                (r, 0.08 * r + 0.01)
            })
            .collect();
        let line = regress_m0b_vs_r1(&samples);
        assert!((line.slope - 0.08).abs() < 1e-6, "slope {}", line.slope);
        assert!(
            (line.intercept - 0.01).abs() < 1e-6,
            "intercept {}",
            line.intercept
        );
    }
}
