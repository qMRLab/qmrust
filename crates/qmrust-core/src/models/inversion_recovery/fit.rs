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
        self.fit_block(std::slice::from_ref(data))
            .pop()
            .expect("fit_block returns one result per input")
    }

    /// Fit a block of voxels, returning one `output_names()`-ordered result per
    /// input in order. Blocking amortizes the coarse grid sweep over the block;
    /// each voxel's result is identical to fitting it alone.
    pub fn fit_block(&self, data: &[Array1<f64>]) -> Vec<Vec<f64>> {
        let results = match self.method {
            FitMethod::Complex => rd_nls(data, &self.nls),
            FitMethod::Magnitude => rd_nls_pr(data, &self.nls),
        };
        results
            .into_iter()
            .map(|r| {
                let mut v = vec![r.t1, r.b, r.a, r.residual];
                if let Some(idx) = r.idx {
                    v.push(idx as f64);
                }
                v
            })
            .collect()
    }
}

// ─── RD-NLS internals ───────────────────────────────────────────────────────

/// Pre-computed search grid (equivalent to MATLAB's nlsS struct).
///
/// Every field is voxel-independent, so the struct is built once per fit and
/// shared read-only across the parallel engine. `col_sum` holds the per-T1
/// column sums of `the_exp`, pre-multiplied by `1/n`; it is hoisted here because
/// the coarse search would otherwise recompute the same vector for every voxel.
///
/// The `sorted_*` fields are the ascending-TI view used by the magnitude path.
/// `sorted_scaled_col_sum` is summed over the rows of `sorted_exp` rather than
/// reused from `scaled_col_sum`: floating-point addition is order-dependent, so
/// a permuted row order is not guaranteed to give the same bits.
struct NlsStruct {
    t_vec: Array1<f64>,
    n: usize,
    t1_vec: Array1<f64>,
    the_exp: Array2<f64>,
    scaled_col_sum: Array1<f64>,
    rho_norm_vec: Array1<f64>,
    order: Vec<usize>,
    sorted_t: Array1<f64>,
    sorted_exp: Array2<f64>,
    sorted_scaled_col_sum: Array1<f64>,
    nbr_of_zoom: usize,
    t1_len_z: usize,
}

/// A zoom pass's refined grid, superseding the coarse grid for the passes that
/// follow it and for parameter extraction. Held in an `Option` so that a fit
/// configured with `iterations == 1` borrows the coarse grid instead of copying
/// it per voxel.
struct ZoomGrid {
    t1_vec: Array1<f64>,
    the_exp: Array2<f64>,
    rho_norm_vec: Array1<f64>,
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

/// Build `exp(-TI/T1)` over the TI × T1 grid, together with the per-T1 column
/// sums scaled by `1/n` (the form the mean-centering term needs) and the
/// norm-squared of each mean-centered column.
fn compute_exp_and_norm(
    t_vec: &Array1<f64>,
    t1_vec: &Array1<f64>,
) -> (Array2<f64>, Array1<f64>, Array1<f64>) {
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

    let scale = 1.0 / n_f;
    let mut scaled_col_sum = Array1::<f64>::zeros(t1_len);
    let mut rho_norm_vec = Array1::<f64>::zeros(t1_len);
    for j in 0..t1_len {
        let mut sum_sq = 0.0;
        let mut sum_val = 0.0;
        for i in 0..n {
            let v = the_exp[[i, j]];
            sum_sq += v * v;
            sum_val += v;
        }
        scaled_col_sum[j] = scale * sum_val;
        rho_norm_vec[j] = sum_sq - (1.0 / n_f) * sum_val * sum_val;
    }

    (the_exp, scaled_col_sum, rho_norm_vec)
}

/// Per-T1 column sums scaled by `1/n`, summed over rows in index order.
///
/// The `1/n` factor is folded in here because the mean-centering term is
/// evaluated as `(scale * col_sum[j]) * y_sum`; hoisting the left product out
/// of the per-voxel loop preserves the association and therefore the result.
fn scaled_column_sums(the_exp: &Array2<f64>) -> Array1<f64> {
    let (n, t1_len) = the_exp.dim();
    let scale = 1.0 / n as f64;
    let mut scaled_col_sum = Array1::<f64>::zeros(t1_len);
    for j in 0..t1_len {
        let mut sum_val = 0.0;
        for i in 0..n {
            sum_val += the_exp[[i, j]];
        }
        scaled_col_sum[j] = scale * sum_val;
    }
    scaled_col_sum
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
    let (the_exp, scaled_col_sum, rho_norm_vec) = compute_exp_and_norm(&t_vec, &t1_vec);

    // Ascending-TI view for the magnitude path's polarity restoration. The
    // permutation is fixed by the protocol, so it is applied once here.
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| t_vec[a].partial_cmp(&t_vec[b]).unwrap());
    let sorted_t = Array1::from_iter(order.iter().map(|&i| t_vec[i]));
    let mut sorted_exp = Array2::<f64>::zeros((n, the_exp.ncols()));
    for (new_i, &orig_i) in order.iter().enumerate() {
        for j in 0..the_exp.ncols() {
            sorted_exp[[new_i, j]] = the_exp[[orig_i, j]];
        }
    }
    let sorted_scaled_col_sum = scaled_column_sums(&sorted_exp);

