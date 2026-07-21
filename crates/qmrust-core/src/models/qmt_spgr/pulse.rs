//! gausshann RF pulse envelope and continuous-wave-equivalent power (w1cw).
//!
//! Ported from qMRLab GetPulse.m + gausshann_pulse.m + compute_w1cw.m.

use crate::quad::adaptive_simpson;

/// Gyromagnetic factor used in qMRLab: 2*pi*42576.
pub const GAMMA: f64 = 2.0 * std::f64::consts::PI * 42576.0;

/// A Hanning-apodized Gaussian MT pulse of duration `trf` (s) and Gaussian
/// bandwidth `bandwidth` (Hz).
pub struct GaussHannPulse {
    pub trf: f64,
    pub bandwidth: f64,
}

impl GaussHannPulse {
    pub fn new(trf: f64, bandwidth: f64) -> Self {
        Self { trf, bandwidth }
    }

    /// Raw (unnormalized) pulse envelope b1(t); zero outside [0, trf].
    pub fn envelope(&self, t: f64) -> f64 {
        if t < 0.0 || t > self.trf {
            return 0.0;
        }
        // gaussian_pulse.m: sigma^2 = 2*log(2)/(pi*bw)^2
        let sigma2 = 2.0 * (2.0_f64).ln() / (std::f64::consts::PI * self.bandwidth).powi(2);
        let gauss = (-((t - self.trf / 2.0).powi(2)) / (2.0 * sigma2)).exp();
        // gausshann_pulse.m: hann apodization
        let hann = 0.5 * (1.0 - (2.0 * std::f64::consts::PI * t / self.trf).cos());
        gauss * hann
    }

    /// Pulse amplitude scaling: amp = 2*pi*alpha / (360 * gamma * ∫ b1).
    pub fn amp(&self, alpha_deg: f64) -> f64 {
        let int_b1 = adaptive_simpson(&|t| self.envelope(t), 0.0, self.trf, 1e-12);
        2.0 * std::f64::consts::PI * alpha_deg / (360.0 * GAMMA * int_b1)
    }

    /// Continuous-wave-equivalent RF power over TR (compute_w1cw.m).
    pub fn w1cw(&self, alpha_deg: f64, tr: f64) -> f64 {
        let amp = self.amp(alpha_deg);
        let int_omega2 = adaptive_simpson(
            &|t| (GAMMA * amp * self.envelope(t)).powi(2),
            0.0,
            self.trf,
            1e-12,
        );
        (int_omega2 / tr).sqrt()
    }

    /// FWHM of the omega² waveform (== FWHM of b1², amp-independent),
    /// sampled on a 1001-point grid over [0, trf] (fwhm.m / compute_w1rp.m).
    pub fn tau(&self) -> f64 {
        let n = 1001usize;
        let xs: Vec<f64> = (0..n)
            .map(|i| self.trf * i as f64 / (n as f64 - 1.0))
            .collect();
        let ys: Vec<f64> = xs.iter().map(|&t| self.envelope(t).powi(2)).collect();
        fwhm(&xs, &ys)
    }

    /// Rectangular-pulse-equivalent power over the FWHM Tau (compute_w1rp.m),
    /// returning (w1rp, tau) together. `tau()` + `w1rp()` compute the same
    /// quantities without recomputing the FWHM per angle; this bundled form is
    /// the reference oracle those two are checked against.
    #[cfg(test)]
    pub fn w1rp_and_tau(&self, alpha_deg: f64) -> (f64, f64) {
        let amp = self.amp(alpha_deg);
        let int_omega2 = adaptive_simpson(
            &|t| (GAMMA * amp * self.envelope(t)).powi(2),
            0.0,
            self.trf,
            1e-12,
        );
        let tau = self.tau();
        ((int_omega2 / tau).sqrt(), tau)
    }

