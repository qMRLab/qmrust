//! Double-Angle Method (DAM) B1+ mapping — Insko & Bolinger (1993).
//!
//! Two spoiled gradient-echo volumes are acquired at nominal flip angles
//! `alpha` and `2*alpha`. With a common (unknown) amplitude the signals are
//! proportional to `sin(theta)` and `sin(2*theta)`, where `theta = B1 * alpha`
//! is the achieved flip angle, so the amplitude cancels in
//!
//! ```text
//!   S(2a) / (2 * S(a)) = sin(2*theta) / (2 * sin(theta)) = cos(theta)
//!   B1 = |acos(S(2a) / (2 * S(a)))| / alpha_radians
//! ```
//!
//! `B1` is dimensionless: the achieved flip angle as a fraction of the nominal
//! one, so `1.0` means the transmit field is exactly on target. This is the
//! same convention the `B1map` aux input of other models expects.
//!
//! The amplitude the ratio divides out is then recovered from either signal as
//! `A = S(a) / sin(theta)`. It is receive-weighted and uncalibrated, so it is a
//! diagnostic rather than a written map — but two measurements determine two
//! unknowns exactly, and without it `forward` could only ever emit the shape of
//! the signal at unit scale, never the signal itself.

use crate::models::b1_dam::config::B1DamConfig;

/// Physically plausible transmit-field range, dimensionless. The estimate is
/// closed form and unconstrained; these bounds are descriptive only.
pub const B1_BOUNDS: (f64, f64) = (0.0, 2.0);

/// Pre-computed fitter for double-angle B1+ data.
pub struct B1DamFitter {
    /// Nominal flip angles in degrees, `[alpha, 2*alpha]`.
    flip_angles: Vec<f64>,
}

impl B1DamFitter {
    pub fn new(cfg: &B1DamConfig) -> Self {
        Self {
            flip_angles: cfg.flip_angles.clone(),
        }
    }

    pub fn param_names() -> [&'static str; 2] {
        ["B1", "A"]
    }

    pub fn output_names() -> [&'static str; 2] {
        ["B1", "A"]
    }

    /// Nominal flip angles in degrees, in the order `forward`/`fit_voxel`
    /// expect them: `[alpha, 2*alpha]`.
    pub fn flip_angles(&self) -> &[f64] {
        &self.flip_angles
    }

    /// The nominal angle the estimate is expressed relative to, in degrees.
    pub fn alpha(&self) -> f64 {
        self.flip_angles[0]
    }

    /// Noise-free DAM signal: `S(theta) = A * sin(B1 * theta)` for each nominal
    /// angle, with `A` the receive-weighted equilibrium amplitude.
    pub fn forward(&self, b1: f64, a: f64) -> Vec<f64> {
        self.flip_angles
            .iter()
            .map(|&fa| a * (b1 * fa.to_radians()).sin())
            .collect()
    }

    /// Fit a single voxel from `[S(alpha), S(2*alpha)]`. Returns values in
    /// `output_names()` order.
    ///
    /// Two measurements, two unknowns: the ratio fixes the achieved angle with
    /// the amplitude divided out, and the amplitude then follows from either
    /// signal. Recovering it is what lets `forward` reproduce the measured
    /// data rather than a unit-scaled shape of it.
    pub fn fit_voxel(&self, signal: &[f64]) -> Vec<f64> {
        let theta = acos_abs(signal[1] / (2.0 * signal[0]));
        vec![theta / self.alpha().to_radians(), signal[0] / theta.sin()]
    }
}

/// `abs(acos(r))` evaluated over the complex plane, as MATLAB's `acos` does for
/// a real argument outside `[-1, 1]`. Noise and imperfect spoiling routinely
/// push the ratio out of domain, and qMRLab reports the magnitude of the
/// complex principal value there rather than a NaN, so voxels stay finite:
///
/// ```text
///   r >  1:  acos(r) = -i*acosh(r)          -> |.| = acosh(r)
///   r < -1:  acos(r) = pi - i*acosh(-r)     -> |.| = hypot(pi, acosh(-r))
/// ```
///
/// A non-finite ratio (a zero or NaN denominator) stays NaN, and the engine
/// records the voxel as unfitted.
fn acos_abs(r: f64) -> f64 {
    if r.is_nan() {
        f64::NAN
    } else if r > 1.0 {
        r.acosh()
    } else if r < -1.0 {
        std::f64::consts::PI.hypot((-r).acosh())
    } else {
        r.acos().abs()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fitter(alpha: f64) -> B1DamFitter {
        B1DamFitter::new(&B1DamConfig {
            flip_angles: vec![alpha, 2.0 * alpha],
        })
    }

    #[test]
    fn forward_then_fit_recovers_b1_and_amplitude() {
        let f = fitter(60.0);
        for &truth in &[0.6, 0.85, 1.0, 1.15, 1.4] {
            for &amp in &[1.0, 13_500.0] {
                let sig = f.forward(truth, amp);
                let out = f.fit_voxel(&sig);
                assert!((out[0] - truth).abs() < 1e-12, "B1={truth}: got {}", out[0]);
                assert!(
                    (out[1] - amp).abs() / amp < 1e-12,
                    "A={amp}: got {}",
                    out[1]
                );
            }
        }
    }

    #[test]
    fn the_fitted_pair_reproduces_the_measured_signal() {
        // What the playground's forward curve plots against the measured
        // points: with the amplitude recovered, the prediction lands on the
        // data rather than on a unit-scaled shape of it near zero.
        let f = fitter(60.0);
        let measured = [13_500.0, 15_600.0];
        let out = f.fit_voxel(&measured);
        let predicted = f.forward(out[0], out[1]);
        for (m, p) in measured.iter().zip(&predicted) {
            assert!((m - p).abs() / m < 1e-12, "measured {m} vs predicted {p}");
        }
    }

    #[test]
    fn a_common_amplitude_cancels() {
        // Only the ratio of the two signals matters, so receive sensitivity and
        // proton density drop out.
        let f = fitter(60.0);
        let plain = f.forward(0.9, 1.0);
        let scaled: Vec<f64> = plain.iter().map(|s| s * 4321.0).collect();
        assert!((f.fit_voxel(&plain)[0] - f.fit_voxel(&scaled)[0]).abs() < 1e-12);
        // The amplitude, by contrast, tracks the scale exactly.
        assert!((f.fit_voxel(&scaled)[1] / f.fit_voxel(&plain)[1] - 4321.0).abs() < 1e-6);
    }

    #[test]
    fn out_of_domain_ratios_follow_matlabs_complex_acos_magnitude() {
        // acos(2) = 1.3170i; acos(-2) = pi - 1.3170i.
        assert!((acos_abs(2.0) - 2.0_f64.acosh()).abs() < 1e-12);
        let expected = (std::f64::consts::PI.powi(2) + 2.0_f64.acosh().powi(2)).sqrt();
        assert!((acos_abs(-2.0) - expected).abs() < 1e-12);
        // In domain it is the ordinary principal value.
        assert!((acos_abs(0.5) - 0.5_f64.acos()).abs() < 1e-12);
        assert!(acos_abs(f64::NAN).is_nan());
    }

    #[test]
    fn a_zero_reference_signal_yields_nan() {
        // 0/0 is NaN; a nonzero numerator over zero is an infinite ratio, whose
        // acosh is also infinite — neither is a usable estimate.
        let f = fitter(60.0);
        assert!(f.fit_voxel(&[0.0, 0.0])[0].is_nan());
        assert!(!f.fit_voxel(&[0.0, 1.0])[0].is_finite());
    }
}
