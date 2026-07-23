//! Monoexponential T2 mapping.
//!
//! Signal model:
//!   S(TE) = M0 * exp(-TE / T2)                     (`offset_term` adds `+ C`)
//!
//! Two fit paths mirror qMRLab's `FitType`:
//!   - `Exponential`: raw signal fitted by bounded Levenberg-Marquardt (the
//!     bounds are qMRLab's `lb`/`ub`, which its `lsqnonlin` enforces by
//!     silently switching from LM to trust-region-reflective when bounds are
//!     present). The signal is fitted un-normalized: qMRLab's anonymous
//!     residual captures `yDat` before the later `yDat = yDat./max(yDat)`
//!     line, so that normalization only sets the M0 start value (`pdInit`),
//!     never the fitted data — M0 therefore comes out on the raw amplitude
//!     scale.
//!   - `Linear`: log-transform, 2-parameter least-squares regression.

use levenberg_marquardt::{LeastSquaresProblem, LevenbergMarquardt};
use nalgebra::{DMatrix, DVector, Dyn, Owned};

use crate::models::mono_t2::config::{FitType, MonoT2Config};

/// qMRLab default fit bounds, in BIDS-native units: T2 in seconds (qMRLab's
/// 1–300 ms) and M0 as a raw signal amplitude (qMRLab's 1–10000). Reported by
/// the model via `param_bounds()` and enforced by the exponential fit.
pub const T2_BOUNDS: (f64, f64) = (0.001, 0.300);
pub const M0_BOUNDS: (f64, f64) = (1.0, 10000.0);
/// The imperfect-refocusing baseline has no qMRLab bound; keep it wide but
/// finite so the sigmoid reparameterization stays well-defined.
const C_BOUNDS: (f64, f64) = (-1.0e6, 1.0e6);

/// M0 start value: qMRLab's `pdInit = max(normalized yDat) * 1.5`, and the
/// normalized signal peaks at 1.
const M0_INIT: f64 = 1.5;
/// Fallback T2 start when the two-point estimate is non-positive/NaN. qMRLab's
/// 30 ms magic constant, expressed in BIDS-native seconds.
const T2_INIT_FALLBACK: f64 = 0.030;

/// Pre-computed fitter for monoexponential T2 data.
///
/// Build once with `new()`, then call `fit_voxel()` from the parallel engine.
pub struct MonoT2Fitter {
    /// Echo times in seconds, ascending (matches `MonoT2Config::validate_protocol`).
    te: Vec<f64>,
    fit_type: FitType,
    drop_first_echo: bool,
    offset_term: bool,
}

impl MonoT2Fitter {
    pub fn new(cfg: &MonoT2Config) -> Self {
        Self {
            te: cfg.echo_times.clone(),
            fit_type: cfg.fit_type,
            drop_first_echo: cfg.drop_first_echo,
            offset_term: cfg.offset_term,
        }
    }

    pub fn output_names(&self) -> [&'static str; 2] {
        ["T2", "M0"]
    }

    pub fn param_names() -> [&'static str; 2] {
        ["T2", "M0"]
    }

    /// Echo times in the order `forward`/`fit_voxel` expect them.
    pub fn te(&self) -> &[f64] {
        &self.te
    }

    /// Noise-free monoexponential signal: `M0 * exp(-TE / T2)`, over every echo.
    pub fn forward(&self, t2: f64, m0: f64) -> Vec<f64> {
        self.te.iter().map(|&te| m0 * (-te / t2).exp()).collect()
    }

    /// Fit a single voxel. Returns `[T2, M0]` (`output_names()` order); T2 in
    /// seconds.
    pub fn fit_voxel(&self, data: &[f64]) -> Vec<f64> {
        // `data` is aligned to `self.te` (ascending). Dropping the first echo
        // therefore drops the shortest-TE sample.
        let start = usize::from(self.drop_first_echo);
        let xd = &self.te[start..];
        let yd = &data[start..];
        let (t2, m0) = match self.fit_type {
            FitType::Exponential => self.fit_exponential(xd, yd),
            FitType::Linear => fit_linear(xd, yd),
        };
        vec![t2, m0]
    }

