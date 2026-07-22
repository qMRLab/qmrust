//! Simulation: forward signal generation, noise, and sim→fit round-trips
//! mirroring qMRLab's Sim_* family.

pub mod model;
pub mod noise;
pub mod plot;
pub mod report;

use std::path::PathBuf;

use anyhow::{bail, Result};
use rand_distr::{Distribution, Normal};

use crate::config::Config;
use crate::core::model::{Measurement, MeasurementKind, Model, Sample};
use model::{build_model, param_vector, sim_aux};
use noise::{add_noise, seeded_rng, sigma_for, NoiseKind};
use rand::rngs::StdRng;
use report::{
    mean_std, MonteCarloReport, ParamStat, SensitivityReport, SignalReport, SingleVoxelReport,
    SweepPoint,
};

/// Map a ground-truth parameter name to the index of its estimate in the
/// model's fitted output. Returns None if the fitter doesn't estimate it.
fn fitted_index(model: &dyn Model, param: &str) -> Option<usize> {
    model.output_names().iter().position(|n| n == param)
}

/// Per-parameter statistics comparing fitted estimates against a truth vector.
fn compute_stats(model: &dyn Model, truth: &[f64], per_trial: &[Vec<f64>]) -> Vec<ParamStat> {
    let names = model.param_names();
    let mut out = Vec::new();
    for (pi, name) in names.iter().enumerate() {
        let Some(fi) = fitted_index(model, name) else {
            continue;
        };
        let ests: Vec<f64> = per_trial.iter().map(|t| t[fi]).collect();
        let (mean, std) = mean_std(&ests);
        let truth_v = truth[pi];
        let rmse = if ests.is_empty() {
            0.0
        } else {
            (ests.iter().map(|e| (e - truth_v).powi(2)).sum::<f64>() / ests.len() as f64).sqrt()
        };
        out.push(ParamStat {
            name: name.to_string(),
            truth: truth_v,
            mean,
            std,
            bias: mean - truth_v,
            rmse,
        });
    }
    out
}

/// A `Named` measurement's map has no inherent order; the model's declared
/// `roles` (from `measurement()`) is the only canonical order. Panics if `m`
/// is `Named` for a model whose `measurement()` isn't `Named` — callers only
/// ever pass matching pairs (the model that produced/consumes `m`).
fn named_roles(model: &dyn Model) -> &'static [&'static str] {
    match model.measurement() {
        MeasurementKind::Named { roles } => roles,
        MeasurementKind::Series { .. } => {
            unreachable!("named_roles called for a Series-measurement model")
        }
    }
}

/// Extract a measurement's values in a defined order (the reports and plots
/// stay value vectors): `Series` → sample order; `Named` → the model's
/// declared role order (never the map's own, alphabetical, iteration order).
fn measurement_values(model: &dyn Model, m: &Measurement) -> Vec<f64> {
    match m {
        Measurement::Named(map) => named_roles(model).iter().map(|r| map[r]).collect(),
        Measurement::Series(s) => s.iter().map(|s| s.value).collect(),
    }
}

/// Apply `add_noise` to a measurement's values, preserving its shape and
/// identities. Noise is drawn in `measurement_values` order, so the RNG draw
/// sequence is identical to noising the extracted value vector directly, and
/// the `Named` reconstruction below re-zips with that same declared-role
/// order (not the map's alphabetical iteration order).
fn add_noise_measurement(
    model: &dyn Model,
    m: &Measurement,
    kind: NoiseKind,
    sigma: f64,
    rng: &mut StdRng,
) -> Measurement {
    let noised = add_noise(&measurement_values(model, m), kind, sigma, rng);
    match m {
        Measurement::Named(_) => {
            Measurement::Named(named_roles(model).iter().copied().zip(noised).collect())
        }
        Measurement::Series(s) => Measurement::Series(
            s.iter()
                .zip(noised)
                .map(|(s, value)| Sample {
                    params: s.params.clone(),
                    value,
                })
                .collect(),
        ),
    }
}

/// Clamp a value into a (lb, ub) range.
fn clamp_to_bounds(val: f64, bound: (f64, f64)) -> f64 {
    let (lo, hi) = bound;
    val.max(lo).min(hi)
}

