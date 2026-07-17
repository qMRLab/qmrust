//! Bogacki–Shampine (RK23, == MATLAB ode23) adaptive integrator and the
//! free-pool Bloch RHS without MT (BlochNoMT.m).

use std::f64::consts::PI;

/// Free-pool magnetization derivative (no MT, no T1 recovery).
/// m = [Mx, My, Mz]; delta in Hz; omega = gamma*amp*b1(t).
pub fn bloch_no_mt_deriv(m: &[f64; 3], t2f: f64, delta: f64, omega: f64) -> [f64; 3] {
    let two_pi_d = 2.0 * PI * delta;
    [
        -m[0] / t2f - two_pi_d * m[1],
        -m[1] / t2f + two_pi_d * m[0] + omega * m[2],
        -omega * m[1],
    ]
}

/// Hard cap on iterations of the main adaptive loop in `rk_bs23`. Normal
/// (fast-converging) integrations finish in well under a few hundred steps;
/// this cap only exists to guarantee termination for pathological RHS that
/// would otherwise stall on ever-shrinking step sizes.
const MAX_STEPS: usize = 1_000_000;

/// Integrate y' = rhs(t, y) from t0 to t1 with adaptive Bogacki–Shampine RK23.
pub fn rk_bs23<F: Fn(f64, &[f64; 3]) -> [f64; 3]>(
    rhs: &F,
    t0: f64,
    t1: f64,
    y0: [f64; 3],
    rtol: f64,
    atol: f64,
) -> [f64; 3] {
    let add =
        |a: &[f64; 3], b: &[f64; 3], s: f64| [a[0] + s * b[0], a[1] + s * b[1], a[2] + s * b[2]];
    let mut t = t0;
    let mut y = y0;
    let span = t1 - t0;
    let mut h = span / 100.0; // initial step
    let mut k1 = rhs(t, &y);
    let mut steps: usize = 0;
    while t < t1 {
        steps += 1;
        if steps >= MAX_STEPS {
            break;
        }
        if t + h > t1 {
            h = t1 - t;
        }
        // BS23 stages (FSAL: k1 carried from previous accepted step)
        let k2 = rhs(t + 0.5 * h, &add(&y, &k1, 0.5 * h));
        let k3 = rhs(t + 0.75 * h, &add(&y, &k2, 0.75 * h));
        let y3 = [
            y[0] + h * (2.0 / 9.0 * k1[0] + 1.0 / 3.0 * k2[0] + 4.0 / 9.0 * k3[0]),
            y[1] + h * (2.0 / 9.0 * k1[1] + 1.0 / 3.0 * k2[1] + 4.0 / 9.0 * k3[1]),
            y[2] + h * (2.0 / 9.0 * k1[2] + 1.0 / 3.0 * k2[2] + 4.0 / 9.0 * k3[2]),
        ];
        let k4 = rhs(t + h, &y3);
        // 2nd-order estimate for error control
        let mut err: f64 = 0.0;
        for i in 0..3 {
            let z = y[i]
                + h * (7.0 / 24.0 * k1[i]
                    + 1.0 / 4.0 * k2[i]
                    + 1.0 / 3.0 * k3[i]
                    + 1.0 / 8.0 * k4[i]);
            let sc = atol + rtol * y[i].abs().max(y3[i].abs());
            let e = (y3[i] - z) / sc;
            err += e * e;
        }
        let err = (err / 3.0).sqrt();
        if err <= 1.0 || h <= span * 1e-10 {
            // accept
            t += h;
            y = y3;
            k1 = k4; // FSAL
        }
        // step-size update (clamped)
        let factor = if err == 0.0 {
            5.0
        } else {
            0.9 * err.powf(-1.0 / 3.0)
        };
        h *= factor.clamp(0.2, 5.0);
    }
    y
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_exponential_decay() {
        // y' = -y on component 0 → y(1) = e^{-1}. Use a pure-decay rhs.
        let rhs = |_t: f64, y: &[f64; 3]| [-y[0], 0.0, 0.0];
        let out = rk_bs23(&rhs, 0.0, 1.0, [1.0, 0.0, 0.0], 1e-6, 1e-9);
        assert!((out[0] - (-1.0_f64).exp()).abs() < 1e-4, "got {}", out[0]);
    }

    #[test]
    fn zero_omega_keeps_mz() {
        // With omega=0 and Mx=My=0, Mz has zero derivative → stays 1.
        let rhs = |_t: f64, m: &[f64; 3]| bloch_no_mt_deriv(m, 0.03, 2732.0, 0.0);
        let out = rk_bs23(&rhs, 0.0, 0.0102, [0.0, 0.0, 1.0], 1e-3, 1e-6);
        assert!(
            (out[2] - 1.0).abs() < 1e-9,
            "Mz should stay 1, got {}",
            out[2]
        );
    }

    #[test]
    fn terminates_on_stiff_rhs() {
        // Very fast decay relative to the interval; must terminate (not hang)
        // and return finite state.
        let rhs = |_t: f64, y: &[f64; 3]| [-1.0e8 * y[0], 0.0, 0.0];
        let out = rk_bs23(&rhs, 0.0, 1.0, [1.0, 0.0, 0.0], 1e-3, 1e-6);
        assert!(out[0].is_finite(), "must return finite, got {}", out[0]);
    }

    #[test]
    fn bloch_deriv_formula() {
        let d = bloch_no_mt_deriv(&[1.0, 2.0, 3.0], 0.05, 100.0, 7.0);
        let two_pi_d = 2.0 * PI * 100.0;
        assert!((d[0] - (-1.0 / 0.05 - two_pi_d * 2.0)).abs() < 1e-9);
        assert!((d[1] - (-2.0 / 0.05 + two_pi_d * 1.0 + 7.0 * 3.0)).abs() < 1e-9);
        assert!((d[2] - (-7.0 * 2.0)).abs() < 1e-9);
    }
}