    /// Bounded LM on the raw signal. Mirrors qMRLab's `lsqnonlin` exponential
    /// path: per-voxel `t2Init` (with a 30 ms fallback), a fixed M0 start of
    /// 1.5, and enforcement of the `lb`/`ub` bounds.
    fn fit_exponential(&self, xd: &[f64], yd: &[f64]) -> (f64, f64) {
        let n = xd.len();
        let t2_init = {
            // qMRLab's two-point estimate; a ratio, so the (dropped) signal
            // normalization would cancel — computed here on the raw signal.
            let dif = xd[0] - xd[n - 2];
            let t = dif / (yd[n - 2].abs() / yd[0].abs()).ln();
            if t <= 0.0 || t.is_nan() {
                T2_INIT_FALLBACK
            } else {
                t
            }
        };

        // Parameter order matches qMRLab: [M0, T2 (, C)].
        let mut st = vec![M0_INIT, t2_init];
        let mut lb = vec![M0_BOUNDS.0, T2_BOUNDS.0];
        let mut ub = vec![M0_BOUNDS.1, T2_BOUNDS.1];
        if self.offset_term {
            st.push(0.0);
            lb.push(C_BOUNDS.0);
            ub.push(C_BOUNDS.1);
        }

        let z0 = DVector::from_iterator(st.len(), (0..st.len()).map(|i| to_z(st[i], lb[i], ub[i])));
        let problem = ExpProblem {
            z: z0,
            lb,
            ub,
            xd: xd.to_vec(),
            yd: yd.to_vec(),
            offset: self.offset_term,
        };
        let (result, _report) = LevenbergMarquardt::new().minimize(problem);
        let p = result.params_bounded();
        (p[1], p[0])
    }
}

/// `Linear` path: log-transform the raw signal, solve the 2-parameter normal
/// equations `[1, TE] \ log(S)`. Mirrors qMRLab's log-transform branch,
/// including its clamping of non-physical T2 to zero.
fn fit_linear(xd: &[f64], yd: &[f64]) -> (f64, f64) {
    let logy: Vec<f64> = yd.iter().map(|v| v.ln()).collect();
    let (intercept, slope) = ols(xd, &logy);

    let m0 = intercept.exp();
    let slope = if slope == 0.0 { f64::EPSILON } else { slope };
    let mut t2 = -1.0 / slope;
    if t2.is_nan() || t2 < 0.0 {
        t2 = 0.0;
    }
    (t2, m0)
}

/// Ordinary least squares of `y = intercept + slope * x`.
fn ols(x: &[f64], y: &[f64]) -> (f64, f64) {
    let n = x.len() as f64;
    let sx: f64 = x.iter().sum();
    let sy: f64 = y.iter().sum();
    let sxx: f64 = x.iter().map(|v| v * v).sum();
    let sxy: f64 = x.iter().zip(y).map(|(a, b)| a * b).sum();
    let denom = n * sxx - sx * sx;
    let slope = (n * sxy - sx * sy) / denom;
    let intercept = (sy - slope * sx) / n;
    (intercept, slope)
}

// ─── Bounded LM internals ─────────────────────────────────────────────────
//
// Bounds are enforced by a smooth sigmoid reparameterization: the optimizer
// works in unconstrained `z`, and each in-bounds parameter is `from_z(z)`.
// (Same construction as `qmt_spgr`'s bounded fit.)

fn sigmoid(z: f64) -> f64 {
    1.0 / (1.0 + (-z).exp())
}
fn logit(p: f64) -> f64 {
    (p / (1.0 - p)).ln()
}
fn to_z(p: f64, lb: f64, ub: f64) -> f64 {
    let frac = ((p - lb) / (ub - lb)).clamp(1e-6, 1.0 - 1e-6);
    logit(frac)
}
fn from_z(z: f64, lb: f64, ub: f64) -> f64 {
    lb + (ub - lb) * sigmoid(z)
}

/// Bounded monoexponential least-squares problem. Parameters are `[M0, T2]`,
/// or `[M0, T2, C]` when `offset`, mapped in/out of their bounds via sigmoid.
struct ExpProblem {
    z: DVector<f64>,
    lb: Vec<f64>,
    ub: Vec<f64>,
    xd: Vec<f64>,
    yd: Vec<f64>,
    offset: bool,
}

impl ExpProblem {
    fn params_bounded(&self) -> Vec<f64> {
        (0..self.z.len())
            .map(|i| from_z(self.z[i], self.lb[i], self.ub[i]))
            .collect()
    }
    fn residual_for(&self, z: &DVector<f64>) -> DVector<f64> {
        let p: Vec<f64> = (0..z.len())
            .map(|i| from_z(z[i], self.lb[i], self.ub[i]))
            .collect();
        let (m0, t2) = (p[0], p[1]);
        let c = if self.offset { p[2] } else { 0.0 };
        DVector::from_iterator(
            self.xd.len(),
            self.xd
                .iter()
                .zip(&self.yd)
                .map(|(&x, &y)| m0 * (-x / t2).exp() + c - y),
        )
    }
}

