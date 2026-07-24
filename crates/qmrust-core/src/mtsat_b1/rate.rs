//! Longitudinal 3-pool (free/bound/dipolar) rate matrix, ported from
//! <ref>/functions/calc_RF_matrix_wDipolar2.m (Lee 2011 dipolar form).

use crate::mtsat_b1::lineshape::{absorption, Lineshape};
use crate::mtsat_b1::mat3::Mat3;
use std::f64::consts::PI;

pub struct PoolParams {
    pub ra: f64,
    pub rb: f64,
    pub r: f64,
    pub m0a: f64,
    pub m0b: f64,
    pub t2a: f64,
    pub t2b: f64,
    pub t1d: f64,
    pub lineshape: Lineshape,
}

/// Rate matrix `A` for `dM/dt = A·M + B`, state `[Mza, Mzb, Bpr]`. `w1` in
/// rad/s, `delta` in Hz. `dual_continuous` uncouples the dipolar pool.
pub fn rate_matrix(p: &PoolParams, w1: f64, delta: f64, dual_continuous: bool) -> Mat3 {
    let rrfa = (w1 * w1 * p.t2a) / (1.0 + (2.0 * PI * delta * p.t2a).powi(2));
    let (rrfb, wloc) = match p.lineshape {
        Lineshape::SuperLorentzian => (
            PI * w1 * w1 * absorption(Lineshape::SuperLorentzian, p.t2b, delta),
            (1.0 / (15.0 * p.t2b * p.t2b)).sqrt(),
        ),
        Lineshape::Gaussian => (
            w1 * w1 * absorption(Lineshape::Gaussian, p.t2b, delta),
            (1.0 / (3.0 * p.t2b * p.t2b)).sqrt(),
        ),
    };

    if dual_continuous {
        [
            [-(p.ra + p.r * p.m0b + rrfa), p.r * p.m0a, 0.0],
            [p.r * p.m0b, -(p.rb + rrfb + p.r * p.m0a), 0.0],
            [0.0, 0.0, -1.0 / p.t1d],
        ]
    } else {
        let two_pi_delta = 2.0 * PI * delta;
        [
            [-(p.ra + p.r * p.m0b + rrfa), p.r * p.m0a, 0.0],
            [
                p.r * p.m0b,
                -(p.rb + rrfb + p.r * p.m0a),
                two_pi_delta * rrfb / wloc,
            ],
            [
                0.0,
                rrfb * (two_pi_delta / wloc),
                -(rrfb * (two_pi_delta / wloc).powi(2) + 1.0 / p.t1d),
            ],
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mtsat_b1::lineshape::Lineshape;

    fn pp() -> PoolParams {
        PoolParams {
            ra: 1.0,
            rb: 1.0,
            r: 26.0,
            m0a: 1.0,
            m0b: 0.1,
            t2a: 70e-3,
            t2b: 12e-6,
            t1d: 6e-3,
            lineshape: Lineshape::SuperLorentzian,
        }
    }

    #[test]
    fn no_rf_matrix_is_pure_exchange_relaxation() {
        // w1 = 0 → Rrfa = Rrfb = 0. Matches the hand-derived free/bound block.
        let a = rate_matrix(&pp(), 0.0, 7000.0, false);
        assert!((a[0][0] - -(1.0 + 26.0 * 0.1)).abs() < 1e-12); // -(Ra + R*M0b)
        assert!((a[0][1] - 26.0 * 1.0).abs() < 1e-12); // R*M0a
        assert!((a[1][0] - 26.0 * 0.1).abs() < 1e-12); // R*M0b
        assert!((a[1][1] - -(1.0 + 26.0 * 1.0)).abs() < 1e-12); // -(Rb + R*M0a)
        assert!((a[2][2] - -1.0 / 6e-3).abs() < 1e-9); // -1/T1D
        assert!(a[1][2].abs() < 1e-12 && a[2][1].abs() < 1e-12); // Rrfb=0 → no dipolar coupling
    }

    #[test]
    fn dual_continuous_uncouples_dipolar_pool() {
        let a = rate_matrix(&pp(), 5000.0, 7000.0, true);
        assert!(a[0][2] == 0.0 && a[1][2] == 0.0 && a[2][0] == 0.0 && a[2][1] == 0.0);
        assert!((a[2][2] - -1.0 / 6e-3).abs() < 1e-9);
    }
}
