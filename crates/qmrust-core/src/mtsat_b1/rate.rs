//! 5-state Bloch–McConnell rate matrix, ported from
//! <ref>/functions/Bloch_McConnell_wDipolar.m (water-excitation branch).
//! State `M = [Wx, Wy, Wz, Bz, D]`: free-water transverse x/y, free-water
//! longitudinal, bound-pool longitudinal, dipolar order. Bound-pool absorption
//! uses the in-tree 2π super-Lorentzian lineshape (`super_lorentzian_g`):
//! `Rrfb = π·w1²·G(δ, T2b)`.

use crate::models::qmt_spgr::lineshape::super_lorentzian_g;
use crate::mtsat_b1::mat5::Mat5;
use std::f64::consts::PI;

pub struct PoolParams {
    pub ra: f64,
    pub r1b: f64,
    pub r: f64,
    pub m0a: f64,
    pub m0b: f64,
    pub t2a: f64,
    pub t2b: f64,
    pub t1d: f64,
}

/// Rate matrix `A` for `dM/dt = A·M`, state `[Wx, Wy, Wz, Bz, D]`. `w1` in
/// rad/s, `delta` in Hz (negative `delta` selects the dualAlternate even
/// pulse). The bound-pool absorption uses the in-tree 2π super-Lorentzian
/// lineshape (`super_lorentzian_g`).
pub fn rate_matrix(p: &PoolParams, w1: f64, delta: f64) -> Mat5 {
    let r2a = 1.0 / p.t2a;
    let g = super_lorentzian_g(delta.abs(), p.t2b);
    let rrfb = PI * w1 * w1 * g;
    let wloc = (1.0 / (15.0 * p.t2b * p.t2b)).sqrt();
    let omega = 2.0 * PI * delta / wloc;
    let kf = p.r * p.m0b;
    let kr = p.r * p.m0a;
    let tpd = 2.0 * PI * delta;
    [
        [-r2a, -tpd, 0.0, 0.0, 0.0],
        [tpd, -r2a, -w1, 0.0, 0.0],
        [0.0, w1, -(p.ra + kf), kr, 0.0],
        [0.0, 0.0, kf, -(rrfb + kr + p.r1b), omega * rrfb],
        [
            0.0,
            0.0,
            0.0,
            omega * rrfb,
            -(omega * omega * rrfb + 1.0 / p.t1d),
        ],
    ]
}

/// Bound-pool RF saturation rates `(Rrfb_exc, Rrfd_exc)` from an excitation
/// pulse of nominal flip angle `flip_deg` and duration `w_exc_dur` (s),
/// on-resonance (`delta = 0`). `Rrfd_exc` is always zero: the excitation
/// pulse is on-resonance and does not drive dipolar order.
pub fn bound_exc_sat(flip_deg: f64, w_exc_dur: f64, t2b: f64) -> (f64, f64) {
    let gamma = crate::mtsat_b1::GAMMA;
    let b1 = flip_deg / (360.0 * gamma * w_exc_dur);
    let w1 = 2.0 * PI * gamma * b1;
    let rrfb = PI * w1 * w1 * super_lorentzian_g(0.0, t2b);
    (rrfb, 0.0)
}

