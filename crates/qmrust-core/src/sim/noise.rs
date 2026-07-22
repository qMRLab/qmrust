//! Seeded measurement noise for simulation: Gaussian and Rician, SNR-defined.

use anyhow::{bail, Result};
use rand::rngs::StdRng;
use rand::SeedableRng;
use rand_distr::{Distribution, Normal};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NoiseKind {
    None,
    Gaussian,
    Rician,
}

impl NoiseKind {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Result<NoiseKind> {
        Ok(match s {
            "none" => NoiseKind::None,
            "gaussian" => NoiseKind::Gaussian,
            "rician" => NoiseKind::Rician,
            other => bail!("unknown noise type '{}'", other),
        })
    }
}

/// Deterministic RNG seeded from `seed`.
pub fn seeded_rng(seed: u64) -> StdRng {
    StdRng::seed_from_u64(seed)
}

/// qMRLab-style noise scale: sigma = max(|signal|) / SNR.
pub fn sigma_for(signal: &[f64], snr: f64) -> f64 {
    let max_abs = signal.iter().fold(0.0_f64, |m, &v| m.max(v.abs()));
    max_abs / snr
}

/// Add measurement noise. `None` returns the signal unchanged.
pub fn add_noise(signal: &[f64], kind: NoiseKind, sigma: f64, rng: &mut StdRng) -> Vec<f64> {
    if kind == NoiseKind::None || sigma == 0.0 {
        return signal.to_vec();
    }
    let normal = Normal::new(0.0, sigma).expect("sigma >= 0");
    signal
        .iter()
        .map(|&s| match kind {
            NoiseKind::Gaussian => s + normal.sample(rng),
            NoiseKind::Rician => {
                let n1 = normal.sample(rng);
                let n2 = normal.sample(rng);
                ((s + n1).powi(2) + n2 * n2).sqrt()
            }
            NoiseKind::None => s,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sigma_is_max_abs_over_snr() {
        let s = [0.2, -0.9, 0.5];
        let sigma = sigma_for(&s, 100.0);
        assert!((sigma - 0.9 / 100.0).abs() < 1e-12);
    }

    #[test]
    fn none_returns_signal_unchanged() {
        let s = vec![0.2, 0.5, 0.9];
        let mut rng = seeded_rng(0);
        let out = add_noise(&s, NoiseKind::None, 0.1, &mut rng);
        assert_eq!(out, s);
    }

    #[test]
    fn same_seed_same_output() {
        let s = vec![0.2, 0.5, 0.9];
        let a = add_noise(&s, NoiseKind::Gaussian, 0.05, &mut seeded_rng(42));
        let b = add_noise(&s, NoiseKind::Gaussian, 0.05, &mut seeded_rng(42));
        assert_eq!(a, b);
    }

    #[test]
    fn rician_floor_raises_zero_signal_mean() {
        // At zero signal, Rician mean ≈ sigma*sqrt(pi/2) > 0; Gaussian mean ≈ 0.
        let s = vec![0.0; 20000];
        let sigma = 0.1;
        let ric = add_noise(&s, NoiseKind::Rician, sigma, &mut seeded_rng(1));
        let gau = add_noise(&s, NoiseKind::Gaussian, sigma, &mut seeded_rng(1));
        let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
        assert!(
            mean(&ric) > 0.5 * sigma,
            "rician mean too low: {}",
            mean(&ric)
        );
        assert!(
            mean(&gau).abs() < 0.1 * sigma,
            "gaussian mean not ~0: {}",
            mean(&gau)
        );
    }

    #[test]
    fn kind_from_str() {
        assert!(matches!(
            NoiseKind::from_str("rician").unwrap(),
            NoiseKind::Rician
        ));
        assert!(NoiseKind::from_str("bogus").is_err());
    }
}
