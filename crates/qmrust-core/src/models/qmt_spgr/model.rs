//! Ramani analytical SPGR-MT signal (SPGR_R_fun.m), computeR1.m, and
//! per-voxel MToff normalization (qmt_spgr.m fit()).

use super::lineshape::compute_wb;
use super::pulse::GaussHannPulse;
use super::sf::SfTable;
use std::f64::consts::PI;

/// Fitting options that influence the signal equation.
pub struct FitOpt {
    /// Whether R1map is used to constrain R1f.
    pub r1map: bool,
    /// Observed R1 for this voxel (Some when r1map is on).
    pub r1obs: Option<f64>,
    /// Fix R1r = R1f.
    pub r1req_r1f: bool,
    /// Fix R1f*T2f to a constant.
    pub fix_r1f_t2f: bool,
    /// The R1f*T2f value used when fix_r1f_t2f is true.
    pub fix_r1f_t2f_value: f64,
}

/// computeR1.m: free-pool R1 from observed R1.
pub fn compute_r1(f: f64, kf: f64, r1r: f64, r1obs: f64) -> f64 {
    r1obs - kf * (r1r - r1obs) / (r1r - r1obs + kf / f)
}

/// Ramani model signal for each protocol row. `x = [F,kr,R1f,R1r,T2f,T2r]`.
pub fn ramani_signal(x: &[f64; 6], offsets: &[f64], w1cw: &[f64], opt: &FitOpt) -> Vec<f64> {
    let f = x[0];
    let kr = x[1];
    let mut r1f = x[2];
    let mut r1r = x[3];
    let t2f = x[4];
    let t2r = x[5];
    let kf = kr * f;

    if opt.r1req_r1f {
        r1r = x[2];
    }
    if opt.r1map {
        if let Some(r1obs) = opt.r1obs {
            r1f = compute_r1(f, kf, r1r, r1obs);
        }
    }

    offsets
        .iter()
        .zip(w1cw.iter())
        .map(|(&delta, &w1)| {
            let wb = compute_wb(w1, delta, t2r);
            let wf = if opt.fix_r1f_t2f {
                (w1 / (2.0 * PI * delta)).powi(2) / opt.fix_r1f_t2f_value
            } else {
                (w1 / (2.0 * PI * delta)).powi(2) / (r1f * t2f)
            };
            let kfr = kf / r1f; // kr*F/R1f == kf/R1f
            let num = r1r * kfr + wb + r1r + kr;
            let den = kfr * (r1r + wb) + (1.0 + wf) * (wb + r1r + kr);
            num / den
        })
        .collect()
}

/// Divide `voxel` by the median of its MToff entries and return only the
/// entries to be fitted. If `mtoff_idx` is empty, returns the fit entries
/// unchanged (data assumed already normalized).
pub fn normalize_mtoff(voxel: &[f64], mtoff_idx: &[usize], fit_idx: &[usize]) -> Vec<f64> {
    if mtoff_idx.is_empty() {
        return fit_idx.iter().map(|&i| voxel[i]).collect();
    }
    let mut offv: Vec<f64> = mtoff_idx.iter().map(|&i| voxel[i]).collect();
    offv.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = offv.len();
    let median = if n % 2 == 1 {
        offv[n / 2]
    } else {
        0.5 * (offv[n / 2 - 1] + offv[n / 2])
    };
    fit_idx.iter().map(|&i| voxel[i] / median).collect()
}

/// Closed-form matrix exponential of a real 2×2 matrix.
/// expm(M) = e^s (c0 I + c1 (M - s I)) with s=tr/2, disc=s²-det.
pub fn mat2x2_exp(m: [[f64; 2]; 2]) -> [[f64; 2]; 2] {
    let (a, b, c, d) = (m[0][0], m[0][1], m[1][0], m[1][1]);
    let s = 0.5 * (a + d);
    let det = a * d - b * c;
    let disc = s * s - det;
    let (c0, c1) = if disc > 1e-14 {
        let q = disc.sqrt();
        (q.cosh(), q.sinh() / q)
    } else if disc < -1e-14 {
        let q = (-disc).sqrt();
        (q.cos(), q.sin() / q)
    } else {
        (1.0, 1.0)
    };
    let es = s.exp();
    [
        [es * (c0 + c1 * (a - s)), es * (c1 * b)],
        [es * (c1 * c), es * (c0 + c1 * (d - s))],
    ]
}

// ─── 2×2 linear algebra helpers for the Sled-Pike steady state ───────────────
type M2 = [[f64; 2]; 2];
type V2 = [f64; 2];
const I2: M2 = [[1.0, 0.0], [0.0, 1.0]];

