//! Variable flip angle T1 mapping — Fram et al. (1987) linearized SPGR fit.
//!
//! Signal model (spoiled gradient echo at steady state):
//!   S(α) = M0 · sin(α·B1) · (1 − E) / (1 − E·cos(α·B1)),   E = exp(−TR/T1)
//!
//! Dividing through linearizes it: with y = S/sin(α·B1) and x = S/tan(α·B1),
//!   y = E·x + M0(1 − E)
//! so an ordinary least-squares line through (x, y) yields T1 = −TR/ln(slope)
//! and M0 = intercept/(1 − slope). The fit is closed form — no iteration.

use levenberg_marquardt::{LeastSquaresProblem, LevenbergMarquardt};
use nalgebra::{DMatrix, DVector, Dyn, Owned};

use crate::models::vfa_t1::config::FitType;

/// qMRLab's declared bounds, in `param_names()` order [T1, M0] (T1 in seconds).
/// The linearized solve is unconstrained; these describe the physically
/// sensible range rather than constraining the solution.
pub const M0_BOUNDS: (f64, f64) = (0.0, 6000.0);
pub const T1_BOUNDS: (f64, f64) = (1e-5, 5.0);

/// Pre-computed fitter for variable flip angle data.
///
/// Build once with `new()`, then call `fit_voxel()` from the parallel engine.
pub struct VfaT1Fitter {
    /// Nominal flip angles in degrees, in canonical (sorted) acquisition order.
    flip_angles: Vec<f64>,
    /// Excitation repetition time in seconds, shared by every volume.
    tr: f64,
    fit_type: FitType,
}

impl VfaT1Fitter {
    pub fn new(cfg: &super::config::VfaT1Config) -> Self {
        Self {
            flip_angles: cfg.flip_angles.clone(),
            // `validate_protocol` guarantees a TR on the fit path. `describe`
            // skips it to interrogate a model's structure before any sidecar is
            // resolved, and reads neither `forward` nor `fit_voxel`.
            tr: cfg.repetition_time.unwrap_or(f64::NAN),
            fit_type: cfg.fit_type,
        }
    }

    /// Parameter order is `[T1, M0]`, and four things must agree on it:
    /// `param_names`, `output_names`, `forward`'s argument order, and the
    /// order `fit_voxel` returns. Changing one without the others silently
    /// relabels every value that crosses this boundary.
    pub fn param_names() -> [&'static str; 2] {
        ["T1", "M0"]
    }

    pub fn output_names() -> [&'static str; 2] {
        ["T1", "M0"]
    }

    /// Nominal flip angles (degrees) in the order `forward`/`fit_voxel` expect.
    pub fn flip_angles(&self) -> &[f64] {
        &self.flip_angles
    }

    pub fn repetition_time(&self) -> f64 {
        self.tr
    }

    /// Noise-free SPGR signal at each flip angle. `b1` is the normalized
    /// transmit field scaling the nominal angle (`α_actual = b1 · α_nominal`).
    pub fn forward(&self, t1: f64, m0: f64, b1: f64) -> Vec<f64> {
        let e = (-self.tr / t1).exp();
        self.flip_angles
            .iter()
            .map(|&deg| {
                let a = (deg * b1).to_radians();
                m0 * a.sin() * (1.0 - e) / (1.0 - e * a.cos())
            })
            .collect()
    }

    /// Fit a single voxel. Returns values in `output_names()` order — [T1, M0],
    /// with T1 in seconds.
    pub fn fit_voxel(&self, signal: &[f64], b1: f64) -> Vec<f64> {
        let linear = self.fit_linear(signal, b1);
        match self.fit_type {
            FitType::Linear => linear,
            // Seed the iterative solve with the closed-form answer: it is free,
            // already in the right basin, and makes the result reproducible.
            // A failed linearization leaves nothing to refine.
            FitType::Nonlinear if linear[0].is_nan() => linear,
            FitType::Nonlinear => self.fit_nonlinear(signal, b1, linear[0], linear[1]),
        }
    }

    /// qMRLab's method: the Fram linearization solved in closed form. A
    /// non-physical solve (negative slope or intercept, or a degenerate
    /// regression) yields NaN for both, matching `Compute_M0_T1_OnSPGR`.
    fn fit_linear(&self, signal: &[f64], b1: f64) -> Vec<f64> {
        let (x, y): (Vec<f64>, Vec<f64>) = self
            .flip_angles
            .iter()
            .zip(signal)
            .map(|(&deg, &s)| {
                let a = (deg * b1).to_radians();
                (s / a.tan(), s / a.sin())
            })
            .unzip();

        let (slope, intercept) = lin_least_squares(&x, &y);
        if slope.is_nan() || slope < 0.0 || intercept.is_nan() || intercept < 0.0 {
            return vec![f64::NAN, f64::NAN];
        }
        vec![-self.tr / slope.ln(), intercept / (1.0 - slope)]
    }