/// Excitation propagator for state `[Wx, Wy, Wz, Bz, D]`: a rotation by `fa`
/// (rad) about the axis at phase `ph` (rad) in the transverse plane, applied
/// to the free-water block `[Wx, Wy, Wz]`, combined with bound-pool and
/// dipolar-order saturation decay `diag(Erfb, Erfd)` on `[Bz, D]` over the
/// pulse duration `w_exc_dur` (s), given the saturation rates
/// `(rrfb_exc, rrfd_exc)` from `bound_exc_sat`.
pub fn excitation_matrix(fa: f64, ph: f64, rrfb_exc: f64, rrfd_exc: f64, w_exc_dur: f64) -> Mat5 {
    let (c, s) = (fa.cos(), fa.sin());
    let (cp, sp) = (ph.cos(), ph.sin());
    let erfb = (-rrfb_exc * w_exc_dur).exp();
    let erfd = (-rrfd_exc * w_exc_dur).exp();
    [
        [
            c + (1.0 - c) * cp * cp,
            (1.0 - c) * sp * cp,
            -s * sp,
            0.0,
            0.0,
        ],
        [
            (1.0 - c) * sp * cp,
            c + (1.0 - c) * sp * sp,
            s * cp,
            0.0,
            0.0,
        ],
        [s * sp, -s * cp, c, 0.0, 0.0],
        [0.0, 0.0, 0.0, erfb, 0.0],
        [0.0, 0.0, 0.0, 0.0, erfd],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pp() -> PoolParams {
        PoolParams {
            ra: 1.0,
            r1b: 1.0,
            r: 26.0,
            m0a: 1.0,
            m0b: 0.1,
            t2a: 70e-3,
            t2b: 12e-6,
            t1d: 6e-3,
        }
    }

    #[test]
    fn no_rf_matrix_is_pure_exchange_relaxation() {
        let a = rate_matrix(&pp(), 0.0, 7000.0);
        let r2a = 1.0 / 70e-3;
        assert!((a[0][0] - -r2a).abs() < 1e-9);
        assert!((a[1][1] - -r2a).abs() < 1e-9);
        assert!((a[2][2] - -(1.0 + 26.0 * 0.1)).abs() < 1e-9); // -(Ra + R*M0b)
        assert!((a[2][3] - 26.0 * 1.0).abs() < 1e-9); // kr = R*M0a
        assert!((a[3][2] - 26.0 * 0.1).abs() < 1e-9); // kf = R*M0b
        assert!((a[3][3] - -(1.0 + 26.0 * 1.0)).abs() < 1e-9); // -(R1b + R*M0a), Rrfb=0
        assert!((a[4][4] - -1.0 / 6e-3).abs() < 1e-9); // -1/T1D
        assert!(a[3][4].abs() < 1e-12 && a[4][3].abs() < 1e-12); // Rrfb=0 -> no dipolar coupling
    }

    #[test]
    fn rf_field_couples_transverse_and_longitudinal_water() {
        let a = rate_matrix(&pp(), 5000.0, 7000.0);
        let tpd = 2.0 * PI * 7000.0;
        assert!((a[2][1] - 5000.0).abs() < 1e-9);
        assert!((a[1][2] - -5000.0).abs() < 1e-9);
        assert!((a[0][1] - -tpd).abs() < 1e-6);
        assert!((a[1][0] - tpd).abs() < 1e-6);
    }

    #[test]
    fn bound_exc_sat_matches_reference_formula() {
        let (flip_deg, w_exc_dur, t2b) = (90.0, 100e-6, 12e-6);
        let gamma = crate::mtsat_b1::GAMMA;
        let b1 = flip_deg / (360.0 * gamma * w_exc_dur);
        let w1 = 2.0 * PI * gamma * b1;
        let expected_rrfb = PI * w1 * w1 * super_lorentzian_g(0.0, t2b);

        let (rrfb, rrfd) = bound_exc_sat(flip_deg, w_exc_dur, t2b);
        assert!((rrfb - expected_rrfb).abs() < 1e-9 * expected_rrfb.abs().max(1.0));
        assert_eq!(rrfd, 0.0);
    }

    #[test]
    fn excitation_matrix_identity_at_zero_flip() {
        let (rrfb_exc, rrfd_exc) = (500.0, 0.0);
        let w_exc_dur = 100e-6;
        let m = excitation_matrix(0.0, 0.0, rrfb_exc, rrfd_exc, w_exc_dur);
        for (i, row) in m.iter().enumerate().take(3) {
            for (j, &v) in row.iter().enumerate().take(3) {
                let expected = if i == j { 1.0 } else { 0.0 };
                assert!((v - expected).abs() < 1e-12);
            }
        }
        let erfb = (-rrfb_exc * w_exc_dur).exp();
        let erfd = (-rrfd_exc * w_exc_dur).exp();
        assert!((m[3][3] - erfb).abs() < 1e-12);
        assert!((m[4][4] - erfd).abs() < 1e-12);
    }

    #[test]
    fn excitation_matrix_half_pi_rotates_wz_to_wy() {
        let m = excitation_matrix(PI / 2.0, 0.0, 0.0, 0.0, 100e-6);
        assert!(m[2][2].abs() < 1e-12);
        assert!(m[0][2].abs() < 1e-12);
        assert!((m[1][2] - 1.0).abs() < 1e-12);
    }
}
