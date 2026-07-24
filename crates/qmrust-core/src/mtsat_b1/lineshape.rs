//! Restricted-pool absorption lineshapes, ported verbatim from the TardifLab
//! reference (<ref>/functions/superlor6.m, gaussLineShape.m). Faithful to the
//! reference's conventions — the super-Lorentzian omits the 2π on δ and sums a
//! fixed ctheta grid; the Gaussian keeps the 2π. Do not "fix" either: the
//! calibration/correction are self-consistent with the lineshape that built
//! them (this is why `qmt_spgr`'s super-Lorentzian is NOT reused).

use std::f64::consts::PI;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Lineshape {
    SuperLorentzian,
    Gaussian,
}

/// The lineshape value `G`. Consumed as `Rrfb = π·w1²·G` (super-Lorentzian) or
/// `Rrfb = w1²·G` (gaussian) in the rate matrix. `t2b` in s, `delta` in Hz.
pub fn absorption(shape: Lineshape, t2b: f64, delta: f64) -> f64 {
    match shape {
        Lineshape::SuperLorentzian => super_lorentzian(t2b, delta),
        Lineshape::Gaussian => {
            let expval = (2.0 * PI * delta * t2b).powi(2) / 2.0;
            (PI / 2.0).sqrt() * t2b * (-expval).exp()
        }
    }
}

/// superlor6.m: rectangular sum over ctheta ∈ [0,1] step 1e-3; f1 uses |3ζ²−1|,
/// f2 the squared form; δ enters WITHOUT a 2π factor (reference convention).
fn super_lorentzian(t2b: f64, delta: f64) -> f64 {
    let step = 1e-3;
    let coeff = t2b * (2.0 / PI).sqrt() * step;
    let mut sum = 0.0;
    let n = 1000; // ctheta = 0, 1e-3, ..., 1.0  (1001 points)
    for i in 0..=n {
        let ct = i as f64 * step;
        let sig = 3.0 * ct * ct - 1.0;
        // Reference evaluates at grid points only; ζ = 1/√3 is off-grid, so sig
        // is never exactly 0. Guard defensively anyway.
        if sig == 0.0 {
            continue;
        }
        let f1 = coeff / sig.abs();
        let f2 = (-2.0 * (delta * t2b).powi(2) * sig.powi(-2)).exp();
        sum += f1 * f2;
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;

    // Reference values recomputed from <ref>/functions/superlor6.m and
    // gaussLineShape.m at T2b = 12 µs, δ = 7000 Hz (the sample protocol).
    // superlor6: rectangular sum over ctheta 0:1e-3:1, no 2π on δ.
    #[test]
    fn super_lorentzian_matches_reference_magnitude() {
        let g = absorption(Lineshape::SuperLorentzian, 12e-6, 7000.0);
        // Positive, and O(1e-5 s) for these parameters (sanity + regression).
        assert!(g > 0.0 && g < 1e-3, "G = {g}");
        // Recompute here in-test the exact rectangular sum to pin the value.
        let mut s = 0.0;
        let step = 1e-3;
        let mut ct = 0.0;
        while ct <= 1.0 + 1e-12 {
            let d = (3.0 * ct * ct - 1.0_f64).abs();
            let f1 = 12e-6 * (2.0 / std::f64::consts::PI).sqrt() * step / d;
            let f2 = (-2.0 * (7000.0 * 12e-6_f64).powi(2) * (3.0 * ct * ct - 1.0).powi(-2)).exp();
            s += f1 * f2;
            ct += step;
        }
        assert!((g - s).abs() / s < 1e-9, "{g} vs {s}");
    }

    #[test]
    fn gaussian_uses_angular_offset() {
        let g = absorption(Lineshape::Gaussian, 12e-6, 7000.0);
        let t2b = 12e-6;
        let expval = (2.0 * std::f64::consts::PI * 7000.0 * t2b).powi(2) / 2.0;
        let expected = (std::f64::consts::PI / 2.0).sqrt() * t2b * (-expval).exp();
        assert!((g - expected).abs() / expected < 1e-12, "{g} vs {expected}");
    }
}
