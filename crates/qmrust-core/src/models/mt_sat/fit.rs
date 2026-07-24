//! Magnetization transfer saturation (MTsat) — Helms et al. MRM 60:1396 (2008)
//! with the erratum in MRM 64:1856 (2010). A closed-form, T1- and (optionally
//! B1-) corrected combination of three spoiled-gradient-echo signals (MTw,
//! PDw, T1w), not an iterative fit.
//!
//! From the small-flip-angle SPGR signal
//!   S_X = A·α_X·(R1·TR_X) / (R1·TR_X + α_X²/2 [+ δ for MTw])
//! the PDw/T1w pair inverts to R1 and the apparent signal amplitude A, and the
//! MTw signal then yields the MT saturation δ. All flip angles are in radians,
//! all TRs in seconds; MTsat and MTR are reported in percent, R1 in 1/s.

/// The three weightings' acquisition parameters: flip angle (radians) and
/// repetition time (seconds) for each of MTw, PDw, T1w.
#[derive(Debug, Clone, Copy)]
pub struct Acq {
    pub alpha_mt: f64,
    pub tr_mt: f64,
    pub alpha_pd: f64,
    pub tr_pd: f64,
    pub alpha_t1: f64,
    pub tr_t1: f64,
}

/// MTsat (percent) and R1 (1/s) from the three signals, with an optional
/// multiplicative B1 map (`FA_actual = B1·FA_nominal`). Mirrors `MTSAT_exec.m`:
/// outside the region where all three signals are nonzero, R1 and MTsat are 0
/// (so T1 = 1/R1 is infinite, as in qMRLab). B1-correction of R1/A is applied
/// whenever `b1` is `Some`; the empirical Helms multiplicative factor
/// `(1-f)/(1-f·b1)` is a separate, independent knob applied to MTsat only when
/// `apply_helms` is true (and a B1 map is present) — the two paths that feed a
/// B1-vs-R1 calibration surface must use the same R1/MTsat coordinates, so
/// only one of them should set `apply_helms`.
pub fn mtsat(
    acq: &Acq,
    mtw: f64,
    pdw: f64,
    t1w: f64,
    b1: Option<f64>,
    apply_helms: bool,
    b1_factor: f64,
) -> (f64, f64) {
    if mtw == 0.0 || pdw == 0.0 || t1w == 0.0 {
        return (0.0, 0.0);
    }
    let b = b1.unwrap_or(1.0);
    let r1 = 0.5 * b * b * ((acq.alpha_t1 / acq.tr_t1) * t1w - (acq.alpha_pd / acq.tr_pd) * pdw)
        / (pdw / acq.alpha_pd - t1w / acq.alpha_t1);
    let a = (1.0 / b)
        * (acq.tr_pd * acq.alpha_t1 / acq.alpha_pd - acq.tr_t1 * acq.alpha_pd / acq.alpha_t1)
        * (pdw * t1w)
        / (acq.tr_pd * acq.alpha_t1 * t1w - acq.tr_t1 * acq.alpha_pd * pdw);
    let mut mtsat = 100.0
        * (acq.tr_mt * (acq.alpha_mt * (a / mtw) - 1.0) * r1 - acq.alpha_mt * acq.alpha_mt / 2.0);
    if apply_helms {
        if let Some(b1v) = b1 {
            mtsat *= (1.0 - b1_factor) / (1.0 - b1_factor * b1v);
        }
    }
    (mtsat, r1)
}

/// MTR (percent) from the PD-weighted and MT-weighted signals, matching
/// `mt_ratio`: non-finite (PDw == 0) collapses to 0.
pub fn mtr(pdw: f64, mtw: f64) -> f64 {
    let r = 100.0 * (pdw - mtw) / pdw;
    if r.is_finite() {
        r
    } else {
        0.0
    }
}

