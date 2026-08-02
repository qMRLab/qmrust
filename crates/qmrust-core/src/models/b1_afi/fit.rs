//! Actual Flip-Angle Imaging (AFI) B1+ mapping — Yarnykh (2007).
//!
//! One spoiled gradient-echo sequence interleaves two excitation repetition
//! times, TR1 and TR2 = n·TR1, at a single nominal flip angle. In the pulsed
//! steady state the ratio of the two signals depends on the achieved flip
//! angle `θ` alone, so proton density and receive sensitivity cancel:
//!
//! ```text
//!   r = |S(TR2) / S(TR1)|          cos θ = (r·n − 1) / (n − r)
//!   B1 = θ / θ_nominal
//! ```
//!
//! `B1` is dimensionless: the achieved flip angle as a fraction of the nominal
//! one, the same convention the `B1map` aux input of other models expects.
//!
//! The closed form above assumes TR ≪ T1, while [`B1AfiFitter::forward`] uses
//! the exact steady-state signal, which also depends on T1. Recovery is
//! therefore not exact — the residual is the method's own systematic bias, and
//! grows as T1 falls toward TR (see `forward_then_fit_recovers_b1_to_within_the_methods_own_bias`).

use crate::models::b1_afi::config::B1AfiConfig;

/// Physically plausible transmit-field range, dimensionless. The estimate is
/// closed form and unconstrained; these bounds are descriptive only.
pub const B1_BOUNDS: (f64, f64) = (0.0, 2.0);

/// Longitudinal relaxation time range in seconds, for the T1 the forward
/// signal depends on. Never fitted (see `Model::fixed_mask`).
pub const T1_BOUNDS: (f64, f64) = (0.0, 10.0);

/// Pre-computed fitter for AFI data.
pub struct B1AfiFitter {
    /// Excitation repetition times in seconds, in acquisition order.
    repetition_times: Vec<f64>,
    /// Nominal excitation flip angle in degrees.
    flip_angle: f64,
}

impl B1AfiFitter {
    pub fn new(cfg: &B1AfiConfig) -> Self {
        Self {
            repetition_times: cfg.repetition_times.clone(),
            // `validate_protocol` guarantees a nominal flip angle on the fit
            // path. `describe` skips it to interrogate a model's structure
            // before any sidecar is resolved, and reads neither `forward` nor
            // `fit_voxel`.
            flip_angle: cfg.flip_angle.unwrap_or(f64::NAN),
        }
    }

    /// Ground-truth parameters in `forward` order. T1 is not recovered by the
    /// fit; it exists because the exact steady-state signal depends on it.
    pub fn param_names() -> [&'static str; 2] {
        ["B1", "T1"]
    }

    pub fn output_names() -> [&'static str; 1] {
        ["B1"]
    }

    /// Excitation repetition times in seconds, in the order
    /// `forward`/`fit_voxel` expect them.
    pub fn repetition_times(&self) -> &[f64] {
        &self.repetition_times
    }

    /// The nominal flip angle the estimate is expressed relative to, degrees.
    pub fn flip_angle(&self) -> f64 {
        self.flip_angle
    }

    /// The shorter repetition time, in seconds. Identified by value rather
    /// than by position, so either acquisition order works and the BIDS
    /// `acq-tr1`/`acq-tr2` labels always name the right volume.
    pub fn tr1(&self) -> f64 {
        self.repetition_times[0].min(self.repetition_times[1])
    }

    /// The longer repetition time, in seconds.
    pub fn tr2(&self) -> f64 {
        self.repetition_times[0].max(self.repetition_times[1])
    }

    /// Exact steady-state AFI signal at unit amplitude, one value per
    /// repetition time in `repetition_times()` order — qMRLab's
    /// `afi_equation`. `b1` scales the nominal flip angle; `t1` is in seconds.
    pub fn forward(&self, b1: f64, t1: f64) -> Vec<f64> {
        let theta = (b1 * self.flip_angle).to_radians();
        let (cos, sin) = (theta.cos(), theta.sin());
        let (tr1, tr2) = (self.tr1(), self.tr2());
        let (e1, e2) = ((-tr1 / t1).exp(), (-tr2 / t1).exp());
        let den = 1.0 - e1 * e2 * cos * cos;
        // The volume acquired at TR1 leads with the *other* interval's
        // recovery, and vice versa — the asymmetry the estimate reads.
        let mz1 = ((1.0 - e2) + (1.0 - e1) * e2 * cos) / den * sin;
        let mz2 = ((1.0 - e1) + (1.0 - e2) * e1 * cos) / den * sin;
        self.repetition_times
            .iter()
            .map(|&tr| if tr == tr1 { mz1 } else { mz2 })
            .collect()
    }

    /// Fit a single voxel from the signals in `repetition_times()` order.
    /// Returns values in `output_names()` order.
    pub fn fit_voxel(&self, signal: &[f64]) -> Vec<f64> {
        let (s1, s2) = if self.repetition_times[0] == self.tr1() {
            (signal[0], signal[1])
        } else {
            (signal[1], signal[0])
        };
        let n = self.tr2() / self.tr1();
        vec![afi_b1(s2 / s1, n, self.flip_angle)]
    }
}

