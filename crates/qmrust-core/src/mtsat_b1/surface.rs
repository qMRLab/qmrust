//! Tricubic (64-term) least-squares surface `MTsat = SS(M0b, b1, Raobs)` and
//! its evaluator. Term order is fixed (M0b outer, b1 middle, Raobs inner, each
//! degree 0..3), matching simSeq_M0b_R1obs.m so coefficients are comparable to
//! the reference.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct SsSurface {
    pub coeffs: [f64; 64],
}

// `serde`'s derive only covers arrays up to 32 elements, so the 64-term
// coefficient array needs a manual (de)serializer as a fixed-length tuple.
impl Serialize for SsSurface {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeTuple;
        let mut tup = serializer.serialize_tuple(64)?;
        for c in &self.coeffs {
            tup.serialize_element(c)?;
        }
        tup.end()
    }
}

impl<'de> Deserialize<'de> for SsSurface {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct CoeffsVisitor;
        impl<'de> serde::de::Visitor<'de> for CoeffsVisitor {
            type Value = [f64; 64];

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "a tuple of 64 f64 values")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let mut coeffs = [0.0f64; 64];
                for (i, c) in coeffs.iter_mut().enumerate() {
                    *c = seq
                        .next_element()?
                        .ok_or_else(|| serde::de::Error::invalid_length(i, &self))?;
                }
                Ok(coeffs)
            }
        }
        let coeffs = deserializer.deserialize_tuple(64, CoeffsVisitor)?;
        Ok(SsSurface { coeffs })
    }
}

/// Exponents `(i,j,k)` of `M0b^i · b1^j · Raobs^k` for basis term `idx`.
pub fn term(idx: usize) -> (u32, u32, u32) {
    let i = (idx / 16) as u32;
    let j = ((idx / 4) % 4) as u32;
    let k = (idx % 4) as u32;
    (i, j, k)
}

fn basis(m0b: f64, b1: f64, raobs: f64) -> [f64; 64] {
    let mut row = [0.0; 64];
    for (idx, r) in row.iter_mut().enumerate() {
        let (i, j, k) = term(idx);
        *r = m0b.powi(i as i32) * b1.powi(j as i32) * raobs.powi(k as i32);
    }
    row
}

impl SsSurface {
    pub fn eval(&self, m0b: f64, b1: f64, raobs: f64) -> f64 {
        let row = basis(m0b, b1, raobs);
        row.iter().zip(&self.coeffs).map(|(x, c)| x * c).sum()
    }
}

pub fn fit(samples: &[([f64; 3], f64)]) -> SsSurface {
    let rows: Vec<[f64; 64]> = samples
        .iter()
        .map(|(x, _)| basis(x[0], x[1], x[2]))
        .collect();
    let y: Vec<f64> = samples.iter().map(|(_, v)| *v).collect();
    SsSurface {
        coeffs: solve_lstsq(&rows, &y),
    }
}

/// Least squares via normal equations XᵀX c = Xᵀy, solved by Gaussian
/// elimination with partial pivoting. 64 unknowns; inputs scaled to O(1) so
/// conditioning is acceptable for this basis.
pub fn solve_lstsq(rows: &[[f64; 64]], y: &[f64]) -> [f64; 64] {
    let mut ata = [[0.0f64; 64]; 64];
    let mut aty = [0.0f64; 64];
    for (row, &yi) in rows.iter().zip(y) {
        for a in 0..64 {
            aty[a] += row[a] * yi;
            for b in 0..64 {
                ata[a][b] += row[a] * row[b];
            }
        }
    }
    // Solve ata c = aty.
    let n = 64;
    for col in 0..n {
        // partial pivot
        let mut piv = col;
        for r in (col + 1)..n {
            if ata[r][col].abs() > ata[piv][col].abs() {
                piv = r;
            }
        }
        ata.swap(col, piv);
        aty.swap(col, piv);
        let d = ata[col][col];
        for r in (col + 1)..n {
            let f = ata[r][col] / d;
            if f != 0.0 {
                let (above, below) = ata.split_at_mut(r);
                let pivot_row = &above[col];
                let target_row = &mut below[0];
                for (t, p) in target_row.iter_mut().zip(pivot_row).skip(col) {
                    *t -= f * p;
                }
                aty[r] -= f * aty[col];
            }
        }
    }
    let mut c = [0.0f64; 64];
    for row in (0..n).rev() {
        let mut s = aty[row];
        for col in (row + 1)..n {
            s -= ata[row][col] * c[col];
        }
        c[row] = s / ata[row][row];
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    // A surface that IS a tricubic polynomial must be recovered exactly.
    fn truth(m: f64, b: f64, r: f64) -> f64 {
        1.0 + 2.0 * r - 0.5 * b * r + 3.0 * m + m * b * r - 0.25 * m * m * r * r * r
    }

    #[test]
    fn fit_recovers_exact_polynomial() {
        let mut samples = Vec::new();
        for mi in 0..5 {
            for bi in 0..5 {
                for ri in 0..5 {
                    let m = mi as f64 * 0.05;
                    let b = bi as f64 * 2.0;
                    let r = 0.3 + ri as f64 * 0.3;
                    samples.push(([m, b, r], truth(m, b, r)));
                }
            }
        }
        let s = fit(&samples);
        for &([m, b, r], _) in samples.iter().take(20) {
            assert!(
                (s.eval(m, b, r) - truth(m, b, r)).abs() < 1e-6,
                "at {m},{b},{r}"
            );
        }
    }
}