pub fn run_signal(cfg: &Config, raw: &serde_yaml::Value) -> Result<SignalReport> {
    let sim = cfg
        .sim
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("no sim block"))?;
    let model = build_model(cfg, raw)?;
    let aux = sim_aux(sim);
    let truth = param_vector(model.as_ref(), sim)?;
    let signal = measurement_values(model.as_ref(), &model.forward(&truth, &aux));
    let names = model.param_names();
    Ok(SignalReport {
        mode: "signal".into(),
        model: cfg.model.clone(),
        params: names
            .iter()
            .map(|s| s.to_string())
            .zip(truth.iter().copied())
            .collect(),
        signal,
    })
}

pub fn run_single_voxel(cfg: &Config, raw: &serde_yaml::Value) -> Result<SingleVoxelReport> {
    let sim = cfg
        .sim
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("no sim block"))?;
    let model = build_model(cfg, raw)?;
    let aux = sim_aux(sim);
    let truth = param_vector(model.as_ref(), sim)?;
    let clean_meas = model.forward(&truth, &aux);
    let clean = measurement_values(model.as_ref(), &clean_meas);
    let kind = NoiseKind::from_str(&sim.noise.kind)?;
    let sigma = sigma_for(&clean, sim.noise.snr);
    let mut rng = seeded_rng(sim.seed);

    let mut per_trial = Vec::with_capacity(sim.trials);
    let mut first_noisy = clean.clone();
    for t in 0..sim.trials {
        let noisy = add_noise_measurement(model.as_ref(), &clean_meas, kind, sigma, &mut rng);
        if t == 0 {
            first_noisy = measurement_values(model.as_ref(), &noisy);
        }
        per_trial.push(model.fit(&noisy, &aux));
    }
    let stats = compute_stats(model.as_ref(), &truth, &per_trial);
    let names = model.param_names();
    Ok(SingleVoxelReport {
        mode: "single-voxel".into(),
        model: cfg.model.clone(),
        truth: names
            .iter()
            .map(|s| s.to_string())
            .zip(truth.iter().copied())
            .collect(),
        noisy_signal: first_noisy,
        trials: sim.trials,
        fitted_names: model.output_names(),
        stats,
        per_trial,
    })
}

pub fn run_sensitivity(cfg: &Config, raw: &serde_yaml::Value) -> Result<SensitivityReport> {
    let sim = cfg
        .sim
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("no sim block"))?;
    let sweep = sim
        .sweep
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("sensitivity requires sim.sweep"))?;
    let model = build_model(cfg, raw)?;
    let aux = sim_aux(sim);
    let base = param_vector(model.as_ref(), sim)?;
    let names = model.param_names();
    let pi = names
        .iter()
        .position(|n| *n == sweep.param)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "sweep.param '{}' is not a parameter of this model",
                sweep.param
            )
        })?;
    if model.fixed_mask()[pi] {
        eprintln!(
            "warning: swept parameter '{}' is fixed in this fit configuration; \
             its bias/std reflect the fixed value, not a recovered estimate",
            sweep.param
        );
    }
    if sweep.steps < 2 {
        bail!("sweep.steps must be >= 2");
    }
    let kind = NoiseKind::from_str(&sim.noise.kind)?;
    let mut rng = seeded_rng(sim.seed);

    let mut points = Vec::with_capacity(sweep.steps);
    let step = (sweep.stop - sweep.start) / (sweep.steps as f64 - 1.0);
    for k in 0..sweep.steps {
        let value = sweep.start + step * k as f64;
        let mut truth = base.clone();
        truth[pi] = value;
        let clean_meas = model.forward(&truth, &aux);
        let sigma = sigma_for(
            &measurement_values(model.as_ref(), &clean_meas),
            sim.noise.snr,
        );
        let mut per_trial = Vec::with_capacity(sim.trials);
        for _ in 0..sim.trials {
            let noisy = add_noise_measurement(model.as_ref(), &clean_meas, kind, sigma, &mut rng);
            per_trial.push(model.fit(&noisy, &aux));
        }
        points.push(SweepPoint {
            value,
            stats: compute_stats(model.as_ref(), &truth, &per_trial),
        });
    }
    Ok(SensitivityReport {
        mode: "sensitivity".into(),
        model: cfg.model.clone(),
        swept_param: sweep.param.clone(),
        points,
    })
}