fn mm(a: M2, b: M2) -> M2 {
    [
        [
            a[0][0] * b[0][0] + a[0][1] * b[1][0],
            a[0][0] * b[0][1] + a[0][1] * b[1][1],
        ],
        [
            a[1][0] * b[0][0] + a[1][1] * b[1][0],
            a[1][0] * b[0][1] + a[1][1] * b[1][1],
        ],
    ]
}
fn mv(a: M2, v: V2) -> V2 {
    [
        a[0][0] * v[0] + a[0][1] * v[1],
        a[1][0] * v[0] + a[1][1] * v[1],
    ]
}
fn madd(a: M2, b: M2) -> M2 {
    [
        [a[0][0] + b[0][0], a[0][1] + b[0][1]],
        [a[1][0] + b[1][0], a[1][1] + b[1][1]],
    ]
}
fn msub(a: M2, b: M2) -> M2 {
    [
        [a[0][0] - b[0][0], a[0][1] - b[0][1]],
        [a[1][0] - b[1][0], a[1][1] - b[1][1]],
    ]
}
fn mscale(a: M2, s: f64) -> M2 {
    [[a[0][0] * s, a[0][1] * s], [a[1][0] * s, a[1][1] * s]]
}
/// Solve A x = b for 2×2 A via Cramer's rule.
fn solve2(a: M2, b: V2) -> V2 {
    let det = a[0][0] * a[1][1] - a[0][1] * a[1][0];
    [
        (b[0] * a[1][1] - a[0][1] * b[1]) / det,
        (a[0][0] * b[1] - b[0] * a[1][0]) / det,
    ]
}

/// calcMxy from SPGR_Srp_fun.m: transverse signal for one (Sf, W) pair.
#[allow(clippy::too_many_arguments)]
fn calc_mxy(
    f: f64,
    m0f: f64,
    m0r: f64,
    kf: f64,
    kr: f64,
    r1f: f64,
    r1r: f64,
    sf: f64,
    sr: f64,
    w: f64,
    tr: f64,
    tau: f64,
    alpha: f64,
) -> f64 {
    let a12: M2 = [[r1f + kf, -kr], [-kf, r1r + kr + w]];
    let a0: M2 = [[r1f + kf, -kr], [-kf, r1r + kr]];
    let ea12 = mat2x2_exp(mscale(a12, -tau / 2.0));
    let ea0 = mat2x2_exp(mscale(a0, -(tr - tau)));

    let (mzf_inf, mzr_inf) = if f == 0.0 {
        (m0f, 0.0)
    } else {
        let n1 = m0f * (r1r * kf + r1r * r1f + r1f * kr + w * r1f);
        let n2 = m0r * (r1r * kf + r1r * r1f + r1f * kr);
        let den = r1r * kf + r1r * r1f + r1f * kr + w * r1f + w * kf;
        (n1 / den, n2 / den)
    };
    let mss: V2 = [mzf_inf, mzr_inf];
    let m0_inf: V2 = [m0f, m0r];
    let diag: M2 = [[sf * alpha.cos(), 0.0], [0.0, sr]];

    // LHS = I - eA12*eA0*eA12*diag
    let lhs = msub(I2, mm(mm(mm(ea12, ea0), ea12), diag));
    // RHS = (I + eA12*(-I + eA0*(I - eA12)))*Mss + eA12*(I - eA0)*M0_inf
    let inner = madd(mscale(I2, -1.0), mm(ea0, msub(I2, ea12)));
    let term1 = madd(I2, mm(ea12, inner));
    let rhs_vec = [
        mv(term1, mss)[0] + mv(mm(ea12, msub(I2, ea0)), m0_inf)[0],
        mv(term1, mss)[1] + mv(mm(ea12, msub(I2, ea0)), m0_inf)[1],
    ];
    let mz = solve2(lhs, rhs_vec);
    mz[0] * alpha.sin() * sf
}

