//! Inversion Recovery T1 mapping — Barral et al. (2010) RD-NLS algorithm.
//!
//! Signal model:
//!   Complex:   S(TI) = a + b * exp(-TI / T1)
//!   Magnitude: S(TI) = |a + b * exp(-TI / T1)|

use ndarray::{Array1, Array2};

use crate::config::FitMethod;
use crate::models::inversion_recovery::config::IrConfig;

// ─── Public API ──────────────────────────────────────────────────────────────

/// Pre-computed fitter for inversion recovery data.
///
/// Build once with `new()`, then call `fit_voxel()` from the parallel engine.
pub struct IrFitter {
    nls: NlsStruct,
    method: FitMethod,
    ti: Vec<f64>,
}

impl IrFitter {
    pub fn new(cfg: &IrConfig) -> Self {
        let nls = build_nls_struct(
            &cfg.inversion_times,
            cfg.t1_range.start,
            cfg.t1_range.stop,
            cfg.t1_range.step,
            cfg.zoom.iterations,
            cfg.zoom.points,
        );
        Self {
            nls,
            method: cfg.method.clone().expect("IR requires method"),
            ti: cfg.inversion_times.clone(),
        }
    }

    pub fn output_names(&self) -> &[&str] {
        match self.method {
            FitMethod::Magnitude => &["T1", "b", "a", "res", "idx"],
            FitMethod::Complex => &["T1", "b", "a", "res"],
        }
    }

    pub fn param_names() -> [&'static str; 3] {
        ["T1", "a", "b"]
    }

    /// Inversion times in the order `forward`/`fit_voxel` expect them.
    pub fn ti(&self) -> &[f64] {
        &self.ti
    }

    /// Noise-free IR signal: a + b*exp(-TI/T1); magnitude method takes |·|.
    pub fn forward(&self, t1: f64, a: f64, b: f64) -> Vec<f64> {
        self.ti
            .iter()
            .map(|&ti| {
                let s = a + b * (-ti / t1).exp();
                match self.method {
                    FitMethod::Magnitude => s.abs(),
                    FitMethod::Complex => s,
                }
            })
            .collect()
    }

    /// Fit a single voxel. Returns values in `output_names()` order.
    pub fn fit_voxel(&self, data: &Array1<f64>) -> Vec<f64> {
        let r = match self.method {
            FitMethod::Complex => rd_nls(data, &self.nls),
            FitMethod::Magnitude => rd_nls_pr(data, &self.nls),
        };
        let mut v = vec![r.t1, r.b, r.a, r.residual];
        if let Some(idx) = r.idx {
            v.push(idx as f64);
        }
        v
    }
}

// ─── RD-NLS internals ───────────────────────────────────────────────────────

/// Pre-computed search grid (equivalent to MATLAB's nlsS struct).
struct NlsStruct {
    t_vec: Array1<f64>,
    n: usize,
    t1_vec: Array1<f64>,
    the_exp: Array2<f64>,
    rho_norm_vec: Array1<f64>,
    nbr_of_zoom: usize,
    t1_len_z: usize,
}

struct FitResult {
    t1: f64,
    b: f64,
    a: f64,
    residual: f64,
    idx: Option<usize>,
}

fn linspace(start: f64, stop: f64, n: usize) -> Array1<f64> {
    if n <= 1 {
        return Array1::from_vec(vec![start]);
    }
    let step = (stop - start) / (n - 1) as f64;
    Array1::from_iter((0..n).map(|i| start + step * i as f64))
}

fn compute_exp_and_norm(t_vec: &Array1<f64>, t1_vec: &Array1<f64>) -> (Array2<f64>, Array1<f64>) {
    let n = t_vec.len();
    let t1_len = t1_vec.len();
    let n_f = n as f64;

    let mut the_exp = Array2::<f64>::zeros((n, t1_len));
    for j in 0..t1_len {
        let alpha = 1.0 / t1_vec[j];
        for i in 0..n {
            the_exp[[i, j]] = (-t_vec[i] * alpha).exp();
        }
    }

    let mut rho_norm_vec = Array1::<f64>::zeros(t1_len);
    for j in 0..t1_len {
        let mut sum_sq = 0.0;
        let mut sum_val = 0.0;
        for i in 0..n {
            let v = the_exp[[i, j]];
            sum_sq += v * v;
            sum_val += v;
        }
        rho_norm_vec[j] = sum_sq - (1.0 / n_f) * sum_val * sum_val;
    }

    (the_exp, rho_norm_vec)
}

