//! Magnetization transfer ratio (MTR): a closed-form two-input ratio, not an
//! iterative fit.
//!
//!   MTR = 100 * (MToff - MTon) / MToff   [percent]
//!
//! `MTon` is the MT-weighted signal (spoiled gradient echo with an MT pulse),
//! `MToff` the reference without it. A non-finite result — `MToff == 0`, giving
//! `Inf`/`NaN` — collapses to 0, matching qMRLab's `mt_ratio.fit`.

/// MTR (percent) from the MT-off and MT-on signals.
pub fn mtr(mt_off: f64, mt_on: f64) -> f64 {
    let r = 100.0 * (mt_off - mt_on) / mt_off;
    if r.is_finite() {
        r
    } else {
        0.0
    }
}

/// The MT-on signal that produces `mtr_percent` at a reference MT-off level of
/// 1.0 — the inverse the forward model / round-trip uses. With `MToff ≡ 1`,
/// `MTon = 1 − MTR/100`, so `mtr(1.0, forward_mton(x)) == x`.
pub fn forward_mton(mtr_percent: f64) -> f64 {
    1.0 - mtr_percent / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratio_matches_definition() {
        // 100 * (200 - 150) / 200 = 25%
        assert!((mtr(200.0, 150.0) - 25.0).abs() < 1e-12);
    }

    #[test]
    fn zero_reference_collapses_to_zero() {
        assert_eq!(mtr(0.0, 5.0), 0.0); // -inf -> 0
        assert_eq!(mtr(0.0, 0.0), 0.0); // NaN  -> 0
    }

    #[test]
    fn forward_inverts_the_ratio() {
        for &x in &[-10.0, 0.0, 12.5, 50.0, 100.0] {
            assert!((mtr(1.0, forward_mton(x)) - x).abs() < 1e-12, "x={x}");
        }
    }
}