/// SledPikeRP analytical signal (SPGR_Srp_fun.m). Returns normalized mz per volume.
#[allow(clippy::too_many_arguments)]
pub fn srp_signal(
    x: &[f64; 6],
    angles: &[f64],
    offsets: &[f64],
    w1rp: &[f64],
    tau: f64,
    tr: f64,
    alpha_deg: f64,
    sf: &SfTable,
    pulse: &GaussHannPulse,
    opt: &FitOpt,
) -> Vec<f64> {
    let f = x[0];
    let kr = x[1];
    let mut r1f = x[2];
    let mut r1r = x[3];
    let t2f = x[4];
    let t2r = x[5];
    let kf = kr * f;
    let m0f = 1.0;
    let m0r = f * m0f;

    if t2f <= 0.0 {
        return vec![f64::NAN; angles.len()];
    }
    if opt.r1req_r1f {
        r1r = x[2];
    }
    if opt.r1map {
        if let Some(r1obs) = opt.r1obs {
            r1f = compute_r1(f, kf, r1r, r1obs);
        }
    }
    let alpha = alpha_deg * std::f64::consts::PI / 180.0;

    let mxy0 = calc_mxy(f, m0f, m0r, kf, kr, r1f, r1r, 1.0, 1.0, 0.0, tr, tau, alpha);
    angles
        .iter()
        .zip(offsets.iter())
        .zip(w1rp.iter())
        .map(|((&angle, &delta), &w1)| {
            let sf_i = sf.get(angle, delta, t2f, pulse);
            let w = compute_wb(w1, delta, t2r);
            let mxy = calc_mxy(f, m0f, m0r, kf, kr, r1f, r1r, sf_i, 1.0, w, tr, tau, alpha);
            mxy / mxy0
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opt_none() -> FitOpt {
        FitOpt {
            r1map: false,
            r1obs: None,
            r1req_r1f: false,
            fix_r1f_t2f: false,
            fix_r1f_t2f_value: 0.055,
        }
    }

    fn opt_with(
        r1map: bool,
        r1obs: Option<f64>,
        r1req_r1f: bool,
        fix_r1f_t2f: bool,
        fix_r1f_t2f_value: f64,
    ) -> FitOpt {
        FitOpt {
            r1map,
            r1obs,
            r1req_r1f,
            fix_r1f_t2f,
            fix_r1f_t2f_value,
        }
    }

    #[test]
    fn compute_r1_algebra() {
        // R1f = R1obs - kf*(R1r-R1obs)/(R1r-R1obs+kf/F)
        let got = compute_r1(0.16, 4.8, 1.0, 1.0);
        // R1r-R1obs = 0 -> R1f = R1obs
        assert!((got - 1.0).abs() < 1e-12, "got {}", got);
    }

    #[test]
    fn signal_bounded_and_positive() {
        let x = [0.16, 30.0, 1.0, 1.0, 0.03, 1.3e-5];
        let offsets = [443.0, 2732.0, 17235.0];
        let w1cw = [200.0, 200.0, 200.0];
        let mz = ramani_signal(&x, &offsets, &w1cw, &opt_none());
        for m in &mz {
            assert!(*m > 0.0 && *m <= 1.0 + 1e-9, "mz out of range: {}", m);
        }
    }

    #[test]
    fn signal_approaches_one_at_large_offset() {
        let x = [0.16, 30.0, 1.0, 1.0, 0.03, 1.3e-5];
        let small = ramani_signal(&x, &[500.0], &[200.0], &opt_none())[0];
        let large = ramani_signal(&x, &[1.0e6], &[200.0], &opt_none())[0];
        assert!(
            large > small,
            "far off-res should saturate less: {} !> {}",
            large,
            small
        );
        assert!(
            large > 0.99,
            "far off-res mz should approach 1, got {}",
            large
        );
    }

    #[test]
    fn r1req_r1f_ignores_x3() {
        // x[3] (R1r) differs between the two vectors; x[2] (R1f) is the same.
        let x_a = [0.16, 30.0, 1.0, 1.0, 0.03, 1.3e-5];
        let x_b = [0.16, 30.0, 1.0, 5.0, 0.03, 1.3e-5];
        let offsets = [443.0, 2732.0];
        let w1cw = [200.0, 200.0];

        // With r1req_r1f = true, R1r is overwritten by x[2] for both, so the
        // differing x[3] must have no effect: results must match exactly.
        let opt_fixed = opt_with(false, None, true, false, 0.055);
        let mz_a_fixed = ramani_signal(&x_a, &offsets, &w1cw, &opt_fixed);
        let mz_b_fixed = ramani_signal(&x_b, &offsets, &w1cw, &opt_fixed);
        for (a, b) in mz_a_fixed.iter().zip(mz_b_fixed.iter()) {
            assert!(
                (a - b).abs() < 1e-12,
                "expected identical mz with r1req_r1f, got {} vs {}",
                a,
                b
            );
        }

        // With opt_none (r1req_r1f = false), x[3] is used directly, so the
        // two parameter vectors must produce different results.
        let mz_a_none = ramani_signal(&x_a, &offsets, &w1cw, &opt_none());
        let mz_b_none = ramani_signal(&x_b, &offsets, &w1cw, &opt_none());
        let mut any_diff = false;
        for (a, b) in mz_a_none.iter().zip(mz_b_none.iter()) {
            if (a - b).abs() > 1e-9 {
                any_diff = true;
            }
        }
        assert!(
            any_diff,
            "expected differing mz without r1req_r1f, got {:?} vs {:?}",
            mz_a_none, mz_b_none
        );
    }

    #[test]
    fn r1map_recomputes_r1f() {
        let x = [0.16, 30.0, 1.0, 1.0, 0.03, 1.3e-5];
        let offsets = [443.0, 2732.0, 17235.0];
        let w1cw = [200.0, 200.0, 200.0];

        let opt_map = opt_with(true, Some(0.8), false, false, 0.055);
        let mz_map = ramani_signal(&x, &offsets, &w1cw, &opt_map);
        let mz_none = ramani_signal(&x, &offsets, &w1cw, &opt_none());

        for m in &mz_map {
            assert!(m.is_finite() && *m > 0.0, "mz not finite/positive: {}", m);
        }
        let mut any_diff = false;
        for (a, b) in mz_map.iter().zip(mz_none.iter()) {
            if (a - b).abs() > 1e-9 {
                any_diff = true;
            }
        }
        assert!(
            any_diff,
            "expected r1map to change mz, got {:?} vs {:?}",
            mz_map, mz_none
        );
    }

    #[test]
    fn fix_r1f_t2f_changes_wf() {
        let x = [0.16, 30.0, 1.0, 1.0, 0.03, 1.3e-5];
        let offsets = [443.0, 2732.0, 17235.0];
        let w1cw = [200.0, 200.0, 200.0];

        let opt_fixed = opt_with(false, None, false, true, 0.055);
        let mz_fixed = ramani_signal(&x, &offsets, &w1cw, &opt_fixed);
        let mz_none = ramani_signal(&x, &offsets, &w1cw, &opt_none());

        for m in &mz_fixed {
            assert!(m.is_finite() && *m > 0.0, "mz not finite/positive: {}", m);
        }
        let mut any_diff = false;
        for (a, b) in mz_fixed.iter().zip(mz_none.iter()) {
            if (a - b).abs() > 1e-9 {
                any_diff = true;
            }
        }
        assert!(
            any_diff,
            "expected fix_r1f_t2f to change mz, got {:?} vs {:?}",
            mz_fixed, mz_none
        );
    }

    #[test]
    fn normalize_with_no_mtoff_returns_fit_entries() {
        let v = vec![0.9, 0.8, 0.7];
        let out = normalize_mtoff(&v, &[], &[0, 1, 2]);
        assert_eq!(out, vec![0.9, 0.8, 0.7]);
    }

    #[test]
    fn normalize_divides_by_median_of_mtoff() {
        // MToff entries at idx 0,1 (values 2.0, 4.0 -> median 3.0), fit idx 2,3
        let v = vec![2.0, 4.0, 6.0, 9.0];
        let out = normalize_mtoff(&v, &[0, 1], &[2, 3]);
        assert!((out[0] - 2.0).abs() < 1e-12);
        assert!((out[1] - 3.0).abs() < 1e-12);
    }

    #[test]
    fn exp_of_zero_is_identity() {
        let e = mat2x2_exp([[0.0, 0.0], [0.0, 0.0]]);
        assert!((e[0][0] - 1.0).abs() < 1e-12 && (e[1][1] - 1.0).abs() < 1e-12);
        assert!(e[0][1].abs() < 1e-12 && e[1][0].abs() < 1e-12);
    }

    #[test]
    fn exp_of_diagonal() {
        let e = mat2x2_exp([[0.5, 0.0], [0.0, -0.3]]);
        assert!((e[0][0] - 0.5_f64.exp()).abs() < 1e-10, "{}", e[0][0]);
        assert!((e[1][1] - (-0.3_f64).exp()).abs() < 1e-10, "{}", e[1][1]);
        assert!(e[0][1].abs() < 1e-12 && e[1][0].abs() < 1e-12);
    }

    #[test]
    fn exp_of_known_matrix() {
        // M=[[0,1],[-1,0]] → expm = [[cos1, sin1],[-sin1, cos1]]
        let e = mat2x2_exp([[0.0, 1.0], [-1.0, 0.0]]);
        assert!((e[0][0] - 1.0_f64.cos()).abs() < 1e-10);
        assert!((e[0][1] - 1.0_f64.sin()).abs() < 1e-10);
        assert!((e[1][0] + 1.0_f64.sin()).abs() < 1e-10);
        assert!((e[1][1] - 1.0_f64.cos()).abs() < 1e-10);
    }

    #[test]
    fn srp_signal_bounded_and_saturates() {
        use crate::models::qmt_spgr::pulse::GaussHannPulse;
        use crate::models::qmt_spgr::sf::{build_sf_axes, build_sf_table};
        let p = GaussHannPulse::new(0.0102, 200.0);
        let angles_p = [142.0, 426.0];
        let offsets_p = [443.0, 2732.0, 17235.0];
        let (sa, so, st) = build_sf_axes(&angles_p, &offsets_p);
        let table = build_sf_table(&p, &sa, &so, &st);

        let x = [0.16, 30.0, 1.0, 1.0, 0.03, 1.3e-5];
        let angles = [142.0, 426.0, 142.0];
        let offsets = [443.0, 2732.0, 17235.0];
        let w1rp = [
            p.w1rp_and_tau(142.0).0,
            p.w1rp_and_tau(426.0).0,
            p.w1rp_and_tau(142.0).0,
        ];
        let tau = p.tau();
        let opt = opt_none();
        let mz = srp_signal(
            &x, &angles, &offsets, &w1rp, tau, 0.025, 7.0, &table, &p, &opt,
        );
        assert_eq!(mz.len(), 3);
        for m in &mz {
            assert!(*m > 0.0 && *m <= 1.0 + 1e-6, "mz out of range: {}", m);
        }
        // higher offset (index 2, 17235) less saturated than a low offset
        assert!(
            mz[2] > mz[0],
            "far off-res less saturated: {} !> {}",
            mz[2],
            mz[0]
        );
    }

    #[test]
    fn srp_signal_f_zero_no_nan() {
        use crate::models::qmt_spgr::pulse::GaussHannPulse;
        use crate::models::qmt_spgr::sf::{build_sf_axes, build_sf_table};
        let p = GaussHannPulse::new(0.0102, 200.0);
        let (sa, so, st) = build_sf_axes(&[142.0, 426.0], &[443.0, 2732.0]);
        let table = build_sf_table(&p, &sa, &so, &st);
        let x = [0.0, 30.0, 1.0, 1.0, 0.03, 1.3e-5]; // F=0
        let mz = srp_signal(
            &x,
            &[142.0],
            &[443.0],
            &[p.w1rp_and_tau(142.0).0],
            p.tau(),
            0.025,
            7.0,
            &table,
            &p,
            &opt_none(),
        );
        assert!(mz[0].is_finite(), "F=0 must not NaN: {}", mz[0]);
    }

    #[test]
    fn srp_signal_depends_on_alpha() {
        // Guards the B1-scaling fix in fit_voxel: srp_signal's readout `alpha`
        // must actually affect the output, so passing an unscaled alpha would
        // be caught by a fidelity check against MATLAB.
        use crate::models::qmt_spgr::pulse::GaussHannPulse;
        use crate::models::qmt_spgr::sf::{build_sf_axes, build_sf_table};
        let p = GaussHannPulse::new(0.0102, 200.0);
        let angles_p = [142.0, 426.0];
        let offsets_p = [443.0, 2732.0, 17235.0];
        let (sa, so, st) = build_sf_axes(&angles_p, &offsets_p);
        let table = build_sf_table(&p, &sa, &so, &st);

        let x = [0.16, 30.0, 1.0, 1.0, 0.03, 1.3e-5];
        let angles = [142.0, 426.0, 142.0];
        let offsets = [443.0, 2732.0, 17235.0];
        let w1rp = [
            p.w1rp_and_tau(142.0).0,
            p.w1rp_and_tau(426.0).0,
            p.w1rp_and_tau(142.0).0,
        ];
        let tau = p.tau();
        let opt = opt_none();

        let mz_7 = srp_signal(
            &x, &angles, &offsets, &w1rp, tau, 0.025, 7.0, &table, &p, &opt,
        );
        let mz_14 = srp_signal(
            &x, &angles, &offsets, &w1rp, tau, 0.025, 14.0, &table, &p, &opt,
        );

        let max_diff = mz_7
            .iter()
            .zip(mz_14.iter())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0_f64, f64::max);
        assert!(
            max_diff > 1e-6,
            "srp_signal insensitive to alpha: max_diff={}",
            max_diff
        );
    }
}