/// Build the RD-NLS search grid. `ti_values` and the T1 grid
/// (`t1_start`/`t1_stop`/`t1_step`) are all in BIDS-native **seconds**; the
/// fit is scale-consistent, so whatever unit TI and the T1 grid share is the
/// unit the fitted T1 comes out in.
fn build_nls_struct(
    ti_values: &[f64],
    t1_start: f64,
    t1_stop: f64,
    t1_step: f64,
    nbr_of_zoom: usize,
    t1_len_z: usize,
) -> NlsStruct {
    let t_vec = Array1::from_vec(ti_values.to_vec());
    let n = t_vec.len();
    // Float grid (seconds): n points from t1_start to t1_stop inclusive,
    // spaced by t1_step (rounded to the nearest integer point count).
    let n_t1 = ((t1_stop - t1_start) / t1_step).round() as usize + 1;
    let t1_vec = Array1::from_iter((0..n_t1).map(|i| t1_start + i as f64 * t1_step));
    let (the_exp, rho_norm_vec) = compute_exp_and_norm(&t_vec, &t1_vec);

    NlsStruct {
        t_vec,
        n,
        t1_vec,
        the_exp,
        rho_norm_vec,
        nbr_of_zoom,
        t1_len_z,
    }
}

/// Grid search: argmax_j |rhoTyVec[j]|² / rhoNormVec[j].
fn grid_search(
    data: &Array1<f64>,
    the_exp: &Array2<f64>,
    rho_norm_vec: &Array1<f64>,
    n: usize,
) -> (Array1<f64>, usize) {
    let n_f = n as f64;
    let y_sum: f64 = data.sum();
    let t1_len = rho_norm_vec.len();

    let mut rho_ty_vec = Array1::<f64>::zeros(t1_len);
    for j in 0..t1_len {
        let mut dot = 0.0;
        let mut col_sum = 0.0;
        for i in 0..n {
            dot += data[i] * the_exp[[i, j]];
            col_sum += the_exp[[i, j]];
        }
        rho_ty_vec[j] = dot - (1.0 / n_f) * col_sum * y_sum;
    }

    let mut best_ind = 0;
    let mut best_val = f64::NEG_INFINITY;
    for j in 0..t1_len {
        if rho_norm_vec[j] > 0.0 {
            let val = rho_ty_vec[j] * rho_ty_vec[j] / rho_norm_vec[j];
            if val > best_val {
                best_val = val;
                best_ind = j;
            }
        }
    }

    (rho_ty_vec, best_ind)
}

// Arguments are the distinct algorithm quantities: data/time/T1 grids, the
// exponential design matrix, precomputed reductions, best index, sample count.
#[allow(clippy::too_many_arguments)]
fn extract_params(
    data: &Array1<f64>,
    t_vec: &Array1<f64>,
    t1_vec: &Array1<f64>,
    the_exp: &Array2<f64>,
    rho_ty_vec: &Array1<f64>,
    rho_norm_vec: &Array1<f64>,
    ind: usize,
    n: usize,
) -> (f64, f64, f64, f64) {
    let n_f = n as f64;
    let t1 = t1_vec[ind];
    let b = rho_ty_vec[ind] / rho_norm_vec[ind];

    let y_sum: f64 = data.sum();
    let exp_col_sum: f64 = (0..n).map(|i| the_exp[[i, ind]]).sum();
    let a = (1.0 / n_f) * (y_sum - b * exp_col_sum);

    let mut sum_sq = 0.0;
    for i in 0..n {
        let model_val = a + b * (-t_vec[i] / t1).exp();
        if data[i].abs() > 1e-30 {
            let diff = 1.0 - model_val / data[i];
            sum_sq += diff * diff;
        }
    }
    let residual = (1.0 / n_f.sqrt()) * sum_sq.sqrt();

    (t1, b, a, residual)
}

/// Zoom-refine helper: narrow the T1 grid around the best index.
fn zoom_bounds(t1_vec: &Array1<f64>, ind: usize) -> (f64, f64) {
    let len = t1_vec.len();
    if ind > 0 && ind < len - 1 {
        (t1_vec[ind - 1], t1_vec[ind + 1])
    } else if ind == 0 {
        (t1_vec[0], t1_vec[2.min(len - 1)])
    } else {
        (t1_vec[(len - 1).saturating_sub(2)], t1_vec[len - 1])
    }
}

