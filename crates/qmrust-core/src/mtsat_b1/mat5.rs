//! Minimal fixed-size 5×5 linear algebra for the MTsat B1-correction
//! simulation: matrix product, matrix–vector product, a dense solve (Gaussian
//! elimination with partial pivoting), and a matrix exponential
//! (scaling-and-squaring with a Taylor series). Kept local and
//! dependency-free so `qmrust-core` stays wasm-clean.

pub type Mat5 = [[f64; 5]; 5];
pub type Vec5 = [f64; 5];

pub fn ident5() -> Mat5 {
    let mut m = [[0.0; 5]; 5];
    for (i, row) in m.iter_mut().enumerate() {
        row[i] = 1.0;
    }
    m
}

pub fn mul5(a: &Mat5, b: &Mat5) -> Mat5 {
    let mut c = [[0.0; 5]; 5];
    for i in 0..5 {
        for j in 0..5 {
            let mut s = 0.0;
            for k in 0..5 {
                s += a[i][k] * b[k][j];
            }
            c[i][j] = s;
        }
    }
    c
}

pub fn matvec5(a: &Mat5, v: &Vec5) -> Vec5 {
    let mut o = [0.0; 5];
    for i in 0..5 {
        let mut s = 0.0;
        for k in 0..5 {
            s += a[i][k] * v[k];
        }
        o[i] = s;
    }
    o
}

pub fn sub5(a: &Mat5, b: &Mat5) -> Mat5 {
    let mut c = [[0.0; 5]; 5];
    for i in 0..5 {
        for j in 0..5 {
            c[i][j] = a[i][j] - b[i][j];
        }
    }
    c
}

/// Solve `a x = b` for a 5×5 system via Gaussian elimination with partial
/// pivoting. Preconditions: `a` is nonsingular (the rate matrix always is
/// here).
pub fn solve5(a: &Mat5, b: &Vec5) -> Vec5 {
    let n = 5;
    let mut m = *a;
    let mut rhs = *b;
    for col in 0..n {
        let mut piv = col;
        for r in (col + 1)..n {
            if m[r][col].abs() > m[piv][col].abs() {
                piv = r;
            }
        }
        m.swap(col, piv);
        rhs.swap(col, piv);
        let d = m[col][col];
        for r in (col + 1)..n {
            let f = m[r][col] / d;
            if f != 0.0 {
                let (above, below) = m.split_at_mut(r);
                let pivot_row = &above[col];
                let target_row = &mut below[0];
                for (t, p) in target_row.iter_mut().zip(pivot_row).skip(col) {
                    *t -= f * p;
                }
                rhs[r] -= f * rhs[col];
            }
        }
    }
    let mut x = [0.0; 5];
    for row in (0..n).rev() {
        let mut s = rhs[row];
        for col in (row + 1)..n {
            s -= m[row][col] * x[col];
        }
        x[row] = s / m[row][row];
    }
    x
}

fn frob_norm5(a: &Mat5) -> f64 {
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
pub fn expm5(a: &Mat5) -> Mat5 {
    let norm = frob_norm5(a);
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
    let mut term = ident5();
    let mut sum = ident5();
    for k in 1..=18 {
        term = mul5(&term, &a_scaled);
        let inv = 1.0 / (1..=k).product::<u64>() as f64;
        for i in 0..5 {
            for j in 0..5 {
                sum[i][j] += term[i][j] * inv;
            }
        }
    }
    for _ in 0..s {
        sum = mul5(&sum, &sum);
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expm5_of_zero_is_identity() {
        let z = [[0.0; 5]; 5];
        let e = expm5(&z);
        assert_eq!(e, ident5());
    }

    #[test]
    fn expm5_of_diagonal_is_elementwise_exp() {
        let mut a = [[0.0; 5]; 5];
        let diag = [-2.0, -0.5, -10.0, -1.0, -3.0];
        for i in 0..5 {
            a[i][i] = diag[i];
        }
        let e = expm5(&a);
        for i in 0..5 {
            assert!((e[i][i] - diag[i].exp()).abs() < 1e-9, "{i}");
        }
        assert!(e[0][1].abs() < 1e-12);
    }

    #[test]
    fn expm5_matches_scalar_coupled_2x2_block() {
        // A = [[-1, 1],[0,-1]] embedded in the top-left 2x2 block; expm
        // known in closed form: e^{-1}[[1,1],[0,1]].
        let mut a = [[0.0; 5]; 5];
        a[0][0] = -1.0;
        a[0][1] = 1.0;
        a[1][1] = -1.0;
        let e = expm5(&a);
        let em1 = (-1.0f64).exp();
        assert!((e[0][0] - em1).abs() < 1e-9);
        assert!((e[0][1] - em1).abs() < 1e-9);
        assert!((e[1][1] - em1).abs() < 1e-9);
    }

    #[test]
    fn solve5_recovers_known_solution() {
        let a = [
            [4.0, 0.0, 1.0, 0.0, 0.0],
            [0.0, 3.0, 0.0, 1.0, 0.0],
            [1.0, 0.0, 5.0, 0.0, 1.0],
            [0.0, 1.0, 0.0, 6.0, 0.0],
            [0.0, 0.0, 1.0, 0.0, 2.0],
        ];
        let x = [1.0, -2.0, 3.0, 0.5, -1.5];
        let b = matvec5(&a, &x);
        let got = solve5(&a, &b);
        for i in 0..5 {
            assert!((got[i] - x[i]).abs() < 1e-9, "{i}");
        }
    }

    #[test]
    fn ident5_is_identity_under_matvec() {
        let v = [1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(matvec5(&ident5(), &v), v);
    }

    #[test]
    fn sub5_subtracts_elementwise() {
        let a = ident5();
        let b = ident5();
        let c = sub5(&a, &b);
        assert_eq!(c, [[0.0; 5]; 5]);
    }
}