/// The AFI estimate for one voxel: `acos((r·n − 1)/(n − r)) / θ_nominal`,
/// dimensionless, from the signal ratio `r` (signed; magnitude is taken here)
/// and the repetition-time ratio `n`.
///
/// A ratio above 1 is unphysical — the longer-TR volume cannot out-recover the
/// shorter one — so qMRLab attributes it to noise and pins the estimate at
/// zero. It expresses that as the arithmetic `cos_arg·[r ≤ 1] + 1·[r > 1]`,
/// which is reproduced literally below because its IEEE behaviour is part of
/// the contract: `Inf·0` and `NaN·0` are both NaN, so a voxel whose ratio is
/// non-finite (an empty denominator, or `r` landing exactly on `n`) stays NaN
/// instead of being pinned to zero. Writing the pin as an `if`/`else` clamp
/// would silently turn those voxels into `B1 = 0` and change the NaN
/// footprint of the output map.
fn afi_b1(ratio: f64, n: f64, nominal_flip_angle_deg: f64) -> f64 {
    let r = ratio.abs();
    let indicator = |b: bool| if b { 1.0 } else { 0.0 };
    let cos_arg = (r * n - 1.0) / (n - r) * indicator(r <= 1.0) + indicator(r > 1.0);
    cos_arg.acos().to_degrees() / nominal_flip_angle_deg
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fitter(trs: Vec<f64>) -> B1AfiFitter {
        B1AfiFitter::new(&B1AfiConfig {
            repetition_times: trs,
            flip_angle: Some(60.0),
        })
    }

    #[test]
    fn forward_then_fit_recovers_b1_to_within_the_methods_own_bias() {
        // `forward` is the exact steady-state signal; the estimator assumes
        // TR << T1. The gap is AFI's real systematic error, not a porting
        // defect: it is under 1% here and shrinks as T1 grows.
        let f = fitter(vec![0.02, 0.1]);
        for &t1 in &[0.5, 0.9, 2.0] {
            for &truth in &[0.6, 0.8, 1.0, 1.2, 1.4] {
                let got = f.fit_voxel(&f.forward(truth, t1))[0];
                let rel = (got - truth).abs() / truth;
                assert!(rel < 0.01, "B1={truth} T1={t1}: got {got} (rel {rel})");
                // The bias is one-signed: the estimate always undershoots.
                assert!(
                    got <= truth,
                    "B1={truth} T1={t1}: got {got}, expected undershoot"
                );
            }
        }
    }

    #[test]
    fn the_bias_vanishes_as_repetition_times_shrink_against_t1() {
        // Same B1, ever shorter TRs: recovery tends to exact, which is the
        // TR << T1 limit the closed form is derived in. The error falls in
        // proportion to TR/T1, so each tenfold shortening buys a decade.
        let mut previous = f64::INFINITY;
        let mut first = 0.0;
        for (i, scale) in [1.0, 0.1, 0.01].into_iter().enumerate() {
            let f = fitter(vec![0.02 * scale, 0.1 * scale]);
            let err = (f.fit_voxel(&f.forward(1.2, 0.9))[0] - 1.2).abs();
            assert!(err < previous, "error did not shrink at scale {scale}");
            if i == 0 {
                first = err;
            }
            previous = err;
        }
        assert!(
            previous < first / 50.0,
            "shortening TR 100x only improved {first} to {previous}"
        );
    }

    #[test]
    fn a_common_amplitude_cancels() {
        let f = fitter(vec![0.02, 0.1]);
        let plain = f.forward(0.9, 0.9);
        let scaled: Vec<f64> = plain.iter().map(|s| s * 4321.0).collect();
        assert!((f.fit_voxel(&plain)[0] - f.fit_voxel(&scaled)[0]).abs() < 1e-12);
    }

    #[test]
    fn either_acquisition_order_gives_the_same_estimate() {
        // TR1/TR2 are identified by value, so a series listed longest-first
        // must not invert the ratio.
        let ascending = fitter(vec![0.02, 0.1]);
        let descending = fitter(vec![0.1, 0.02]);
        let sig = ascending.forward(1.1, 0.9);
        let flipped: Vec<f64> = sig.iter().rev().copied().collect();
        let a = ascending.fit_voxel(&sig)[0];
        let b = descending.fit_voxel(&flipped)[0];
        assert!((a - b).abs() < 1e-12, "{a} vs {b}");
    }

    #[test]
    fn an_unphysical_ratio_is_pinned_to_zero_but_a_non_finite_one_stays_nan() {
        // r > 1 cannot happen physically, so qMRLab reads it as noise and
        // reports B1 = 0 rather than an out-of-domain arc cosine...
        assert_eq!(afi_b1(1.5, 5.0, 60.0), 0.0);
        // ...but only where the ratio is finite. `r` landing exactly on `n`
        // makes the quotient infinite, and an empty voxel makes it NaN; both
        // stay NaN, which is what keeps the NaN footprint equal to qMRLab's.
        assert!(afi_b1(5.0, 5.0, 60.0).is_nan());
        assert!(afi_b1(f64::NAN, 5.0, 60.0).is_nan());
        assert!(afi_b1(f64::INFINITY, 5.0, 60.0).is_nan());
    }
}