    /// Levenberg-Marquardt on the signal equation itself, minimising
    /// `S_i − S(α_i; M0, T1)`. Unconstrained, like the other iterative fitters
    /// here — a bounded LM reparameterization distorts the step and is a trap
    /// this codebase has hit before.
    ///
    /// Only a solve this method cannot stand behind is rejected: one that did
    /// not converge, or a T1 that is not a positive finite number (the signal
    /// equation is meaningless there). A large-but-finite T1 is reported as
    /// fitted. Clamping to `T1_BOUNDS` here would make `fit_type` change the
    /// NaN footprint rather than only the estimate, so the two methods would
    /// disagree about which voxels were fitted at all — bounds are a policy
    /// that must apply to both paths or neither, and the qMRLab-faithful
    /// `Linear` default applies neither.
    fn fit_nonlinear(&self, signal: &[f64], b1: f64, t1_init: f64, m0_init: f64) -> Vec<f64> {
        let problem = VfaProblem {
            p: DVector::from_vec(vec![m0_init, t1_init]),
            alpha: self
                .flip_angles
                .iter()
                .map(|&deg| (deg * b1).to_radians())
                .collect(),
            signal: signal.to_vec(),
            tr: self.tr,
        };
        let (result, report) = LevenbergMarquardt::new().minimize(problem);
        let (m0, t1) = (result.p[0], result.p[1]);
        if !report.termination.was_successful() || !m0.is_finite() || !t1.is_finite() || t1 <= 0.0 {
            return vec![f64::NAN, f64::NAN];
        }
        vec![t1, m0]
    }
}

/// Residuals of the SPGR signal equation in `(M0, T1)`, for the `Nonlinear`
/// fit. `alpha` is already B1-scaled and in radians.
struct VfaProblem {
    p: DVector<f64>,
    alpha: Vec<f64>,
    signal: Vec<f64>,
    tr: f64,
}

impl VfaProblem {
    fn residual_for(&self, p: &DVector<f64>) -> DVector<f64> {
        let (m0, t1) = (p[0], p[1]);
        let e = (-self.tr / t1).exp();
        DVector::from_iterator(
            self.alpha.len(),
            self.alpha
                .iter()
                .zip(&self.signal)
                .map(|(&a, &s)| m0 * a.sin() * (1.0 - e) / (1.0 - e * a.cos()) - s),
        )
    }
}

impl LeastSquaresProblem<f64, Dyn, Dyn> for VfaProblem {
    type ParameterStorage = Owned<f64, Dyn>;
    type ResidualStorage = Owned<f64, Dyn>;
    type JacobianStorage = Owned<f64, Dyn, Dyn>;

    fn set_params(&mut self, p: &DVector<f64>) {
        self.p = p.clone();
    }
    fn params(&self) -> DVector<f64> {
        self.p.clone()
    }
    fn residuals(&self) -> Option<DVector<f64>> {
        Some(self.residual_for(&self.p))
    }
    fn jacobian(&self) -> Option<DMatrix<f64>> {
        // Central differences, as the other iterative fitters here do: the
        // analytic Jacobian buys little for two parameters and is one more
        // thing to keep in step with the equation above.
        let m = self.signal.len();
        let n = self.p.len();
        let mut jac = DMatrix::zeros(m, n);
        for k in 0..n {
            let h = 1e-6 * (1.0 + self.p[k].abs());
            let (mut pp, mut pm) = (self.p.clone(), self.p.clone());
            pp[k] += h;
            pm[k] -= h;
            let col = (self.residual_for(&pp) - self.residual_for(&pm)) / (2.0 * h);
            for i in 0..m {
                jac[(i, k)] = col[i];
            }
        }
        Some(jac)
    }
}

