//! Bounded nonlinear least-squares fit of the Ramani model via
//! levenberg-marquardt with a smooth sigmoid reparameterization for bounds.

use super::model::{ramani_signal, FitOpt};
use levenberg_marquardt::{LeastSquaresProblem, LevenbergMarquardt};
use nalgebra::{DMatrix, DVector, Dyn, Owned};

/// Fit configuration: starting point, bounds, and fixed-parameter mask.
pub struct FitBounds {
    pub st: [f64; 6],
    pub lb: [f64; 6],
    pub ub: [f64; 6],
    pub fx: [bool; 6],
}

// sigmoid and its inverse for mapping free params in/out of bounds.
fn sigmoid(z: f64) -> f64 {
    1.0 / (1.0 + (-z).exp())
}
fn logit(p: f64) -> f64 {
    (p / (1.0 - p)).ln()
}

/// Map an in-bounds parameter to unconstrained z (initial guess).
fn to_z(p: f64, lb: f64, ub: f64) -> f64 {
    let frac = ((p - lb) / (ub - lb)).clamp(1e-6, 1.0 - 1e-6);
    logit(frac)
}
/// Map unconstrained z back to an in-bounds parameter.
fn from_z(z: f64, lb: f64, ub: f64) -> f64 {
    lb + (ub - lb) * sigmoid(z)
}

struct BoundedProblem<F: Fn(&[f64; 6]) -> Vec<f64>> {
    z: DVector<f64>,
    free: Vec<usize>,
    st: [f64; 6],
    lb: [f64; 6],
    ub: [f64; 6],
    mtdata: Vec<f64>,
    model_fn: F,
}

impl<F: Fn(&[f64; 6]) -> Vec<f64>> BoundedProblem<F> {
    fn full_x(&self, z: &DVector<f64>) -> [f64; 6] {
        let mut x = self.st;
        for (k, &i) in self.free.iter().enumerate() {
            x[i] = from_z(z[k], self.lb[i], self.ub[i]);
        }
        x
    }
    fn residual_for(&self, z: &DVector<f64>) -> DVector<f64> {
        let x = self.full_x(z);
        let model = (self.model_fn)(&x);
        DVector::from_iterator(
            model.len(),
            model.iter().zip(self.mtdata.iter()).map(|(m, d)| m - d),
        )
    }
}

impl<F: Fn(&[f64; 6]) -> Vec<f64>> LeastSquaresProblem<f64, Dyn, Dyn> for BoundedProblem<F> {
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
        let m = self.mtdata.len();
        let n = self.free.len();
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

/// Bounded LM fit of an arbitrary 6-parameter model. `model_fn(x)` returns the
/// model signal vector (same length as `mtdata`). Returns full `x` and resnorm.
pub fn fit_bounded<F: Fn(&[f64; 6]) -> Vec<f64>>(
    mtdata: &[f64],
    bounds: &FitBounds,
    model_fn: F,
) -> ([f64; 6], f64) {
    let free: Vec<usize> = (0..6).filter(|&i| !bounds.fx[i]).collect();
    let z0 = DVector::from_iterator(
        free.len(),
        free.iter()
            .map(|&i| to_z(bounds.st[i], bounds.lb[i], bounds.ub[i])),
    );
    let problem = BoundedProblem {
        z: z0,
        free,
        st: bounds.st,
        lb: bounds.lb,
        ub: bounds.ub,
        mtdata: mtdata.to_vec(),
        model_fn,
    };
    let (result, _report) = LevenbergMarquardt::new().minimize(problem);
    let x = result.full_x(&result.z);
    let res = result.residual_for(&result.z);
    let resnorm = res.iter().map(|r| r * r).sum();
    (x, resnorm)
}

/// Fit the Ramani sub-model: bounded LM over the Ramani CW-power signal model.
pub fn fit_ramani(
    mtdata: &[f64],
    offsets: &[f64],
    w1cw: &[f64],
    bounds: &FitBounds,
    opt: &FitOpt,
) -> ([f64; 6], f64) {
    fit_bounded(mtdata, bounds, |x| ramani_signal(x, offsets, w1cw, opt))
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

    fn default_bounds() -> FitBounds {
        FitBounds {
            st: [0.16, 30.0, 1.0, 1.0, 0.03, 1.3e-5],
            lb: [1e-4, 1e-4, 0.05, 0.05, 0.003, 3e-6],
            ub: [0.5, 100.0, 5.0, 5.0, 0.5, 5e-5],
            // Fit all 4 default-free params: F, kr, T2f, T2r
            fx: [false, false, true, true, false, false],
        }
    }

    #[test]
    fn recovers_synthetic_params() {
        let offsets = [443.0, 1088.0, 2732.0, 6862.0, 17235.0];
        let w1cw = [50.0, 50.0, 50.0, 50.0, 50.0];
        let truth = [0.15, 25.0, 1.0, 1.0, 0.028, 1.1e-5];
        let data = ramani_signal(&truth, &offsets, &w1cw, &opt_none());

        let (x, resnorm) = fit_ramani(&data, &offsets, &w1cw, &default_bounds(), &opt_none());
        assert!(resnorm < 1e-6, "resnorm too high: {}", resnorm);
        assert!((x[0] - 0.15).abs() < 0.02, "F: {}", x[0]);
        assert!((x[4] - 0.028).abs() < 0.005, "T2f: {}", x[4]);
    }

    #[test]
    fn respects_bounds() {
        let offsets = [443.0, 2732.0, 17235.0];
        let w1cw = [50.0, 50.0, 50.0];
        // Garbage data forces the optimizer toward an edge; params must stay in-bounds.
        let data = [0.01, 0.01, 0.01];
        let b = default_bounds();
        let (x, _) = fit_ramani(&data, &offsets, &w1cw, &b, &opt_none());
        for (i, ((xi, lb), ub)) in x.iter().zip(b.lb.iter()).zip(b.ub.iter()).enumerate() {
            assert!(
                *xi >= lb - 1e-9 && *xi <= ub + 1e-9,
                "x[{}]={} out of [{},{}]",
                i,
                xi,
                lb,
                ub
            );
        }
    }

    #[test]
    fn fit_bounded_matches_fit_ramani() {
        let offsets = [443.0, 1088.0, 2732.0, 6862.0, 17235.0];
        let w1cw = [50.0, 50.0, 50.0, 50.0, 50.0];
        let truth = [0.15, 25.0, 1.0, 1.0, 0.028, 1.1e-5];
        let data = ramani_signal(&truth, &offsets, &w1cw, &opt_none());
        let b = default_bounds();
        let (x_wrap, _) = fit_ramani(&data, &offsets, &w1cw, &b, &opt_none());
        let (x_gen, _) = fit_bounded(&data, &b, |x| {
            ramani_signal(x, &offsets, &w1cw, &opt_none())
        });
        for i in 0..6 {
            assert!(
                (x_wrap[i] - x_gen[i]).abs() < 1e-9,
                "param {} differs: {} vs {}",
                i,
                x_wrap[i],
                x_gen[i]
            );
        }
    }
}
