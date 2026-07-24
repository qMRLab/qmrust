//! 5-state Bloch–McConnell FLASH signal engine, ported from
//! <ref>/functions/Bloch_McConnell_wDipolar.m and the surrounding MTsat B1+
//! simulation. Time-steps `dM/dt = A·M + B` (state `[Wx, Wy, Wz, Bz, D]`)
//! through the gausshann saturation-pulse train, inter-pulse gaps, the MT
//! spoiler gradient, the water excitation, and the TR fill, spoiling the
//! transverse magnetisation to zero at each gradient. The FLASH signal is the
//! transverse magnitude at excitation, averaged over the last `n_avg` TRs.
//!
//! `vfa_apparent` runs the un-saturated FLASH pair through the Helms VFA
//! formulas for the apparent R1/amplitude, and `mtsat_sim` combines the
//! MT-weighted signal with those to give the Helms MTsat (percent).

use crate::mtsat_b1::mat5::{expm5, ident5, matvec5, solve5, sub5, Mat5, Vec5};
use crate::mtsat_b1::pulse::gausshann_omega;
use crate::mtsat_b1::rate::{bound_exc_sat, excitation_matrix, rate_matrix, PoolParams};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FreqPattern {
    Single,
    DualAlternate,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct SeqParams {
    pub num_sat_pulse: usize,
    pub pulse_dur: f64,
    pub pulse_gap_dur: f64,
    pub tr: f64,
    pub w_exc_dur: f64,
    pub freq_pattern: FreqPattern,
    pub delta: f64,
    pub flip_angle: f64,
    pub r: f64,
    pub t2a: f64,
    pub t1d: f64,
    pub m0a: f64,
    pub r1b: f64,
    pub t2b: f64,
    pub bw: f64,
    pub mt_grad_time: f64,
    pub n_avg: usize,
}

const DEFAULT_STEP: f64 = 50e-6;

/// FLASH signal (transverse magnitude at excitation, averaged over the last
/// `n_avg` TRs) for the given `(M0b, Raobs, flip_deg, b1_sat)`, using the
/// default 50 µs integration step. When `mtc` is false the saturation train is
/// skipped entirely (a plain FLASH readout).
pub fn flash_signal(
    p: &SeqParams,
    m0b: f64,
    raobs: f64,
    flip_deg: f64,
    b1_sat: f64,
    mtc: bool,
) -> f64 {
    flash_signal_with_step(p, m0b, raobs, flip_deg, b1_sat, mtc, DEFAULT_STEP)
}

/// FLASH signal with an explicit integration `step` (s). `flash_signal` is the
/// 50 µs entry point; this variant exists so the step size can be varied for
/// convergence checks.
#[allow(clippy::too_many_arguments)]
pub fn flash_signal_with_step(
    p: &SeqParams,
    m0b: f64,
    raobs: f64,
    flip_deg: f64,
    b1_sat: f64,
    mtc: bool,
    step: f64,
) -> f64 {
    // Ra from Raobs (MAMT_model_2007_5 lines 53-58).
    let ra = {
        let denom = p.r1b - raobs + p.r;
        let val = raobs - (p.r * m0b * (p.r1b - raobs)) / denom;
        if val.is_nan() {
            1.0
        } else {
            val
        }
    };
    let pool = PoolParams {
        ra,
        r1b: p.r1b,
        r: p.r,
        m0a: p.m0a,
        m0b,
        t2a: p.t2a,
        t2b: p.t2b,
        t1d: p.t1d,
    };
    let b: Vec5 = [0.0, 0.0, ra * p.m0a, p.r1b * m0b, 0.0];
    let ident = ident5();

    // Per-step saturation rate matrices for the two frequency offsets. Only
    // needed with saturation; skip the fill entirely for the VFA calls.
    let (a_sat, a_sat2): (Vec<Mat5>, Vec<Mat5>) = if mtc {
        let omega = gausshann_omega(b1_sat, p.pulse_dur, p.bw, step);
        (
            omega
                .iter()
                .map(|&w| rate_matrix(&pool, w, p.delta))
                .collect(),
            omega
                .iter()
                .map(|&w| rate_matrix(&pool, w, -p.delta))
                .collect(),
        )
    } else {
        (Vec::new(), Vec::new())
    };
    let pulse_steps = (p.pulse_dur / step).ceil() as usize;

    // Free-relaxation propagator matrix (w1 = 0, delta = 0), reused for gaps
    // and TR fill.
    let a_relax = rate_matrix(&pool, 0.0, 0.0);

    // Water excitation propagator (rotation + bound/dipolar saturation decay).
    let (rrfb_exc, rrfd_exc) = bound_exc_sat(flip_deg, p.w_exc_dur, p.t2b);
    let rexc = excitation_matrix(
        flip_deg * std::f64::consts::PI / 180.0,
        0.0,
        rrfb_exc,
        rrfd_exc,
        p.w_exc_dur,
    );

    // Propagate one interval of constant dynamics `A` for duration `t`:
    // M ← e^{A·t}·M + (e^{A·t} − I)·A⁻¹·B.
    let propagate = |m: &Vec5, a: &Mat5, t: f64| -> Vec5 {
        let mut scaled = *a;
        for row in &mut scaled {
            for x in row {
                *x *= t;
            }
        }
        let e = expm5(&scaled);
        let em = matvec5(&e, m);
        let ab = solve5(a, &b);
        let corr = matvec5(&sub5(&e, &ident), &ab);
        let mut out = [0.0; 5];
        for i in 0..5 {
            out[i] = em[i] + corr[i];
        }
        out
    };

    let loops = (6.0 / p.tr).ceil() as usize + p.n_avg;
    let mut m: Vec5 = [0.0, 0.0, p.m0a, m0b, 0.0];
    let mut acc = 0.0;
    for i in 1..=loops {
        if mtc {
            for j in 1..=p.num_sat_pulse {
                // dualAlternate flips the offset sign on even pulses.
                let mats = match p.freq_pattern {
                    FreqPattern::DualAlternate if j % 2 == 0 => &a_sat2,
                    _ => &a_sat,
                };
                for k in 0..pulse_steps {
                    let a = &mats[k.min(mats.len() - 1)];
                    m = propagate(&m, a, step);
                }
                // Inter-pulse gap, then perfect spoiling.
                m = propagate(&m, &a_relax, p.pulse_gap_dur);
                m[0] = 0.0;
                m[1] = 0.0;
            }
            // MT spoiler gradient, then perfect spoiling.
            m = propagate(&m, &a_relax, p.mt_grad_time);
            m[0] = 0.0;
            m[1] = 0.0;
        }
        // Water excitation and readout.
        m = matvec5(&rexc, &m);
        let sig = m[0].hypot(m[1]);
        // Relax a full echoSpacing = TR after excitation, then perfect
        // spoiling. The saturation train and MT spoiler gradient are additional
        // wall time before excitation; TR is not compressed and WExcDur is
        // never subtracted (it only drives bound-pool saturation inside the
        // excitation matrix).
        m = propagate(&m, &a_relax, p.tr);
        m[0] = 0.0;
        m[1] = 0.0;

        if i > loops - p.n_avg {
            acc += sig;
        }
    }
    acc / p.n_avg as f64
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct VfaParams {
    pub fa1_deg: f64,
    pub fa2_deg: f64,
    pub tr1: f64,
    pub tr2: f64,
}

/// Apparent R1 (1/s) and amplitude from the simulated un-saturated VFA pair,
/// via the Helms VFA formulas. The pair varies only TR and flip angle; there is
/// no saturation block (`mtc = false`, `b1_sat = 0`).
pub fn vfa_apparent(p: &SeqParams, vfa: &VfaParams, m0b: f64, raobs: f64) -> (f64, f64) {
    let mut vp1 = *p;
    vp1.tr = vfa.tr1;
    let lfa = flash_signal(&vp1, m0b, raobs, vfa.fa1_deg, 0.0, false);
    let mut vp2 = *p;
    vp2.tr = vfa.tr2;
    let hfa = flash_signal(&vp2, m0b, raobs, vfa.fa2_deg, 0.0, false);

    let a1 = vfa.fa1_deg * std::f64::consts::PI / 180.0;
    let a2 = vfa.fa2_deg * std::f64::consts::PI / 180.0;
    let r1 = 0.5 * (hfa * a2 / vfa.tr2 - lfa * a1 / vfa.tr1) / (lfa / a1 - hfa / a2);
    let aapp = lfa * hfa * (vfa.tr1 * a2 / a1 - vfa.tr2 * a1 / a2)
        / (hfa * vfa.tr1 * a2 - lfa * vfa.tr2 * a1);
    (r1, aapp)
}

/// Simulated MTsat (percent) for the given `(M0b, Raobs, b1_sat)`: the Helms
/// MTsat formula applied to the MT-weighted FLASH signal and the VFA apparent
/// values, using the nominal excitation flip.
pub fn mtsat_sim(p: &SeqParams, vfa: &VfaParams, m0b: f64, raobs: f64, b1_sat: f64) -> f64 {
    let (r1app, aapp) = vfa_apparent(p, vfa, m0b, raobs);
    let gre = flash_signal(p, m0b, raobs, p.flip_angle, b1_sat, true);
    let flip_rad = p.flip_angle * std::f64::consts::PI / 180.0;
    100.0 * ((aapp * flip_rad / gre - 1.0) * r1app * p.tr - flip_rad * flip_rad / 2.0)
}

#[cfg(test)]
pub fn tests_sample_params() -> SeqParams {
    SeqParams {
        num_sat_pulse: 2,
        pulse_dur: 0.768e-3,
        pulse_gap_dur: 0.6e-3,
        tr: 28e-3,
        w_exc_dur: 3e-3,
        freq_pattern: FreqPattern::DualAlternate,
        delta: 7000.0,
        flip_angle: 6.0,
        r: 26.0,
        t2a: 70e-3,
        t1d: 6e-3,
        m0a: 1.0,
        r1b: 1.0,
        t2b: 12e-6,
        bw: 0.3 / 0.768e-3,
        mt_grad_time: 0.0,
        n_avg: 20,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_params() -> SeqParams {
        tests_sample_params()
    }

    fn vfa() -> VfaParams {
        VfaParams {
            fa1_deg: 5.0,
            fa2_deg: 20.0,
            tr1: 30e-3,
            tr2: 30e-3,
        }
    }

    #[test]
    fn signal_is_positive_and_bounded() {
        let p = sample_params();
        let s = flash_signal(&p, 0.1, 1.0, p.flip_angle, 9.0, true);
        assert!(s > 0.0 && s < p.m0a, "signal {s}");
    }

    #[test]
    fn saturation_reduces_signal() {
        // Higher b1_sat drives more MT saturation and lowers the FLASH signal.
        // With b1_sat = 0 the gausshann omega is all zeros, so there is no
        // saturation.
        let p = sample_params();
        let no_sat = flash_signal(&p, 0.1, 1.0, p.flip_angle, 0.0, true);
        let with_sat = flash_signal(&p, 0.1, 1.0, p.flip_angle, 9.0, true);
        assert!(with_sat < no_sat, "{with_sat} !< {no_sat}");
    }

    #[test]
    fn step_size_halving_is_stable() {
        let p = sample_params();
        let a = flash_signal_with_step(&p, 0.1, 1.0, p.flip_angle, 9.0, true, 50e-6);
        let b = flash_signal_with_step(&p, 0.1, 1.0, p.flip_angle, 9.0, true, 25e-6);
        assert!((a - b).abs() / b < 0.01, "{a} vs {b}");
    }

    #[test]
    fn vfa_recovers_positive_r1_and_amplitude() {
        // Exchange with a bound pool that relaxes faster than free water
        // (R1b > Ra) pulls the VFA-apparent R1 above the observed rate, so
        // R1app sits between raobs and R1b, and Aapp ≈ M0a.
        let p = sample_params();
        let raobs = 1.0 / 1.2;
        let (r1, a) = vfa_apparent(&p, &vfa(), 0.1, raobs);
        assert!(
            r1 > raobs && r1 < p.r1b,
            "R1app {r1} (expected {raobs} < r1 < {})",
            p.r1b
        );
        assert!((a - p.m0a).abs() < 0.1, "Aapp {a} (expected ≈ {})", p.m0a);
    }

    #[test]
    fn mtsat_sim_increases_with_bound_pool() {
        let p = sample_params();
        let low = mtsat_sim(&p, &vfa(), 0.05, 1.0, 9.0);
        let high = mtsat_sim(&p, &vfa(), 0.15, 1.0, 9.0);
        assert!(high > low, "{high} !> {low}");
    }
}
