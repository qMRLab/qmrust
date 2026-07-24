//! Time-varying RF pulse shapes, ported from <ref>/functions/MAMT_preparePulses.m.
//! Each saturation shape is RMS-normalized so ∫p² dt = pulse_dur·b1² (trapz),
//! matching the reference. Amplitudes in µT, sampled every `step` seconds over
//! [0, dur] inclusive.

use std::f64::consts::PI;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SatShape {
    Hanning,
    Gaussian,
    Fermi,
    Square,
}

fn sample_times(dur: f64, step: f64) -> Vec<f64> {
    let n = (dur / step).round() as usize; // tSat = 0:step:dur
    (0..=n).map(|i| i as f64 * step).collect()
}

fn trapz(y: &[f64], step: f64) -> f64 {
    let mut s = 0.0;
    for i in 0..y.len() - 1 {
        s += 0.5 * (y[i] + y[i + 1]) * step;
    }
    s
}

/// RMS-normalize a window to the target `b1` (µT): amp = sqrt(dur·b1² / ∫w² dt).
fn rms_normalize(window: &[f64], b1: f64, dur: f64, step: f64) -> Vec<f64> {
    let sq: Vec<f64> = window.iter().map(|w| w * w).collect();
    let sat_rms = trapz(&sq, step);
    let square_rms = dur * b1 * b1;
    let amp = (square_rms / sat_rms).sqrt();
    window.iter().map(|w| w * amp).collect()
}

pub fn sat_pulse(shape: SatShape, b1: f64, pulse_dur: f64, step: f64) -> Vec<f64> {
    let t = sample_times(pulse_dur, step);
    if shape == SatShape::Square {
        return vec![b1; t.len()];
    }
    let window: Vec<f64> = match shape {
        SatShape::Hanning => t
            .iter()
            .map(|&ti| 0.5 * (1.0 - (2.0 * PI * ti / pulse_dur).cos()))
            .collect(),
        SatShape::Gaussian => t
            .iter()
            .map(|&ti| {
                let c = pulse_dur / 4.0;
                (-0.5 * (ti - pulse_dur / 2.0).powi(2) / (c * c)).exp()
            })
            .collect(),
        SatShape::Fermi => {
            let slope = pulse_dur / 33.81;
            let t0 = (pulse_dur - 13.81 * slope) / 2.0;
            t.iter()
                .map(|&ti| 1.0 / (1.0 + (((ti - pulse_dur / 2.0).abs() - t0) / slope).exp()))
                .collect()
        }
        SatShape::Square => unreachable!(),
    };
    rms_normalize(&window, b1, pulse_dur, step)
}

/// Sinc water-excitation pulse (µT), RMS-normalized to the nominal flip's mean
/// amplitude w_b1 = flip/(360·γ·dur). γ = 42.57748.
pub fn sinc_exc_pulse(flip_deg: f64, w_exc_dur: f64, step: f64) -> Vec<f64> {
    let t = sample_times(w_exc_dur, step);
    let n = t.len();
    // x = linspace(-pi, pi, n); sinc(x) = sin(pi x)/(pi x) (MATLAB sinc).
    let window: Vec<f64> = (0..n)
        .map(|i| {
            let x = -PI + 2.0 * PI * (i as f64) / ((n - 1) as f64);
            if x == 0.0 {
                1.0
            } else {
                (PI * x).sin() / (PI * x)
            }
        })
        .collect();
    let w_b1 = flip_deg / (360.0 * 42.57748 * w_exc_dur);
    rms_normalize(&window, w_b1, w_exc_dur, step)
}

/// Gyromagnetic ratio (MHz/T), used for the gausshann w1(t) normalization.
const GAMMA_MHZ_T: f64 = 42.577478518;