/// Ordinary least-squares line through `(x, y)`, returning `(slope, intercept)`.
/// Uses slope = cov(x, y)/cov(x, x) and the fact that the line passes through
/// the sample mean. A zero denominator (all x identical) yields a NaN slope,
/// which the caller treats as a failed fit.
fn lin_least_squares(x: &[f64], y: &[f64]) -> (f64, f64) {
    let n = x.len() as f64;
    let sum_x: f64 = x.iter().sum();
    let sum_y: f64 = y.iter().sum();
    let sum_xy: f64 = x.iter().zip(y).map(|(a, b)| a * b).sum();
    let sum_xx: f64 = x.iter().map(|a| a * a).sum();

    let slope = (sum_xy - sum_x * sum_y / n) / (sum_xx - sum_x * sum_x / n);
    let intercept = sum_y / n - slope * sum_x / n;
    (slope, intercept)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::vfa_t1::config::VfaT1Config;

    /// Read a fit result by name. `fit_voxel` returns `output_names()` order,
    /// and every assertion below reads through these — so reordering the
    /// outputs without updating `output_names` fails the guard test rather than
    /// silently relabelling what each number means.
    fn t1_of(out: &[f64]) -> f64 {
        out[0]
    }
    fn m0_of(out: &[f64]) -> f64 {
        out[1]
    }

    #[test]
    fn outputs_are_named_in_the_order_the_accessors_assume() {
        assert_eq!(VfaT1Fitter::output_names(), ["T1", "M0"]);
        assert_eq!(VfaT1Fitter::param_names(), ["T1", "M0"]);
    }

    fn fitter(flip_angles: Vec<f64>) -> VfaT1Fitter {
        with_type(flip_angles, FitType::Linear)
    }

    fn with_type(flip_angles: Vec<f64>, fit_type: FitType) -> VfaT1Fitter {
        VfaT1Fitter::new(&VfaT1Config {
            flip_angles,
            repetition_time: Some(0.015),
            fit_type,
        })
    }

    /// Deterministic Gaussian noise: a fixed-seed LCG through Box-Muller, so
    /// the comparison below is reproducible without pulling in an RNG crate.
    struct Noise(u64);

    impl Noise {
        fn unit(&mut self) -> f64 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
            ((self.0 >> 11) as f64) / ((1u64 << 53) as f64)
        }
        fn normal(&mut self) -> f64 {
            let u1 = self.unit().max(1e-12);
            let u2 = self.unit();
            (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
        }
        fn apply(&mut self, signal: &[f64], sigma: f64) -> Vec<f64> {
            signal.iter().map(|&s| s + sigma * self.normal()).collect()
        }
    }

    #[test]
    fn nonlinear_recovers_known_params_on_clean_data() {
        let f = with_type(vec![2.0, 5.0, 10.0, 20.0, 30.0], FitType::Nonlinear);
        for &t1 in &[0.2, 0.5, 0.9, 1.5, 3.0] {
            let clean = f.forward(t1, 750.0, 1.0);
            let out = f.fit_voxel(&clean, 1.0);
            assert!(
                (t1_of(&out) - t1).abs() < 1e-6,
                "T1={t1}: got {}",
                t1_of(&out)
            );
            assert!(
                (m0_of(&out) - 750.0).abs() < 1e-3,
                "T1={t1}: M0 {}",
                m0_of(&out)
            );
        }
    }

    #[test]
    fn nonlinear_applies_b1_like_the_linear_path() {
        let f = with_type(vec![3.0, 10.0, 20.0], FitType::Nonlinear);
        let sig = f.forward(0.9, 1000.0, 1.15);
        assert!((t1_of(&f.fit_voxel(&sig, 1.15)) - 0.9).abs() < 1e-6);
    }

    #[test]
    fn nonlinear_beats_the_linearization_across_noise_realizations() {
        // The reason the option exists: linearizing divides the noise by
        // sin/tan, so each point carries an angle-dependent weight the solve is
        // blind to. Fitting the signal directly leaves the noise where it was.
        //
        // This is a statement about the estimator, not about any one draw — on
        // a single realization either method can win by luck. Compare RMS error
        // over many draws, which is the claim actually being made.
        let angles = vec![2.0, 4.0, 8.0, 16.0, 32.0];
        let truth = 1.2;
        let lin = with_type(angles.clone(), FitType::Linear);
        let non = with_type(angles, FitType::Nonlinear);
        let clean = lin.forward(truth, 1000.0, 1.0);

        let mut noise = Noise(0x5EED);
        let (mut sq_lin, mut sq_non, mut n) = (0.0, 0.0, 0u32);
        for _ in 0..400 {
            let data = noise.apply(&clean, 3.0);
            let (a, b) = (
                t1_of(&lin.fit_voxel(&data, 1.0)),
                t1_of(&non.fit_voxel(&data, 1.0)),
            );
            if !a.is_finite() || !b.is_finite() {
                continue; // a failed fit is not evidence either way
            }
            sq_lin += (a - truth).powi(2);
            sq_non += (b - truth).powi(2);
            n += 1;
        }
        assert!(n > 300, "too few usable realizations: {n}");
        let (rmse_lin, rmse_non) = ((sq_lin / n as f64).sqrt(), (sq_non / n as f64).sqrt());
        assert!(
            rmse_non < rmse_lin,
            "nonlinear RMSE {rmse_non:.5} should beat linear {rmse_lin:.5} over {n} draws"
        );
    }

    #[test]
    fn nonlinear_reports_a_failed_linearization_as_a_failed_fit() {
        // Nothing to seed from, so no refinement is possible; both report NaN
        // rather than the nonlinear path inventing a starting point.
        let f = with_type(vec![3.0, 20.0], FitType::Nonlinear);
        let out = f.fit_voxel(&[0.0, 0.0], 1.0);
        assert!(out.iter().all(|v| v.is_nan()), "{out:?}");
    }

    #[test]
    fn both_fit_types_agree_on_which_voxels_were_fitted() {
        // `fit_type` selects an estimator, not a masking policy. An exactly
        // determined acquisition (as many angles as parameters) leaves no
        // residual for the two methods to weigh differently, so they must agree
        // voxel for voxel — including on a long-T1 voxel above the model's
        // declared bound, which neither path is entitled to silently discard.
        let lin = with_type(vec![3.0, 20.0], FitType::Linear);
        let non = with_type(vec![3.0, 20.0], FitType::Nonlinear);
        for &t1 in &[0.5, 1.2, 6.5] {
            let data = lin.forward(t1, 1000.0, 1.0);
            let (a, b) = (lin.fit_voxel(&data, 1.0), non.fit_voxel(&data, 1.0));
            assert_eq!(
                t1_of(&a).is_nan(),
                t1_of(&b).is_nan(),
                "T1={t1}: disagreement on whether the voxel was fitted"
            );
            assert!(
                (t1_of(&a) - t1_of(&b)).abs() < 1e-9,
                "T1={t1}: {a:?} vs {b:?}"
            );
        }
    }

    #[test]
    fn default_fit_type_is_the_qmrlab_linear_path() {
        // Default must not drift: it is what CI compares against qMRLab.
        assert_eq!(VfaT1Config::default().fit_type, FitType::Linear);
    }

    #[test]
    fn forward_then_fit_recovers_params() {
        let f = fitter(vec![3.0, 20.0]);
        let sig = f.forward(0.9, 1000.0, 1.0);
        let out = f.fit_voxel(&sig, 1.0);
        assert!((m0_of(&out) - 1000.0).abs() < 1e-6, "M0: {}", m0_of(&out));
        assert!((t1_of(&out) - 0.9).abs() < 1e-9, "T1: {}", t1_of(&out));
    }

    #[test]
    fn round_trip_over_a_range_of_t1() {
        let f = fitter(vec![2.0, 5.0, 10.0, 20.0, 30.0]);
        for &t1 in &[0.2, 0.5, 0.9, 1.5, 3.0] {
            let out = f.fit_voxel(&f.forward(t1, 750.0, 1.0), 1.0);
            assert!(
                (t1_of(&out) - t1).abs() < 1e-9,
                "T1={t1}: got {}",
                t1_of(&out)
            );
            assert!(
                (m0_of(&out) - 750.0).abs() < 1e-6,
                "T1={t1}: M0 {}",
                m0_of(&out)
            );
        }
    }

    #[test]
    fn b1_scaling_is_inverted_by_the_fit() {
        // A transmit field that departs from nominal biases the fit unless the
        // same B1 is applied; supplying it recovers the truth exactly.
        let f = fitter(vec![3.0, 20.0]);
        let sig = f.forward(0.9, 1000.0, 1.15);
        let corrected = f.fit_voxel(&sig, 1.15);
        assert!(
            (t1_of(&corrected) - 0.9).abs() < 1e-9,
            "T1: {}",
            t1_of(&corrected)
        );
        let uncorrected = f.fit_voxel(&sig, 1.0);
        assert!(
            (t1_of(&uncorrected) - 0.9).abs() > 1e-3,
            "ignoring B1 should bias T1, got {}",
            t1_of(&uncorrected)
        );
    }

    #[test]
    fn non_physical_solve_yields_nan() {
        // A flat (zero) signal gives a degenerate regression, not a T1.
        let f = fitter(vec![3.0, 20.0]);
        let out = f.fit_voxel(&[0.0, 0.0], 1.0);
        assert!(out.iter().all(|v| v.is_nan()), "{out:?}");
    }

    #[test]
    fn linearization_matches_ordinary_least_squares() {
        // y = 2x + 1 exactly.
        let (slope, intercept) = lin_least_squares(&[0.0, 1.0, 2.0, 3.0], &[1.0, 3.0, 5.0, 7.0]);
        assert!((slope - 2.0).abs() < 1e-12);
        assert!((intercept - 1.0).abs() < 1e-12);
    }
}
