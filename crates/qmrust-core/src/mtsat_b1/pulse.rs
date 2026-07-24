//! Time-varying RF pulse shape for the FLASH saturation train, ported from
//! <ref>/functions/MAMT_preparePulses.m. Amplitudes are returned as `w1(t)`
//! (rad/s), sampled every `step` seconds over [0, pulse_dur] inclusive.

use std::f64::consts::PI;

fn sample_times(dur: f64, step: f64) -> Vec<f64> {
    let n = (dur / step).round() as usize; // tSat = 0:step:dur
    (0..=n).map(|i| i as f64 * step).collect()
}

/// Gausshann-shaped saturation pulse w1(t) (rad/s), RMS-normalized so that
/// `sqrt(mean(omega^2)) / (2*pi*GAMMA) == b1_rms` (µT).
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
    let scale = 2.0 * PI * crate::mtsat_b1::GAMMA * b1_rms / mean_sq.sqrt();
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
        let b1_rms_check = time_rms / (2.0 * PI * crate::mtsat_b1::GAMMA);
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
}
