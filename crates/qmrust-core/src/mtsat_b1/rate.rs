//! 5-state Bloch–McConnell rate matrix, ported from
//! <ref>/functions/Bloch_McConnell_wDipolar.m (water-excitation branch).
//! State `M = [Wx, Wy, Wz, Bz, D]`: free-water transverse x/y, free-water
//! longitudinal, bound-pool longitudinal, dipolar order. Bound-pool absorption
//! uses the in-tree 2π super-Lorentzian lineshape (`super_lorentzian_g`):
//! `Rrfb = π·w1²·G(δ, T2b)`.

use crate::models::qmt_spgr::lineshape::super_lorentzian_g;
use crate::mtsat_b1::mat3::Mat5;
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
}