/// Gausshann-shaped saturation pulse w1(t) (rad/s), RMS-normalized so that
/// `sqrt(mean(omega^2)) / (2*pi*GAMMA_MHZ_T) == b1_rms` (µT).
///
/// shape(t) = exp(-(t - Trf/2)^2 / (2*sigma2)) * 0.5*(1 - cos(2*pi*t/Trf)),
/// with sigma2 = 2*ln(2) / (pi*bw)^2 and Trf = pulse_dur. Samples are taken
/// at t = 0:step:pulse_dur (inclusive).
pub fn gausshann_omega(b1_rms: f64, pulse_dur: f64, bw: f64, step: f64) -> Vec<f64> {
    let t = sample_times(pulse_dur, step);
    let sigma2 = 2.0 * std::f64::consts::LN_2 / (PI * bw).powi(2);
    let shape: Vec<f64> = t
        .iter()
        .map(|&ti| {
            let gauss = (-((ti - pulse_dur / 2.0).powi(2)) / (2.0 * sigma2)).exp();
            let hann = 0.5 * (1.0 - (2.0 * PI * ti / pulse_dur).cos());
            gauss * hann
        })
        .collect();
    let mean_sq: f64 = shape.iter().map(|&s| s * s).sum::<f64>() / shape.len() as f64;
    let scale = 2.0 * PI * GAMMA_MHZ_T * b1_rms / mean_sq.sqrt();
    shape.iter().map(|&s| s * scale).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gausshann_is_rms_normalized_and_peaks_mid_pulse() {
        let b1_rms = 2.0;
        let dur = 20e-3;
        let bw = 200.0;
        let step = 100e-6;
        let omega = gausshann_omega(b1_rms, dur, bw, step);

        let n = omega.len();
        let mean_sq: f64 = omega.iter().map(|&w| w * w).sum::<f64>() / n as f64;
        let time_rms = mean_sq.sqrt();
        let b1_rms_check = time_rms / (2.0 * PI * GAMMA_MHZ_T);
        assert!(
            (b1_rms_check - b1_rms).abs() / b1_rms < 1e-9,
            "expected b1_rms {b1_rms}, got {b1_rms_check}"
        );

        assert!(omega[0].abs() < 1e-6 * omega.iter().cloned().fold(0.0, f64::max));
        assert!(omega[n - 1].abs() < 1e-6 * omega.iter().cloned().fold(0.0, f64::max));

        let mid = n / 2;
        let max_idx = omega
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.abs().partial_cmp(&b.1.abs()).unwrap())
            .map(|(i, _)| i)
            .unwrap();
        assert!(
            (max_idx as isize - mid as isize).abs() <= 1,
            "expected peak near mid-pulse (index {mid}), got {max_idx}"
        );
    }

    fn ms(p: &[f64], dur: f64, step: f64) -> f64 {
        // trapezoidal mean of p^2 over [0,dur], matching MATLAB trapz/dur.
        let n = p.len();
        let mut integral = 0.0;
        for i in 0..n - 1 {
            integral += 0.5 * (p[i] * p[i] + p[i + 1] * p[i + 1]) * step;
        }
        integral / dur
    }

    #[test]
    fn hanning_is_rms_normalized_to_b1() {
        let b1 = 9.0;
        let dur = 0.768e-3;
        let step = 50e-6;
        let p = sat_pulse(SatShape::Hanning, b1, dur, step);
        // trapz(p^2) ≈ square_rms = dur*b1^2 by construction.
        let integral: f64 = {
            let mut s = 0.0;
            for i in 0..p.len() - 1 {
                s += 0.5 * (p[i] * p[i] + p[i + 1] * p[i + 1]) * step;
            }
            s
        };
        assert!((integral - dur * b1 * b1).abs() / (dur * b1 * b1) < 1e-9);
        let _ = ms(&p, dur, step);
    }

    #[test]
    fn square_is_flat_b1() {
        let p = sat_pulse(SatShape::Square, 5.0, 12e-3, 50e-6);
        assert!(p.iter().all(|&v| (v - 5.0).abs() < 1e-12));
    }

    #[test]
    fn sinc_excitation_has_expected_length() {
        let p = sinc_exc_pulse(9.0, 3e-3, 50e-6);
        assert_eq!(p.len(), (3e-3f64 / 50e-6).round() as usize + 1);
    }
}