    NlsStruct {
        t_vec,
        n,
        t1_vec,
        the_exp,
        scaled_col_sum,
        rho_norm_vec,
        order,
        sorted_t,
        sorted_exp,
        sorted_scaled_col_sum,
        nbr_of_zoom,
        t1_len_z,
    }
}

/// Reduce a filled `rho` row to `(rho[argmax], argmax)` under the maximizing
/// criterion `|rho[j]|² / rhoNormVec[j]`, applying the mean-centering term in
/// the same pass. Only the winning entry is ever read downstream.
fn centre_and_argmax(
    rho: &mut [f64],
    scaled_col_sum: &Array1<f64>,
    rho_norm_vec: &Array1<f64>,
    y_sum: f64,
) -> (f64, usize) {
    for j in 0..rho.len() {
        rho[j] -= scaled_col_sum[j] * y_sum;
    }
    argmax_ratio(rho, rho_norm_vec)
}

/// Reduce a centered `rho` to `(rho[argmax], argmax)` under the maximizing
/// criterion `|rho[j]|² / rhoNormVec[j]`. Ties keep the lowest index, since the
/// update is on strict `>`.
fn argmax_ratio(rho: &[f64], rho_norm_vec: &Array1<f64>) -> (f64, usize) {
    let mut best_ind = 0;
    let mut best_val = f64::NEG_INFINITY;
    for j in 0..rho.len() {
        if rho_norm_vec[j] > 0.0 {
            let val = rho[j] * rho[j] / rho_norm_vec[j];
            if val > best_val {
                best_val = val;
                best_ind = j;
            }
        }
    }
    (rho[best_ind], best_ind)
}

/// Grid search: argmax_j |rhoTyVec[j]|² / rhoNormVec[j], returning the winning
/// `rhoTyVec` entry and its index.
///
/// `scaled_col_sum` must be the `1/n`-scaled column sums of `the_exp`, summed in
/// row-index order (see [`scaled_column_sums`]).
fn grid_search(
    data: &Array1<f64>,
    the_exp: &Array2<f64>,
    scaled_col_sum: &Array1<f64>,
    rho_norm_vec: &Array1<f64>,
    n: usize,
) -> (f64, usize) {
    let y_sum: f64 = data.sum();
    let mut rho = vec![0.0f64; rho_norm_vec.len()];
    accumulate_rho(&mut rho, data, the_exp, n);
    centre_and_argmax(&mut rho, scaled_col_sum, rho_norm_vec, y_sum)
}