pub fn run_montecarlo(cfg: &Config, raw: &serde_yaml::Value) -> Result<MonteCarloReport> {
    let sim = cfg
        .sim
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("no sim block"))?;
    let dists = sim
        .distributions
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("montecarlo requires sim.distributions"))?;
    let model = build_model(cfg, raw)?;
    let aux = sim_aux(sim);
    let base = param_vector(model.as_ref(), sim)?;
    let names = model.param_names();
    let kind = NoiseKind::from_str(&sim.noise.kind)?;
    let mut rng = seeded_rng(sim.seed);

    // Pre-resolve which params are drawn and their normal distributions.
    let mut drawn: Vec<(usize, Normal<f64>)> = Vec::new();
    for (name, d) in dists {
        let pi = names
            .iter()
            .position(|n| n == name)
            .ok_or_else(|| anyhow::anyhow!("distributions key '{}' is not a parameter", name))?;
        drawn.push((
            pi,
            Normal::new(d.mean, d.std).map_err(|e| anyhow::anyhow!("{}", e))?,
        ));
    }

    let fixed = model.fixed_mask();
    for (pi, _) in &drawn {
        if fixed[*pi] {
            eprintln!(
                "warning: drawn parameter '{}' is fixed in this fit configuration; \
                 its bias/std reflect the fixed value, not a recovered estimate",
                names[*pi]
            );
        }
    }

    let bounds = model.param_bounds();
    let mut per_trial_fit = Vec::with_capacity(sim.trials);
    let mut per_trial_truth = Vec::with_capacity(sim.trials);
    for _ in 0..sim.trials {
        let mut truth = base.clone();
        for (pi, dist) in &drawn {
            truth[*pi] = clamp_to_bounds(dist.sample(&mut rng), bounds[*pi]);
        }
        let clean_meas = model.forward(&truth, &aux);
        let sigma = sigma_for(
            &measurement_values(model.as_ref(), &clean_meas),
            sim.noise.snr,
        );
        let noisy = add_noise_measurement(model.as_ref(), &clean_meas, kind, sigma, &mut rng);
        per_trial_fit.push(model.fit(&noisy, &aux));
        per_trial_truth.push(truth);
    }

    // Stats: bias/rmse against each voxel's own drawn truth (averaged).
    let mut stats = Vec::new();
    for (pi, name) in names.iter().enumerate() {
        let Some(fi) = model.output_names().iter().position(|n| n == name) else {
            continue;
        };
        let errs: Vec<f64> = (0..sim.trials)
            .map(|t| per_trial_fit[t][fi] - per_trial_truth[t][pi])
            .collect();
        let ests: Vec<f64> = per_trial_fit.iter().map(|t| t[fi]).collect();
        let (mean, std) = mean_std(&ests);
        let (bias, _) = mean_std(&errs);
        let rmse = (errs.iter().map(|e| e * e).sum::<f64>() / errs.len().max(1) as f64).sqrt();
        let truth_mean = mean_std(&per_trial_truth.iter().map(|t| t[pi]).collect::<Vec<_>>()).0;
        stats.push(ParamStat {
            name: name.to_string(),
            truth: truth_mean,
            mean,
            std,
            bias,
            rmse,
        });
    }
    Ok(MonteCarloReport {
        mode: "montecarlo".into(),
        model: cfg.model.clone(),
        trials: sim.trials,
        stats,
    })
}

