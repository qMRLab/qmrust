//! SuperLorentzian restricted-pool lineshape (computeG.m /
//! superlorentzianLineshape.m) and WB (computeWB.m).

use crate::quad::adaptive_simpson;
use std::f64::consts::PI;

/// SuperLorentzian lineshape value G(delta, T2r), scaled so that
/// W = pi*(omega1^2)*G. Applies near-resonance extrapolation for
/// |delta| <= 1500 Hz to avoid the on-resonance singularity.
pub fn super_lorentzian_g(delta: f64, t2r: f64) -> f64 {
    // Near-resonance extrapolation (superlorentzianLineshape.m, onres=1).
    let d = if delta.abs() <= 1500.0 {
        0.00016 * delta * delta + 1140.0
    } else {
        delta
    };
    let integrand = |u: f64| {
        let denom = 3.0 * u * u - 1.0;
        (2.0 / PI).sqrt()
            * (t2r / denom.abs())
            * (-2.0 * ((2.0 * PI * d * t2r) / denom).powi(2)).exp()
    };
    adaptive_simpson(&integrand, 0.0, 1.0, 1e-8)
}

/// WB = G * pi * w1^2  (computeWB.m).
pub fn compute_wb(w1: f64, delta: f64, t2r: f64) -> f64 {
    super_lorentzian_g(delta, t2r) * PI * w1 * w1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn g_positive() {
        assert!(super_lorentzian_g(2732.0, 1.3e-5) > 0.0);
        assert!(super_lorentzian_g(443.0, 1.3e-5) > 0.0);
    }

    #[test]
    fn g_continuous_across_extrapolation_seam() {
        // Just below and just above the delta=1500 extrapolation boundary
        // should be close (extrapolation is designed to be continuous-ish).
        let below = super_lorentzian_g(1499.0, 1.3e-5);
        let above = super_lorentzian_g(1501.0, 1.3e-5);
        let rel = (below - above).abs() / above;
        assert!(
            rel < 0.05,
            "seam discontinuity too large: {} vs {}",
            below,
            above
        );
    }

    #[test]
    fn wb_scales_with_w1_squared() {
        let a = compute_wb(1.0, 2732.0, 1.3e-5);
        let b = compute_wb(2.0, 2732.0, 1.3e-5);
        assert!((b / a - 4.0).abs() < 1e-9);
    }
}