/// Complex data: S(TI) = a + b * exp(-TI / T1). Equivalent to rdNls.m.
fn rd_nls(data: &Array1<f64>, nls: &NlsStruct) -> FitResult {
    assert_eq!(data.len(), nls.n);

    let (mut last_rty, mut ind) = grid_search(data, &nls.the_exp, &nls.rho_norm_vec, nls.n);
    let mut t1_vec = nls.t1_vec.clone();
    let mut last_exp = nls.the_exp.clone();
    let mut last_norm = nls.rho_norm_vec.clone();

    for _ in 1..nls.nbr_of_zoom {
        let (lo, hi) = zoom_bounds(&t1_vec, ind);
        t1_vec = linspace(lo, hi, nls.t1_len_z);
        let (exp_new, norm_new) = compute_exp_and_norm(&nls.t_vec, &t1_vec);
        let (rty, best) = grid_search(data, &exp_new, &norm_new, nls.n);
        ind = best;
        last_exp = exp_new;
        last_rty = rty;
        last_norm = norm_new;
    }

    let (t1, b, a, residual) = extract_params(
        data, &nls.t_vec, &t1_vec, &last_exp, &last_rty, &last_norm, ind, nls.n,
    );
    FitResult {
        t1,
        b,
        a,
        residual,
        idx: None,
    }
}

