//! Adaptive Simpson quadrature — self-contained numeric integration
//! used for RF pulse power integrals and the SuperLorentzian lineshape.

/// Integrate `f` over `[a, b]` using recursive adaptive Simpson's rule.
pub fn adaptive_simpson<F: Fn(f64) -> f64>(f: &F, a: f64, b: f64, tol: f64) -> f64 {
    fn simpson<F: Fn(f64) -> f64>(f: &F, a: f64, b: f64) -> (f64, f64) {
        let m = 0.5 * (a + b);
        let fm = f(m);
        ((b - a) / 6.0 * (f(a) + 4.0 * fm + f(b)), fm)
    }
    // clippy: recursive adaptive-Simpson state (interval bounds, cached f-values,
    // running estimate, tolerance, recursion depth) is inherent to the algorithm;
    // splitting it into a struct would not change behavior but adds churn here.
    #[allow(clippy::too_many_arguments)]
    fn recurse<F: Fn(f64) -> f64>(
        f: &F,
        a: f64,
        b: f64,
        fa: f64,
        fb: f64,
        whole: f64,
        fm: f64,
        tol: f64,
        depth: i32,
    ) -> f64 {
        let m = 0.5 * (a + b);
        let lm = 0.5 * (a + m);
        let rm = 0.5 * (m + b);
        let flm = f(lm);
        let frm = f(rm);
        let left = (m - a) / 6.0 * (fa + 4.0 * flm + fm);
        let right = (b - m) / 6.0 * (fm + 4.0 * frm + fb);
        if depth <= 0 || (left + right - whole).abs() <= 15.0 * tol {
            return left + right + (left + right - whole) / 15.0;
        }
        recurse(f, a, m, fa, fm, left, flm, tol / 2.0, depth - 1)
            + recurse(f, m, b, fm, fb, right, frm, tol / 2.0, depth - 1)
    }
    let fa = f(a);
    let fb = f(b);
    let (whole, fm) = simpson(f, a, b);
    recurse(f, a, b, fa, fb, whole, fm, tol, 50)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integrates_polynomial_exactly() {
        // ∫₀¹ x² dx = 1/3
        let v = adaptive_simpson(&|x| x * x, 0.0, 1.0, 1e-10);
        assert!((v - 1.0 / 3.0).abs() < 1e-9, "got {}", v);
    }

    #[test]
    fn integrates_gaussian() {
        // ∫₀¹ exp(-x²) dx ≈ 0.7468241328
        let v = adaptive_simpson(&|x| (-x * x).exp(), 0.0, 1.0, 1e-10);
        assert!((v - 0.746824132812427).abs() < 1e-8, "got {}", v);
    }
}