/// Small-flip-angle SPGR signals `(MTw, PDw, T1w)` for a known amplitude `a`,
/// `t1` (s) and MT saturation `mtsat_pct` (percent) — the inverse of [`mtsat`],
/// used by the forward model / sim round-trip.
pub fn forward_signals(acq: &Acq, a: f64, t1: f64, mtsat_pct: f64) -> (f64, f64, f64) {
    let r1 = 1.0 / t1;
    let delta = mtsat_pct / 100.0;
    let spgr = |alpha: f64, tr: f64, extra: f64| {
        a * alpha * (r1 * tr) / (r1 * tr + alpha * alpha / 2.0 + extra)
    };
    let mtw = spgr(acq.alpha_mt, acq.tr_mt, delta);
    let pdw = spgr(acq.alpha_pd, acq.tr_pd, 0.0);
    let t1w = spgr(acq.alpha_t1, acq.tr_t1, 0.0);
    (mtw, pdw, t1w)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The example protocol (degrees → radians): MTw 6°/28 ms, PDw 6°/28 ms,
    /// T1w 20°/18 ms.
    fn acq() -> Acq {
        let d2r = std::f64::consts::PI / 180.0;
        Acq {
            alpha_mt: 6.0 * d2r,
            tr_mt: 0.028,
            alpha_pd: 6.0 * d2r,
            tr_pd: 0.028,
            alpha_t1: 20.0 * d2r,
            tr_t1: 0.018,
        }
    }

    #[test]
    fn forward_then_mtsat_recovers_t1_and_saturation() {
        let acq = acq();
        for &(a, t1, sat) in &[(1000.0, 0.9, 1.5), (500.0, 1.2, 3.0), (2000.0, 0.6, 0.8)] {
            let (mtw, pdw, t1w) = forward_signals(&acq, a, t1, sat);
            let (mtsat_out, r1) = mtsat(&acq, mtw, pdw, t1w, None, false, 0.4);
            assert!((1.0 / r1 - t1).abs() < 1e-9, "T1: {} vs {}", 1.0 / r1, t1);
            assert!(
                (mtsat_out - sat).abs() < 1e-9,
                "MTsat: {mtsat_out} vs {sat}"
            );
        }
    }

    #[test]
    fn degenerate_voxel_is_zero() {
        let acq = acq();
        assert_eq!(mtsat(&acq, 0.0, 1.0, 1.0, None, false, 0.4), (0.0, 0.0));
        assert_eq!(mtsat(&acq, 1.0, 0.0, 1.0, None, false, 0.4), (0.0, 0.0));
    }

    #[test]
    fn b1_of_one_matches_no_b1() {
        let acq = acq();
        let (mtw, pdw, t1w) = forward_signals(&acq, 1000.0, 0.9, 1.5);
        let none = mtsat(&acq, mtw, pdw, t1w, None, false, 0.4);
        // B1 = 1 with any factor: R1·1, A·1, and (1-f)/(1-f·1) = 1 → identical.
        let unit = mtsat(&acq, mtw, pdw, t1w, Some(1.0), true, 0.4);
        assert!((none.0 - unit.0).abs() < 1e-9 && (none.1 - unit.1).abs() < 1e-12);
    }

    #[test]
    fn apply_helms_toggles_mtsat_but_not_r1() {
        let acq = acq();
        let (mtw, pdw, t1w) = forward_signals(&acq, 1000.0, 0.9, 1.5);
        let b1 = Some(1.2);
        let raw = mtsat(&acq, mtw, pdw, t1w, b1, false, 0.4);
        let helms = mtsat(&acq, mtw, pdw, t1w, b1, true, 0.4);
        assert!(
            (raw.0 - helms.0).abs() > 1e-6,
            "empirical Helms factor should change MTsat: raw {} vs helms {}",
            raw.0,
            helms.0
        );
        assert!(
            (raw.1 - helms.1).abs() < 1e-12,
            "R1 must be identical regardless of apply_helms: {} vs {}",
            raw.1,
            helms.1
        );
    }

    #[test]
    fn mtr_matches_definition_and_guards_zero() {
        assert!((mtr(200.0, 150.0) - 25.0).abs() < 1e-12);
        assert_eq!(mtr(0.0, 5.0), 0.0);
    }
}
