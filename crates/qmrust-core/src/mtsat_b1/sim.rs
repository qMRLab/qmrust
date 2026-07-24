//! Steady-state 3-pool MT-SPGR signal, ported from
//! <ref>/functions/MAMT_model_2007_5.m. Time-steps `dM/dt = A·M + B` with the
//! exact update `M ← e^{A·t}·M + (e^{A·t} − I)·A⁻¹·B` through the saturation
//! pulse train, inter-pulse gaps, the sinc water excitation, and TR-fill, until
//! the post-saturation Mza settles (<0.05% change). Returns `Mza·sin(flip)`.

use crate::mtsat_b1::lineshape::Lineshape;
use crate::mtsat_b1::mat3::{expm3, ident3, matvec3, solve3, sub3, Mat3, Vec3};
use crate::mtsat_b1::pulse::{sat_pulse, sinc_exc_pulse, SatShape};
use crate::mtsat_b1::rate::{rate_matrix, PoolParams};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FreqPattern {
    Single,
    DualAlternate,
    DualContinuous,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct SeqParams {
    pub num_sat_pulse: usize,
    pub pulse_dur: f64,
    pub pulse_gap_dur: f64,
    pub tr: f64,
    pub w_exc_dur: f64,
    pub num_excitation: usize,
    pub freq_pattern: FreqPattern,
    pub delta: f64,
    pub flip_angle: f64,
    pub sat_shape: SatShape,
    pub r: f64,
    pub t2a: f64,
    pub t1d: f64,
    pub lineshape: Lineshape,
    pub m0a: f64,
    pub rb: f64,
    pub t2b: f64,
}

const GAMMA: f64 = 42.57747892; // MHz/T = Hz/µT
const SS_TIME: f64 = 5.0; // seconds to steady state
const SS_THRESHOLD: f64 = 0.05; // percent change in Mza

pub fn mamt_signal(p: &SeqParams, m0b: f64, raobs: f64, flip_deg: f64, b1_sat: f64) -> f64 {
    mamt_signal_with_step(p, m0b, raobs, flip_deg, b1_sat, 50e-6)
}

pub fn mamt_signal_with_step(
    p: &SeqParams,
    m0b: f64,
    raobs: f64,
    flip_deg: f64,
    b1_sat: f64,
    step: f64,
) -> f64 {
    // Ra from Raobs (MAMT_model_2007_5 lines 53-58).
    let ra = {
        let denom = p.rb - raobs + p.r;
        let val = raobs - (p.r * m0b * (p.rb - raobs)) / denom;
        if val.is_nan() {
            1.0
        } else {
            val
        }
    };
    let pool = PoolParams {
        ra,
        rb: p.rb,
        r: p.r,
        m0a: p.m0a,
        m0b,
        t2a: p.t2a,
        t2b: p.t2b,
        t1d: p.t1d,
        lineshape: p.lineshape,
    };
    let b: Vec3 = [ra * p.m0a, p.rb * m0b, 0.0];
    let ident = ident3();
    let dual_cont = p.freq_pattern == FreqPattern::DualContinuous;

    // Precompute the discretized pulses.
    let sat = sat_pulse(p.sat_shape, b1_sat, p.pulse_dur, step);
    let exc = sinc_exc_pulse(flip_deg, p.w_exc_dur, step);
    let pulse_steps = (p.pulse_dur / step).ceil() as usize;
    let exc_steps = (p.w_exc_dur / step).ceil() as usize;
    let tr_fill = p.tr
        - (p.num_sat_pulse as f64) * (p.pulse_dur + p.pulse_gap_dur)
        - (p.num_excitation as f64) * p.w_exc_dur;

    let mut loops = (SS_TIME / p.tr).ceil() as usize;
    if loops < 50 {
        loops *= 10;
    }

    // Propagate one interval of constant (w1, delta) for duration `t`.
    let propagate = |m: &Vec3, w1: f64, delta: f64, t: f64| -> Vec3 {
        let a: Mat3 = rate_matrix(&pool, w1, delta, dual_cont);
        let scaled = [
            [a[0][0] * t, a[0][1] * t, a[0][2] * t],
            [a[1][0] * t, a[1][1] * t, a[1][2] * t],
            [a[2][0] * t, a[2][1] * t, a[2][2] * t],
        ];
        let e = expm3(&scaled);
        // e·m + (e − I)·A⁻¹·b
        let ab = solve3(&a, &b);
        let em = matvec3(&e, m);
        let e_minus_i = sub3(&e, &ident);
        let corr = matvec3(&e_minus_i, &ab);
        [em[0] + corr[0], em[1] + corr[1], em[2] + corr[2]]
    };

    let mut m: Vec3 = [p.m0a, m0b, 0.0];
    let mut prev = 0.0;
    for i in 1..=loops {
        for j in 1..=p.num_sat_pulse {
            // Saturation pulse: time-varying w1.
            let delta = match p.freq_pattern {
                FreqPattern::DualAlternate if j % 2 == 0 => -p.delta,
                _ => p.delta,
            };
            for k in 0..pulse_steps {
                let w1 = 2.0 * std::f64::consts::PI * sat[k.min(sat.len() - 1)] * GAMMA;
                m = propagate(&m, w1, delta, step);
            }
            // Inter-pulse gap (relaxation).
            m = propagate(&m, 0.0, 0.0, p.pulse_gap_dur);
            if j == p.num_sat_pulse {
                let check = m[0];
                let diff = (check - prev).abs() * 100.0;
                if i >= 3 && (diff < SS_THRESHOLD || i == loops) {
                    return check * (p.flip_angle * std::f64::consts::PI / 180.0).sin();
                }
                prev = check;
            }
        }
        // Water excitation(s).
        for _ in 0..p.num_excitation {
            for k in 0..exc_steps {
                let w1 = exc[k.min(exc.len() - 1)] * GAMMA;
                m = propagate(&m, w1, 0.0, step);
            }
        }
        // TR fill.
        m = propagate(&m, 0.0, 0.0, tr_fill);
    }
    m[0] * (p.flip_angle * std::f64::consts::PI / 180.0).sin()
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct VfaParams {
    pub fa1_deg: f64,
    pub fa2_deg: f64,
    pub tr1: f64,
    pub tr2: f64,
}

/// Apparent R1 (1/s) and amplitude from the simulated VFA pair (b1_sat = 0),
/// via the Helms VFA formulas. Ports MAMT_model_simVFA.m. The VFA sim uses its
/// own short pulse/gap timing (pulse_dur = 1 ms, gap = 0.3 ms, 1 sat pulse).
pub fn vfa_apparent(p: &SeqParams, vfa: &VfaParams, m0b: f64, raobs: f64) -> (f64, f64) {
    let mut vp = *p;
    vp.num_sat_pulse = 1;
    vp.pulse_dur = 1e-3;
    vp.pulse_gap_dur = 0.3e-3;
    vp.num_excitation = 1;
    vp.w_exc_dur = 3e-3;

    let mut vp1 = vp;
    vp1.tr = vfa.tr1;
    let lfa = mamt_signal(&vp1, m0b, raobs, vfa.fa1_deg, 0.0);
    let mut vp2 = vp;
    vp2.tr = vfa.tr2;
    let hfa = mamt_signal(&vp2, m0b, raobs, vfa.fa2_deg, 0.0);

    let a1 = vfa.fa1_deg * std::f64::consts::PI / 180.0;
    let a2 = vfa.fa2_deg * std::f64::consts::PI / 180.0;
    let r1 = 0.5 * (hfa * a2 / vfa.tr2 - lfa * a1 / vfa.tr1) / (lfa / a1 - hfa / a2);
    let aapp = lfa * hfa * (vfa.tr1 * a2 / a1 - vfa.tr2 * a1 / a2)
        / (hfa * vfa.tr1 * a2 - lfa * vfa.tr2 * a1);
    (r1, aapp)
}

/// Simulated MTsat (percent) for the given (M0b, Raobs, b1_sat): the MTsat
/// formula applied to the simulated MT-weighted signal and the VFA apparent
/// values, using the NOMINAL excitation flip (simSeq_M0b_R1obs.m line 100).
pub fn mtsat_sim(p: &SeqParams, vfa: &VfaParams, m0b: f64, raobs: f64, b1_sat: f64) -> f64 {
    let (r1app, aapp) = vfa_apparent(p, vfa, m0b, raobs);
    let gre = mamt_signal(p, m0b, raobs, p.flip_angle, b1_sat);
    let flip_rad = p.flip_angle * std::f64::consts::PI / 180.0;
    100.0 * ((aapp * flip_rad / gre - 1.0) * r1app * p.tr - flip_rad * flip_rad / 2.0)
}

#[cfg(test)]
pub fn tests_sample_params() -> SeqParams {
    use crate::mtsat_b1::lineshape::Lineshape;
    use crate::mtsat_b1::pulse::SatShape;
    SeqParams {
        num_sat_pulse: 2,
        pulse_dur: 0.768e-3,
        pulse_gap_dur: 0.6e-3,
        tr: 28e-3,
        w_exc_dur: 3e-3,
        num_excitation: 1,
        freq_pattern: FreqPattern::DualAlternate,
        delta: 7000.0,
        flip_angle: 9.0,
        sat_shape: SatShape::Hanning,
        r: 26.0,
        t2a: 70e-3,
        t1d: 6e-3,
        lineshape: Lineshape::SuperLorentzian,
        m0a: 1.0,
        rb: 1.0,
        t2b: 12e-6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_params() -> SeqParams {
        tests_sample_params()
    }

    #[test]
    fn signal_is_positive_and_bounded() {
        let s = mamt_signal(&sample_params(), 0.1, 1.0, 9.0, 9.0);
        assert!(s > 0.0 && s < 1.0, "signal {s}");
    }

    #[test]
    fn saturation_reduces_signal() {
        // More MT saturation (higher b1_sat) lowers the steady-state Mza.
        let p = sample_params();
        let no_sat = mamt_signal(&p, 0.1, 1.0, 9.0, 0.0);
        let with_sat = mamt_signal(&p, 0.1, 1.0, 9.0, 9.0);
        assert!(with_sat < no_sat, "{with_sat} !< {no_sat}");
    }

    #[test]
    fn step_size_halving_is_stable() {
        // The 50 µs default result should be close to a 25 µs re-run (<1%).
        let p = sample_params();
        let a = mamt_signal_with_step(&p, 0.1, 1.0, 9.0, 9.0, 50e-6);
        let b = mamt_signal_with_step(&p, 0.1, 1.0, 9.0, 9.0, 25e-6);
        assert!((a - b).abs() / b < 0.01, "{a} vs {b}");
    }

    #[test]
    fn vfa_recovers_positive_r1_and_amplitude() {
        let p = sample_params();
        let vfa = VfaParams {
            fa1_deg: 5.0,
            fa2_deg: 20.0,
            tr1: 30e-3,
            tr2: 30e-3,
        };
        let (r1, a) = vfa_apparent(&p, &vfa, 0.1, 1.0);
        assert!(r1 > 0.0 && r1 < 5.0, "R1app {r1}");
        assert!(a > 0.0, "Aapp {a}");
    }

    #[test]
    fn mtsat_sim_increases_with_bound_pool() {
        let p = sample_params();
        let vfa = VfaParams {
            fa1_deg: 5.0,
            fa2_deg: 20.0,
            tr1: 30e-3,
            tr2: 30e-3,
        };
        let low = mtsat_sim(&p, &vfa, 0.05, 1.0, 9.0);
        let high = mtsat_sim(&p, &vfa, 0.15, 1.0, 9.0);
        assert!(high > low, "{high} !> {low}");
    }
}
