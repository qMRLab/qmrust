//! JSON result structs and terminal summaries for simulation runs.

use crate::core::model::VolumeId;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamStat {
    pub name: String,
    pub truth: f64,
    pub mean: f64,
    pub std: f64,
    pub bias: f64,
    pub rmse: f64,
}

// `VolumeId::Role` borrows a `'static str`, which a `Deserialize` impl cannot
// manufacture from arbitrary input, so a report carrying identities is
// write-only from Rust's side; the browser reads the JSON directly.
#[derive(Debug, Clone, Serialize)]
pub struct SignalReport {
    pub mode: String,
    pub model: String,
    pub params: Vec<(String, f64)>,
    pub signal: Vec<f64>,
    /// Each sample's identity along the acquisition axis, in the same order
    /// as `signal`, so a chart can label a point by what it is rather than
    /// where it sits.
    pub identities: Vec<VolumeId>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SingleVoxelReport {
    pub mode: String,
    pub model: String,
    pub truth: Vec<(String, f64)>,
    pub noisy_signal: Vec<f64>,
    /// The noise-free forward signal at the ground truth, and the forward
    /// signal at the first trial's fitted parameters. The three curves a
    /// recovery plot needs travel together, so no caller recomputes them.
    pub clean_signal: Vec<f64>,
    pub fitted_signal: Vec<f64>,
    /// Each sample's identity along the acquisition axis, in the same order
    /// as the three signal curves.
    pub identities: Vec<VolumeId>,
    pub trials: usize,
    pub fitted_names: Vec<String>,
    pub stats: Vec<ParamStat>,
    pub per_trial: Vec<Vec<f64>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SweepPoint {
    pub value: f64,
    pub stats: Vec<ParamStat>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensitivityReport {
    pub mode: String,
    pub model: String,
    pub swept_param: String,
    pub points: Vec<SweepPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonteCarloReport {
    pub mode: String,
    pub model: String,
    pub trials: usize,
    pub stats: Vec<ParamStat>,
    /// Each trial's error for each entry of `stats`, in the same order:
    /// `per_trial_error[trial][i]` belongs to `stats[i]`. A summary cannot
    /// describe the shape of a distribution, so the errors it was computed
    /// from travel with it.
    pub per_trial_error: Vec<Vec<f64>>,
}

/// Sample mean and (n-1) standard deviation. std=0 for n<2.
pub fn mean_std(xs: &[f64]) -> (f64, f64) {
    let n = xs.len();
    if n == 0 {
        return (0.0, 0.0);
    }
    let mean = xs.iter().sum::<f64>() / n as f64;
    if n < 2 {
        return (mean, 0.0);
    }
    let var = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n as f64 - 1.0);
    (mean, var.sqrt())
}

pub fn write_json<T: Serialize>(report: &T, path: &Path) -> Result<()> {
    let json = serde_json::to_string_pretty(report)?;
    std::fs::write(path, json)?;
    Ok(())
}

/// Print a per-parameter stats table to stdout.
pub fn print_stats(title: &str, stats: &[ParamStat]) {
    println!("{}", title);
    println!(
        "  {:>6}  {:>12}  {:>12}  {:>12}  {:>12}",
        "param", "truth", "mean", "bias", "std"
    );
    for s in stats {
        println!(
            "  {:>6}  {:>12.5}  {:>12.5}  {:>12.5}  {:>12.5}",
            s.name, s.truth, s.mean, s.bias, s.std
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mean_std_basic() {
        let (m, s) = mean_std(&[1.0, 2.0, 3.0]);
        assert!((m - 2.0).abs() < 1e-12);
        assert!((s - 1.0).abs() < 1e-12); // sample std (n-1)
    }

    #[test]
    fn mean_std_single() {
        let (m, s) = mean_std(&[5.0]);
        assert!((m - 5.0).abs() < 1e-12);
        assert_eq!(s, 0.0);
    }

    #[test]
    fn report_json_roundtrips() {
        let r = SignalReport {
            mode: "signal".into(),
            model: "qmt_spgr".into(),
            params: vec![("F".into(), 0.16)],
            signal: vec![0.9, 0.8],
            identities: vec![VolumeId::Role("T1w"), VolumeId::Role("PDw")],
        };
        let json = serde_json::to_string(&r).unwrap();
        let back: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(back["signal"], serde_json::json!([0.9, 0.8]));
        assert_eq!(back["mode"], "signal");
        // A role identity serializes untagged, as the plain string it names.
        assert_eq!(back["identities"], serde_json::json!(["T1w", "PDw"]));
    }

    #[test]
    fn a_params_identity_serializes_as_a_json_object_not_a_tagged_variant() {
        let mut params = std::collections::BTreeMap::new();
        params.insert("InversionTime".to_string(), 0.35);
        let id = VolumeId::Params(params);
        let json = serde_json::to_value(&id).unwrap();
        assert_eq!(json, serde_json::json!({ "InversionTime": 0.35 }));
    }
}