/// Accumulate `rho[j] += Σᵢ data[i]·the_exp[i][j]`.
///
/// The loop nest is (i, j) rather than (j, i) so the inner loop walks a row of
/// `the_exp` contiguously and vectorizes. Each `rho[j]` still accumulates `i` in
/// ascending order, so every partial sum — and hence the result — is
/// bit-identical to the scalar (j, i) form.
fn accumulate_rho(rho: &mut [f64], data: &Array1<f64>, the_exp: &Array2<f64>, n: usize) {
    for i in 0..n {
        let d = data[i];
        let row = the_exp.row(i);
        let row = row
            .as_slice()
            .expect("the_exp is row-major, so its rows are contiguous");
        for (r, &e) in rho.iter_mut().zip(row) {
            *r += d * e;
        }
    }
}

/// Coarse grid search over a block of voxels sharing one grid.
///
/// Results are identical to calling [`grid_search`] on each entry: per voxel the
/// accumulation over `i` is unchanged. Blocking exists so each row of `the_exp`
/// is read once per tile and reused across the tile's voxels out of L1, rather
/// than being re-streamed from L2 for every voxel. The `rho` scratch is
/// allocated once and reused across tiles.
fn grid_search_block(
    data: &[Array1<f64>],
    the_exp: &Array2<f64>,
    scaled_col_sum: &Array1<f64>,
    rho_norm_vec: &Array1<f64>,
    n: usize,
) -> Vec<(f64, usize)> {
    /// Voxels per tile. One `the_exp` row (t1_len × 8 bytes) must stay resident
    /// across a tile's inner loops for the reuse to pay off.
    const LANES: usize = 8;

    assert!(n >= 1, "a grid search needs at least one sample");
    let t1_len = rho_norm_vec.len();
    let mut out = Vec::with_capacity(data.len());
    let mut scratch = vec![0.0f64; LANES * t1_len];
    let last = n - 1;

    for tile in data.chunks(LANES) {
        let lanes = tile.len();
        scratch[..lanes * t1_len].fill(0.0);

        for i in 0..last {
            let row = the_exp.row(i);
            let row = row
                .as_slice()
                .expect("the_exp is row-major, so its rows are contiguous");
            for (k, voxel) in tile.iter().enumerate() {
                let d = voxel[i];
                let rho = &mut scratch[k * t1_len..(k + 1) * t1_len];
                for (r, &e) in rho.iter_mut().zip(row) {
                    *r += d * e;
                }
            }
        }

        // The final row's accumulation is fused with the mean-centering, so
        // `rho` is written once rather than read-modify-written again in a
        // separate centering pass. Both operations are branch-free, so the loop
        // still vectorizes; the reduction stays separate because its
        // data-dependent branch would otherwise inhibit that. Per `j` the
        // operation sequence is unchanged — accumulate, then centre — so the
        // values entering the reduction are bit-identical.
        let row = the_exp.row(last);
        let row = row
            .as_slice()
            .expect("the_exp is row-major, so its rows are contiguous");
        for (k, voxel) in tile.iter().enumerate() {
            let d = voxel[last];
            let y_sum: f64 = voxel.sum();
            let rho = &mut scratch[k * t1_len..(k + 1) * t1_len];
            for j in 0..t1_len {
                rho[j] = rho[j] + d * row[j] - scaled_col_sum[j] * y_sum;
            }
            out.push(argmax_ratio(rho, rho_norm_vec));
        }
    }
    out
}