/// Entry point for the `qmrust sim <mode>` command.
pub fn run_sim(mode: &str, config: PathBuf, output: PathBuf, plot: Option<PathBuf>) -> Result<()> {
    let contents = std::fs::read_to_string(&config)
        .map_err(|e| anyhow::anyhow!("Failed to read {:?}: {}", config, e))?;
    let (cfg, raw) = crate::config::parse_config(&contents)?;
    let sim = cfg
        .sim
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("config has no 'sim:' block (required for sim)"))?;
    sim.validate(&cfg.model, &raw)?;

    match mode {
        "signal" => {
            let r = run_signal(&cfg, &raw)?;
            eprintln!("signal ({} volumes)", r.signal.len());
            report::write_json(&r, &output)?;
            if plot.is_some() {
                eprintln!("warning: --plot has no effect for 'signal' mode");
            }
        }
        "single-voxel" => {
            let r = run_single_voxel(&cfg, &raw)?;
            report::print_stats(&format!("single-voxel ({} trials)", r.trials), &r.stats);
            report::write_json(&r, &output)?;
            if let Some(p) = plot {
                // Rebuild clean + fitted curves for the plot.
                let model = build_model(&cfg, &raw)?;
                let aux = sim_aux(sim);
                let truth = param_vector(model.as_ref(), sim)?;
                let clean = measurement_values(model.as_ref(), &model.forward(&truth, &aux));
                // fitted-curve = forward of the first trial's fitted params, mapped by name.
                let fitted_params = fitted_to_param_vec(model.as_ref(), &r.per_trial[0]);
                let fitted_curve =
                    measurement_values(model.as_ref(), &model.forward(&fitted_params, &aux));
                plot::plot_single_voxel(&clean, &r.noisy_signal, &fitted_curve, &p)?;
                eprintln!("wrote plot {:?}", p);
            }
        }
        "sensitivity" => {
            let r = run_sensitivity(&cfg, &raw)?;
            eprintln!(
                "sensitivity: {} sweep points over '{}'",
                r.points.len(),
                r.swept_param
            );
            report::write_json(&r, &output)?;
            if let Some(p) = plot {
                plot::plot_sensitivity(&r, &p)?;
                eprintln!("wrote plot {:?}", p);
            }
        }
        "montecarlo" => {
            let r = run_montecarlo(&cfg, &raw)?;
            report::print_stats(&format!("montecarlo ({} trials)", r.trials), &r.stats);
            report::write_json(&r, &output)?;
            if plot.is_some() {
                eprintln!("warning: --plot has no effect for 'montecarlo' mode");
            }
        }
        other => bail!("unknown sim mode '{}'", other),
    }
    eprintln!("wrote {:?}", output);
    Ok(())
}

