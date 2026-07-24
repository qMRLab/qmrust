//! Minimal fixed-size 3×3 linear algebra for the MTsat B1-correction
//! simulation: matrix product, matrix–vector product, a 3×3 solve (Cramer),
//! and a matrix exponential (scaling-and-squaring with a Taylor series). Kept
//! local and dependency-free so `qmrust-core` stays wasm-clean.

pub type Mat3 = [[f64; 3]; 3];
pub type Vec3 = [f64; 3];

pub fn ident3() -> Mat3 {
    [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
}

pub fn mul3(a: &Mat3, b: &Mat3) -> Mat3 {
    let mut c = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            c[i][j] = a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j];
        }
    }
    c
}

pub fn matvec3(a: &Mat3, v: &Vec3) -> Vec3 {
    let mut o = [0.0; 3];
    for i in 0..3 {
        o[i] = a[i][0] * v[0] + a[i][1] * v[1] + a[i][2] * v[2];
    }
    o
}

pub fn sub3(a: &Mat3, b: &Mat3) -> Mat3 {
    let mut c = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            c[i][j] = a[i][j] - b[i][j];
        }
    }
    c
}

fn det3(a: &Mat3) -> f64 {
    a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1])
        - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
        + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0])
}

/// Solve `a x = b` for a 3×3 system via Cramer's rule. Preconditions: `a` is
/// nonsingular (the rate matrix always is here — the dipolar `−1/T1D` term is
/// added precisely to keep it so).
pub fn solve3(a: &Mat3, b: &Vec3) -> Vec3 {
    let d = det3(a);
    let mut x = [0.0; 3];
    for col in 0..3 {
        let mut m = *a;
        for row in 0..3 {
            m[row][col] = b[row];
        }
        x[col] = det3(&m) / d;
    }
    x
}

fn frob_norm(a: &Mat3) -> f64 {
    let mut s = 0.0;
    for row in a {
        for x in row {
            s += x * x;
        }
    }
    s.sqrt()
}

/// Matrix exponential via scaling-and-squaring: scale `A` by 2^-s until its
/// norm is < 1/2, sum a truncated Taylor series, then square `s` times.
pub fn expm3(a: &Mat3) -> Mat3 {
    let norm = frob_norm(a);
    let s = if norm < 0.5 {
        0
    } else {
        (norm.log2().ceil() as i32 + 1).max(0) as u32
    };
    let scale = 2f64.powi(-(s as i32));
    let mut a_scaled = *a;
    for row in &mut a_scaled {
        for x in row {
            *x *= scale;
        }
    }
    // Taylor: I + A + A^2/2! + ... (18 terms is ample once ‖A‖ < 1/2).
    let mut term = ident3();
    let mut sum = ident3();
    for k in 1..=18 {
        term = mul3(&term, &a_scaled);
        let inv = 1.0 / (1..=k).product::<u64>() as f64;
        for i in 0..3 {
            for j in 0..3 {
                sum[i][j] += term[i][j] * inv;
            }
        }
    }
    for _ in 0..s {
        sum = mul3(&sum, &sum);
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expm_of_zero_is_identity() {
        let z = [[0.0; 3]; 3];
        let e = expm3(&z);
        assert_eq!(e, ident3());
    }

    #[test]
    fn expm_of_diagonal_is_elementwise_exp() {
        let a = [[-2.0, 0.0, 0.0], [0.0, -0.5, 0.0], [0.0, 0.0, -10.0]];
        let e = expm3(&a);
        assert!((e[0][0] - (-2.0f64).exp()).abs() < 1e-10);
        assert!((e[1][1] - (-0.5f64).exp()).abs() < 1e-10);
        assert!((e[2][2] - (-10.0f64).exp()).abs() < 1e-9);
        assert!(e[0][1].abs() < 1e-12);
    }

    #[test]
    fn solve3_recovers_known_solution() {
        let a = [[2.0, 0.0, 1.0], [0.0, 3.0, 0.0], [1.0, 0.0, 4.0]];
        let x = [1.0, -2.0, 3.0];
        let b = matvec3(&a, &x);
        let got = solve3(&a, &b);
        for i in 0..3 {
            assert!((got[i] - x[i]).abs() < 1e-10, "{i}");
        }
    }

    #[test]
    fn expm_matches_scalar_coupled_2x2_block() {
        // A = [[-1, 1],[0,-1]] embedded; expm known: e^{-1}[[1,1],[0,1]]
        let a = [[-1.0, 1.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 0.0]];
        let e = expm3(&a);
        let em1 = (-1.0f64).exp();
        assert!((e[0][0] - em1).abs() < 1e-9);
        assert!((e[0][1] - em1).abs() < 1e-9);
        assert!((e[1][1] - em1).abs() < 1e-9);
    }
}