impl LeastSquaresProblem<f64, Dyn, Dyn> for ExpProblem {
    type ParameterStorage = Owned<f64, Dyn>;
    type ResidualStorage = Owned<f64, Dyn>;
    type JacobianStorage = Owned<f64, Dyn, Dyn>;

    fn set_params(&mut self, p: &DVector<f64>) {
        self.z = p.clone();
    }
    fn params(&self) -> DVector<f64> {
        self.z.clone()
    }
    fn residuals(&self) -> Option<DVector<f64>> {
        Some(self.residual_for(&self.z))
    }
    fn jacobian(&self) -> Option<DMatrix<f64>> {
        let m = self.yd.len();
        let n = self.z.len();
        let mut jac = DMatrix::zeros(m, n);
        for k in 0..n {
            let h = 1e-6 * (1.0 + self.z[k].abs());
            let mut zp = self.z.clone();
            let mut zm = self.z.clone();
            zp[k] += h;
            zm[k] -= h;
            let col = (self.residual_for(&zp) - self.residual_for(&zm)) / (2.0 * h);
            for i in 0..m {
                jac[(i, k)] = col[i];
            }
        }
        Some(jac)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Echo times in seconds (BIDS-native): 30 echoes, 12.8 ms spacing.
    fn default_te() -> Vec<f64> {
        (1..=30).map(|i| i as f64 * 0.0128).collect()
    }

    fn cfg(fit_type: FitType) -> MonoT2Config {
        MonoT2Config {
            echo_times: default_te(),
            fit_type,
            drop_first_echo: false,
            offset_term: false,
        }
    }

    #[test]
    fn exponential_recovers_t2_and_m0() {
        let fitter = MonoT2Fitter::new(&cfg(FitType::Exponential));
        let sig = fitter.forward(0.08, 1000.0);
        let out = fitter.fit_voxel(&sig);
        // Raw (un-normalized) bounded fit recovers both T2 (s) and M0.
        assert!((out[0] - 0.08).abs() < 1e-4, "T2: {}", out[0]);
        assert!((out[1] - 1000.0).abs() < 1.0, "M0: {}", out[1]);
    }

    #[test]
    fn exponential_clamps_t2_to_upper_bound() {
        // A very long true T2 (> 300 ms bound) must clamp at the bound, as the
        // qMRLab reference does (voxels pile up exactly at 300 ms).
        let fitter = MonoT2Fitter::new(&cfg(FitType::Exponential));
        let sig = fitter.forward(2.0, 1000.0); // 2 s ≫ 0.3 s upper bound
        let out = fitter.fit_voxel(&sig);
        assert!(
            out[0] <= T2_BOUNDS.1 + 1e-9,
            "T2 exceeded bound: {}",
            out[0]
        );
        assert!(out[0] > 0.28, "T2 should sit near the bound: {}", out[0]);
    }

    #[test]
    fn linear_recovers_t2_and_m0_exactly() {
        let fitter = MonoT2Fitter::new(&cfg(FitType::Linear));
        let sig = fitter.forward(0.08, 1000.0);
        let out = fitter.fit_voxel(&sig);
        assert!((out[0] - 0.08).abs() < 1e-9, "T2: {}", out[0]);
        assert!((out[1] - 1000.0).abs() < 1e-6, "M0: {}", out[1]);
    }

    #[test]
    fn drop_first_echo_skips_shortest_te() {
        let mut c = cfg(FitType::Linear);
        c.drop_first_echo = true;
        let fitter = MonoT2Fitter::new(&c);
        // Corrupt the first (shortest-TE) sample; dropping it must leave the
        // clean exponential fit intact.
        let mut sig = fitter.forward(0.08, 1000.0);
        sig[0] = 5.0;
        let out = fitter.fit_voxel(&sig);
        assert!((out[0] - 0.08).abs() < 1e-6, "T2: {}", out[0]);
    }

    #[test]
    fn exponential_offset_recovers_t2_with_baseline() {
        let mut c = cfg(FitType::Exponential);
        c.offset_term = true;
        let fitter = MonoT2Fitter::new(&c);
        // Signal with a constant baseline; the offset term must absorb it so T2
        // is still recovered.
        let sig: Vec<f64> = fitter
            .te()
            .iter()
            .map(|&te| 1000.0 * (-te / 0.08).exp() + 50.0)
            .collect();
        let out = fitter.fit_voxel(&sig);
        assert!((out[0] - 0.08).abs() < 1e-3, "T2: {}", out[0]);
    }
}