    /// Same as `w1rp_and_tau` but takes a precomputed `tau` (angle-independent
    /// FWHM), avoiding recomputation of the 1001-point FWHM grid per angle.
    pub fn w1rp(&self, alpha_deg: f64, tau: f64) -> f64 {
        let amp = self.amp(alpha_deg);
        let int_omega2 = adaptive_simpson(
            &|t| (GAMMA * amp * self.envelope(t)).powi(2),
            0.0,
            self.trf,
            1e-12,
        );
        (int_omega2 / tau).sqrt()
    }
}

/// Full-width-at-half-maximum of waveform y(x); x ascending. Ported from fwhm.m.
fn fwhm(x: &[f64], y: &[f64]) -> f64 {
    let ymax = y.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let yn: Vec<f64> = y.iter().map(|&v| v / ymax).collect();
    let n = yn.len();
    let lev = 0.5;
    // center index = argmax |yn|
    let center = yn
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.abs().partial_cmp(&b.abs()).unwrap())
        .map(|(i, _)| i)
        .unwrap();
    // first crossing from the left
    let mut i = 1;
    while (yn[i] - lev).signum() == (yn[i - 1] - lev).signum() {
        i += 1;
    }
    let interp = (lev - yn[i - 1]) / (yn[i] - yn[i - 1]);
    let tlead = x[i - 1] + interp * (x[i] - x[i - 1]);
    // next crossing from the center
    let mut i = center + 1;
    while i < n && (yn[i] - lev).signum() == (yn[i - 1] - lev).signum() {
        i += 1;
    }
    if i != n - 1 && i < n {
        let interp = (lev - yn[i - 1]) / (yn[i] - yn[i - 1]);
        let ttrail = x[i - 1] + interp * (x[i] - x[i - 1]);
        ttrail - tlead
    } else {
        f64::NAN
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_zero_outside_window() {
        let p = GaussHannPulse::new(0.0102, 200.0);
        assert_eq!(p.envelope(-0.001), 0.0);
        assert_eq!(p.envelope(0.0103), 0.0);
        assert!(p.envelope(0.0051) > 0.0);
    }

    #[test]
    fn envelope_symmetric_about_center() {
        let p = GaussHannPulse::new(0.0102, 200.0);
        let c = 0.0102 / 2.0;
        let d = 0.002;
        assert!((p.envelope(c - d) - p.envelope(c + d)).abs() < 1e-12);
    }

    #[test]
    fn w1cw_positive_and_increases_with_flip_angle() {
        let p = GaussHannPulse::new(0.0102, 200.0);
        let a = p.w1cw(142.0, 0.025);
        let b = p.w1cw(426.0, 0.025);
        assert!(a > 0.0, "w1cw should be positive, got {}", a);
        assert!(b > a, "larger flip angle -> larger w1cw ({} !> {})", b, a);
    }

    #[test]
    fn amp_matches_w1cw_definition() {
        let p = GaussHannPulse::new(0.0102, 200.0);
        // w1cw = GAMMA*amp*sqrt(∫b1² / TR); recompute from amp and compare.
        let amp = p.amp(142.0);
        let int_b1sq = crate::quad::adaptive_simpson(&|t| p.envelope(t).powi(2), 0.0, p.trf, 1e-12);
        let expected = GAMMA * amp * (int_b1sq / 0.025).sqrt();
        assert!(
            (p.w1cw(142.0, 0.025) - expected).abs() < 1e-6,
            "w1cw vs amp mismatch"
        );
    }

    #[test]
    fn tau_is_fraction_of_trf() {
        let p = GaussHannPulse::new(0.0102, 200.0);
        let tau = p.tau();
        assert!(
            tau > 0.0 && tau < p.trf,
            "tau {} not in (0, {})",
            tau,
            p.trf
        );
    }

    #[test]
    fn w1rp_positive_and_scales_with_flip() {
        let p = GaussHannPulse::new(0.0102, 200.0);
        let (w1rp_a, tau_a) = p.w1rp_and_tau(142.0);
        let (w1rp_b, tau_b) = p.w1rp_and_tau(426.0);
        assert!(w1rp_a > 0.0);
        assert!(w1rp_b > w1rp_a, "larger flip -> larger w1rp");
        assert!((tau_a - tau_b).abs() < 1e-12, "tau is angle-independent");
    }
}