/// Map a fitter's output vector back to a param-order vector, filling any
/// param the fitter doesn't estimate from... the fitted value if present,
/// else leaving 0.0. Used only to draw the fitted curve.
fn fitted_to_param_vec(model: &dyn Model, fitted: &[f64]) -> Vec<f64> {
    let names = model.param_names();
    let fnames = model.output_names();
    names
        .iter()
        .map(|name| {
            fnames
                .iter()
                .position(|n| n == name)
                .map(|i| fitted[i])
                .unwrap_or(0.0)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn qmt_cfg(extra: &str) -> (Config, serde_yaml::Value) {
        let yaml = format!(
            r#"
model: qmt_spgr
qmt_spgr:
  fitting:
    use_r1map_to_constrain_r1f: false
sim:
  params: {{ F: 0.15, kr: 25.0, R1f: 1.0, R1r: 1.0, T2f: 0.028, T2r: 1.1e-5 }}
{}
"#,
            extra
        );
        let raw: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
        let mut c: Config = serde_yaml::from_str(&yaml).unwrap();
        c.validate().unwrap();
        (c, raw)
    }

    #[test]
    fn fixed_params_reflects_fx_mask() {
        // qmt_cfg only sets use_r1map_to_constrain_r1f: false and does not
        // override fitting.fx, so the default fx = [F,kr,R1f,R1r,T2f,T2r]
        // = [false,false,true,true,false,false] applies (R1f, R1r fixed by
        // default). Confirm the model surfaces exactly that mask.
        let (cfg, raw) = qmt_cfg("");
        let model = crate::sim::model::build_model(&cfg, &raw).unwrap();
        let fixed = model.fixed_mask();
        assert_eq!(fixed.len(), 6);
        assert_eq!(
            fixed,
            vec![false, false, true, true, false, false],
            "got {:?}",
            fixed
        );
    }

    #[test]
    fn signal_mode_produces_protocol_length() {
        let (cfg, raw) = qmt_cfg("");
        let r = run_signal(&cfg, &raw).unwrap();
        let q: crate::config::QmtSpgrConfig = match raw.get("qmt_spgr") {
            Some(sub) => serde_yaml::from_value(sub.clone()).unwrap(),
            None => Default::default(),
        };
        assert_eq!(r.signal.len(), q.protocol.mtdata.len());
        assert_eq!(r.params.len(), 6);
    }

    #[test]
    fn single_voxel_noisefree_recovers_truth() {
        let (cfg, raw) = qmt_cfg("  noise: { type: none }\n  trials: 1\n");
        let r = run_single_voxel(&cfg, &raw).unwrap();
        let f = r.stats.iter().find(|s| s.name == "F").unwrap();
        assert!((f.mean - 0.15).abs() < 0.03, "F mean: {}", f.mean);
    }

    #[test]
    fn sensitivity_produces_points() {
        let (cfg, raw) =
            qmt_cfg("  trials: 2\n  sweep: { param: F, start: 0.1, stop: 0.2, steps: 3 }\n");
        let r = run_sensitivity(&cfg, &raw).unwrap();
        assert_eq!(r.points.len(), 3);
        assert_eq!(r.swept_param, "F");
    }

    #[test]
    fn montecarlo_produces_stats() {
        let (cfg, raw) =
            qmt_cfg("  trials: 5\n  distributions:\n    F: { mean: 0.15, std: 0.01 }\n");
        let r = run_montecarlo(&cfg, &raw).unwrap();
        assert_eq!(r.trials, 5);
        assert!(!r.stats.is_empty());
    }

    #[test]
    fn clamp_to_bounds_clamps_and_passes() {
        assert_eq!(clamp_to_bounds(5.0, (0.0, 1.0)), 1.0);
        assert_eq!(clamp_to_bounds(-2.0, (0.0, 1.0)), 0.0);
        assert_eq!(clamp_to_bounds(0.5, (0.0, 1.0)), 0.5);
        assert_eq!(
            clamp_to_bounds(42.0, (f64::NEG_INFINITY, f64::INFINITY)),
            42.0
        );
    }

    /// Minimal `Named` model stub that exercises `measurement_values`' ordering.
    /// Roles are chosen so their declared order differs from alphabetical
    /// (BTreeMap iteration) order, so a regression back to map-iteration order
    /// is detectable.
    struct NamedStub;
    impl Model for NamedStub {
        fn param_names(&self) -> Vec<&'static str> {
            vec![]
        }
        fn output_names(&self) -> Vec<String> {
            vec![]
        }
        fn param_bounds(&self) -> Vec<(f64, f64)> {
            vec![]
        }
        fn fixed_mask(&self) -> Vec<bool> {
            vec![]
        }
        fn required_inputs(&self) -> Vec<crate::core::model::InputSpec> {
            vec![]
        }
        fn measurement(&self) -> MeasurementKind {
            MeasurementKind::Named {
                roles: &["T1w", "PDw", "MTw"],
            }
        }
        fn forward(&self, _params: &[f64], _aux: &crate::core::model::Aux) -> Measurement {
            unimplemented!()
        }
        fn fit(&self, _m: &Measurement, _aux: &crate::core::model::Aux) -> Vec<f64> {
            unimplemented!()
        }
        fn n_volumes(&self) -> usize {
            3
        }
        fn bids_volume(&self, index: usize) -> crate::core::model::BidsVolume {
            let roles = ["T1w", "PDw", "MTw"];
            crate::core::model::BidsVolume {
                entities: vec![("role", roles[index].to_string())],
                sidecar: std::collections::BTreeMap::new(),
            }
        }
    }

    #[test]
    fn measurement_values_named_follows_declared_role_order_not_alphabetical() {
        let model = NamedStub;
        let mut map = std::collections::BTreeMap::new();
        map.insert("MTw", 2.0);
        map.insert("PDw", 1.0);
        map.insert("T1w", 3.0);
        let meas = Measurement::Named(map);
        // Alphabetical (BTreeMap) order would be [MTw, PDw, T1w] = [2.0, 1.0, 3.0].
        // Declared role order is [T1w, PDw, MTw] = [3.0, 1.0, 2.0].
        assert_eq!(measurement_values(&model, &meas), vec![3.0, 1.0, 2.0]);
    }

    #[test]
    fn montecarlo_draws_respect_bounds() {
        // std=5.0 would blow past F's ub=0.5 without clamping.
        let (cfg, raw) =
            qmt_cfg("  trials: 50\n  distributions:\n    F: { mean: 0.4, std: 5.0 }\n");
        let r = run_montecarlo(&cfg, &raw).unwrap();
        let f = r.stats.iter().find(|s| s.name == "F").unwrap();
        // truth = mean of clamped drawn F; must stay within [lb, ub] = [1e-4, 0.5].
        assert!(
            f.truth >= 1e-4 - 1e-9 && f.truth <= 0.5 + 1e-9,
            "F truth out of bounds: {}",
            f.truth
        );
    }
}