/// Magnitude data with polarity restoration. Equivalent to rdNlsPr.m.
fn rd_nls_pr(data: &Array1<f64>, nls: &NlsStruct) -> FitResult {
    assert_eq!(data.len(), nls.n);
    let n = nls.n;

    // Sort by TI ascending
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| nls.t_vec[a].partial_cmp(&nls.t_vec[b]).unwrap());

    let sorted_t = Array1::from_iter(order.iter().map(|&i| nls.t_vec[i]));
    let sorted_data = Array1::from_iter(order.iter().map(|&i| data[i].abs()));

    let sorted_exp = {
        let mut exp = Array2::<f64>::zeros((n, nls.the_exp.ncols()));
        for (new_i, &orig_i) in order.iter().enumerate() {
            for j in 0..nls.the_exp.ncols() {
                exp[[new_i, j]] = nls.the_exp[[orig_i, j]];
            }
        }
        exp
    };

    // rho_norm_vec is invariant to row permutation
    let sorted_norm = nls.rho_norm_vec.clone();

    // Find signal minimum
    let (min_ind, _) = sorted_data
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
        .unwrap();

    let mut best = FitResult {
        t1: 0.0,
        b: 0.0,
        a: 0.0,
        residual: f64::INFINITY,
        idx: None,
    };
    let mut best_scenario = 0;

    for scenario in 0..2 {
        // Polarity restoration: negate points before the null crossing
        let mut data_tmp = sorted_data.clone();
        let negate_up_to = if scenario == 0 { min_ind + 1 } else { min_ind };
        for i in 0..negate_up_to {
            data_tmp[i] = -data_tmp[i];
        }

        let (mut last_rty, mut ind) = grid_search(&data_tmp, &sorted_exp, &sorted_norm, n);
        let mut t1_vec = nls.t1_vec.clone();
        let mut last_exp_z = sorted_exp.clone();
        let mut last_norm_z = sorted_norm.clone();

        for k in 1..nls.nbr_of_zoom {
            let (lo, hi) = zoom_bounds(&t1_vec, ind);
            t1_vec = linspace(lo, hi, nls.t1_len_z);
            let (exp_new, norm_new) = compute_exp_and_norm(&sorted_t, &t1_vec);
            let (rty, b) = grid_search(&data_tmp, &exp_new, &norm_new, n);
            ind = b;
            last_exp_z = exp_new;
            last_rty = rty;
            last_norm_z = norm_new;

            // Only extract on last zoom iteration
            if k < nls.nbr_of_zoom - 1 {
                continue;
            }
        }

        let (t1, b, a, residual) = extract_params(
            &data_tmp,
            &sorted_t,
            &t1_vec,
            &last_exp_z,
            &last_rty,
            &last_norm_z,
            ind,
            n,
        );
        if residual < best.residual {
            best = FitResult {
                t1,
                b,
                a,
                residual,
                idx: None,
            };
            best_scenario = scenario;
        }
    }

    best.idx = Some(if best_scenario == 0 {
        min_ind + 1
    } else {
        min_ind
    });
    best
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Inversion times in seconds (BIDS-native).
    fn default_ti() -> Vec<f64> {
        vec![
            0.350, 0.500, 0.650, 0.800, 0.950, 1.100, 1.250, 1.400, 1.700,
        ]
    }

    fn ir_signal(ti: &[f64], t1: f64, a: f64, b: f64) -> Array1<f64> {
        Array1::from_iter(ti.iter().map(|&t| a + b * (-t / t1).exp()))
    }

    #[test]
    fn test_linspace() {
        let v = linspace(1.0, 5.0, 5);
        assert_eq!(v.len(), 5);
        assert!((v[0] - 1.0).abs() < 1e-12);
        assert!((v[4] - 5.0).abs() < 1e-12);
        assert!((v[2] - 3.0).abs() < 1e-12);
    }

    /// Default seconds-native grid: 0.001..5.0 s in 0.001 s steps (5000 pts).
    const T1_START: f64 = 0.001;
    const T1_STOP: f64 = 5.0;
    const T1_STEP: f64 = 0.001;

    #[test]
    fn test_build_nls_struct() {
        let ti = default_ti();
        let nls = build_nls_struct(&ti, T1_START, T1_STOP, T1_STEP, 2, 21);
        assert_eq!(nls.n, 9);
        assert_eq!(nls.t1_vec.len(), 5000);
        assert_eq!(nls.the_exp.dim(), (9, 5000));
        let expected = (-0.350_f64 / 0.500).exp();
        assert!((nls.the_exp[[0, 499]] - expected).abs() < 1e-12);
    }

    #[test]
    fn test_rd_nls_recovers_known_t1() {
        let ti = default_ti();
        let data = ir_signal(&ti, 0.9, 500.0, -1000.0);
        let nls = build_nls_struct(&ti, T1_START, T1_STOP, T1_STEP, 2, 21);
        let r = rd_nls(&data, &nls);

        assert!((r.t1 - 0.9).abs() < 1e-3, "T1: {}", r.t1);
        assert!((r.a - 500.0).abs() < 1.0, "a: {}", r.a);
        assert!((r.b - -1000.0).abs() < 1.0, "b: {}", r.b);
        assert!(r.residual < 1e-6);
        assert!(r.idx.is_none());
    }

    #[test]
    fn test_rd_nls_pr_recovers_known_t1() {
        let ti = default_ti();
        let data = Array1::from_iter(
            ti.iter()
                .map(|&t| (500.0 + -1000.0 * (-t / 0.9).exp()).abs()),
        );
        let nls = build_nls_struct(&ti, T1_START, T1_STOP, T1_STEP, 2, 21);
        let r = rd_nls_pr(&data, &nls);

        assert!((r.t1 - 0.9).abs() < 5e-3, "T1: {}", r.t1);
        assert!(r.idx.is_some());
    }

    #[test]
    fn test_rd_nls_various_t1_values() {
        let ti = default_ti();
        let nls = build_nls_struct(&ti, T1_START, T1_STOP, T1_STEP, 2, 21);

        for &true_t1 in &[0.2, 0.5, 1.0, 2.0, 4.0] {
            let data = ir_signal(&ti, true_t1, 500.0, -1000.0);
            let r = rd_nls(&data, &nls);
            assert!(
                (r.t1 - true_t1).abs() < 1e-3,
                "T1={}: got {}",
                true_t1,
                r.t1
            );
        }
    }

    #[test]
    fn test_rd_nls_no_zoom() {
        let ti = default_ti();
        let nls = build_nls_struct(&ti, T1_START, T1_STOP, T1_STEP, 1, 21);
        let data = ir_signal(&ti, 0.9, 500.0, -1000.0);
        let r = rd_nls(&data, &nls);
        assert!((r.t1 - 0.9).abs() < 2e-3, "T1: {}", r.t1);
    }

    #[test]
    fn forward_then_fit_recovers_params() {
        let cfg = crate::models::inversion_recovery::config::IrConfig {
            inversion_times: default_ti(),
            method: Some(FitMethod::Complex),
            t1_range: Default::default(),
            zoom: Default::default(),
            repetition_time: None,
        };
        let fitter = IrFitter::new(&cfg);
        let sig = fitter.forward(0.9, 500.0, -1000.0);
        assert_eq!(sig.len(), default_ti().len());
        let out = fitter.fit_voxel(&Array1::from_vec(sig));
        assert!((out[0] - 0.9).abs() < 1e-3, "T1: {}", out[0]);
    }
}