// Compute the fit parameters at the chosen T1-grid index. Returns
// `(T1, b, a, residual)`: the T1 estimate, the exponential amplitude `b`, the
// offset `a`, and the normalized RMS residual. `rho_ty` is the winning
// `rhoTyVec` entry from the search that produced `ind`. Preconditions: `ind` is
// a valid index into `t1_vec`/`rho_norm_vec`; `n` is within the sample range of
// `data`/`t_vec` and of `the_exp`'s rows; and `rho_norm_vec[ind]` is nonzero,
// since it divides `rho_ty`.
#[allow(clippy::too_many_arguments)]
fn extract_params(
    data: &Array1<f64>,
    t_vec: &Array1<f64>,
    t1_vec: &Array1<f64>,
    the_exp: &Array2<f64>,
    rho_ty: f64,
    rho_norm_vec: &Array1<f64>,
    ind: usize,
    n: usize,
) -> (f64, f64, f64, f64) {
    let n_f = n as f64;
    let t1 = t1_vec[ind];
    let b = rho_ty / rho_norm_vec[ind];

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

/// Run the zoom passes from a coarse-grid winner, then extract the parameters.
///
/// `t_vec` and `coarse_exp` select the TI ordering: the complex path works in
/// protocol order, the magnitude path in ascending-TI order. `coarse_exp` is
/// read only when no zoom pass runs (`iterations == 1`), which is why the
/// coarse grid is borrowed rather than copied.
fn refine_and_extract(
    data: &Array1<f64>,
    nls: &NlsStruct,
    t_vec: &Array1<f64>,
    coarse_exp: &Array2<f64>,
    coarse: (f64, usize),
) -> (f64, f64, f64, f64) {
    let (mut rho_ty, mut ind) = coarse;
    let mut zoomed: Option<ZoomGrid> = None;

    for _ in 1..nls.nbr_of_zoom {
        let t1_vec = {
            let prev = zoomed.as_ref().map_or(&nls.t1_vec, |z| &z.t1_vec);
            let (lo, hi) = zoom_bounds(prev, ind);
            linspace(lo, hi, nls.t1_len_z)
        };
        let (the_exp, scaled_col_sum, rho_norm_vec) = compute_exp_and_norm(t_vec, &t1_vec);
        (rho_ty, ind) = grid_search(data, &the_exp, &scaled_col_sum, &rho_norm_vec, nls.n);
        zoomed = Some(ZoomGrid {
            t1_vec,
            the_exp,
            rho_norm_vec,
        });
    }

    let (t1_vec, the_exp, rho_norm_vec) = match &zoomed {
        Some(z) => (&z.t1_vec, &z.the_exp, &z.rho_norm_vec),
        None => (&nls.t1_vec, coarse_exp, &nls.rho_norm_vec),
    };
    extract_params(
        data,
        t_vec,
        t1_vec,
        the_exp,
        rho_ty,
        rho_norm_vec,
        ind,
        nls.n,
    )
}

/// Complex data: S(TI) = a + b * exp(-TI / T1). Equivalent to rdNls.m.
fn rd_nls(data: &[Array1<f64>], nls: &NlsStruct) -> Vec<FitResult> {
    debug_assert!(data.iter().all(|d| d.len() == nls.n));

    grid_search_block(
        data,
        &nls.the_exp,
        &nls.scaled_col_sum,
        &nls.rho_norm_vec,
        nls.n,
    )
    .into_iter()
    .zip(data)
    .map(|(coarse, d)| {
        let (t1, b, a, residual) = refine_and_extract(d, nls, &nls.t_vec, &nls.the_exp, coarse);
        FitResult {
            t1,
            b,
            a,
            residual,
            idx: None,
        }
    })
    .collect()
}

/// Magnitude data with polarity restoration. Equivalent to rdNlsPr.m.
fn rd_nls_pr(data: &[Array1<f64>], nls: &NlsStruct) -> Vec<FitResult> {
    debug_assert!(data.iter().all(|d| d.len() == nls.n));

    // The ascending-TI grid is pre-permuted in `nls`; only the data needs
    // reordering per voxel. `rho_norm_vec` is invariant to row permutation.
    let sorted: Vec<Array1<f64>> = data
        .iter()
        .map(|d| Array1::from_iter(nls.order.iter().map(|&i| d[i].abs())))
        .collect();

    // Signal minimum per voxel — the null crossing is adjacent to it.
    let min_inds: Vec<usize> = sorted
        .iter()
        .map(|s| {
            s.iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
                .unwrap()
                .0
        })
        .collect();

    let mut best: Vec<FitResult> = (0..data.len())
        .map(|_| FitResult {
            t1: 0.0,
            b: 0.0,
            a: 0.0,
            residual: f64::INFINITY,
            idx: None,
        })
        .collect();
    let mut best_scenario = vec![0usize; data.len()];

    for scenario in 0..2 {
        // Polarity restoration: negate points before the null crossing
        let restored: Vec<Array1<f64>> = sorted
            .iter()
            .zip(&min_inds)
            .map(|(s, &min_ind)| {
                let mut d = s.clone();
                let negate_up_to = if scenario == 0 { min_ind + 1 } else { min_ind };
                for i in 0..negate_up_to {
                    d[i] = -d[i];
                }
                d
            })
            .collect();

        let coarse = grid_search_block(
            &restored,
            &nls.sorted_exp,
            &nls.sorted_scaled_col_sum,
            &nls.rho_norm_vec,
            nls.n,
        );

        for (v, (c, d)) in coarse.into_iter().zip(&restored).enumerate() {
            let (t1, b, a, residual) =
                refine_and_extract(d, nls, &nls.sorted_t, &nls.sorted_exp, c);
            if residual < best[v].residual {
                best[v] = FitResult {
                    t1,
                    b,
                    a,
                    residual,
                    idx: None,
                };
                best_scenario[v] = scenario;
            }
        }
    }

    for (v, r) in best.iter_mut().enumerate() {
        r.idx = Some(if best_scenario[v] == 0 {
            min_inds[v] + 1
        } else {
            min_inds[v]
        });
    }
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

    /// Fit a lone voxel through a block entry point.
    fn fit_one(
        f: fn(&[Array1<f64>], &NlsStruct) -> Vec<FitResult>,
        data: &Array1<f64>,
        nls: &NlsStruct,
    ) -> FitResult {
        f(std::slice::from_ref(data), nls)
            .pop()
            .expect("one result per input")
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
        let r = fit_one(rd_nls, &data, &nls);

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
        let r = fit_one(rd_nls_pr, &data, &nls);

        assert!((r.t1 - 0.9).abs() < 5e-3, "T1: {}", r.t1);
        assert!(r.idx.is_some());
    }

    #[test]
    fn test_rd_nls_various_t1_values() {
        let ti = default_ti();
        let nls = build_nls_struct(&ti, T1_START, T1_STOP, T1_STEP, 2, 21);

        for &true_t1 in &[0.2, 0.5, 1.0, 2.0, 4.0] {
            let data = ir_signal(&ti, true_t1, 500.0, -1000.0);
            let r = fit_one(rd_nls, &data, &nls);
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
        let r = fit_one(rd_nls, &data, &nls);
        assert!((r.t1 - 0.9).abs() < 2e-3, "T1: {}", r.t1);
    }

    /// Blocking must be invisible: a voxel's result may not depend on which
    /// block it landed in, or on where in that block it sat. The sizes span
    /// partial, exact and multiple tiles so tile boundaries are exercised.
    #[test]
    fn fit_block_matches_fit_voxel() {
        for method in [FitMethod::Complex, FitMethod::Magnitude] {
            let cfg = crate::models::inversion_recovery::config::IrConfig {
                inversion_times: default_ti(),
                method: Some(method.clone()),
                t1_range: Default::default(),
                zoom: Default::default(),
                repetition_time: None,
            };
            let fitter = IrFitter::new(&cfg);
            let ti = default_ti();
            // Distinct T1/a/b per voxel so no two share a search path.
            let voxels: Vec<Array1<f64>> = (0..37)
                .map(|k| {
                    let t1 = 0.2 + 0.1 * k as f64;
                    let sig = ir_signal(&ti, t1, 400.0 + 7.0 * k as f64, -1000.0 - 3.0 * k as f64);
                    match method {
                        FitMethod::Magnitude => sig.mapv(f64::abs),
                        FitMethod::Complex => sig,
                    }
                })
                .collect();

            let alone: Vec<Vec<f64>> = voxels.iter().map(|v| fitter.fit_voxel(v)).collect();
            for size in [1, 3, 8, 9, 16, 37] {
                let blocked: Vec<Vec<f64>> = voxels
                    .chunks(size)
                    .flat_map(|c| fitter.fit_block(c))
                    .collect();
                assert_eq!(blocked, alone, "method {method:?}, block size {size}");
            }
        }
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
