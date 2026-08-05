//! Properties every fit must satisfy, over generated inputs rather than the
//! handful of parameter values a person thinks to write down.
//!
//! Two kinds live here, and the difference matters when one fails.
//!
//! A **structural** property is true of every model by contract: `forward`
//! produces one sample per volume, `fit` produces one value per parameter. A
//! failure is a broken contract, always a bug, and the property covers models
//! that do not exist yet.
//!
//! A **numerical** property says a noise-free signal fits back to the
//! parameters that generated it. A failure there is a bug only if the input was
//! well conditioned, so each generator's range is part of the claim: it states
//! where the model is being held to recovery, and the comment above it says why
//! that is the honest boundary rather than a tolerance tuned until green.

use proptest::prelude::*;
use qmrust_core::core::model::{Aux, Measurement, MeasurementKind, Model, Protocol, Sample};

/// Every registered model, built from the non-BIDS recipe it ships.
///
/// That recipe is the one that carries the acquisition, so it is the only
/// input from which every model builds without a resolved protocol. Reading
/// each model's own shipped file also means a model added later is covered by
/// these properties the moment it is registered, with no per-model line here.
fn every_model() -> Vec<(&'static str, Box<dyn qmrust_core::core::model::Model>)> {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    qmrust_core::registry::all()
        .iter()
        .map(|entry| {
            let path = format!("{root}/{}", entry.doc.recipes.non_bids);
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("{}: cannot read {path}: {e}", entry.name));
            let v: serde_yaml::Value = serde_yaml::from_str(&text)
                .unwrap_or_else(|e| panic!("{}: {path} is not YAML: {e}", entry.name));
            let model = (entry.build)(&v, &Protocol::default())
                .unwrap_or_else(|e| panic!("{}: {path} does not build: {e:#}", entry.name));
            (entry.name, model)
        })
        .collect()
}

/// A unit value for each auxiliary input a model needs. B1 and R1 are both
/// normalized quantities whose neutral value is 1, so this is a real
/// acquisition rather than a placeholder that happens to type-check.
fn unit_aux(model: &dyn qmrust_core::core::model::Model) -> Aux {
    let mut aux = Aux::new();
    for name in model.sim_required_aux() {
        aux.set(name, 1.0);
    }
    aux
}

fn samples(m: &Measurement) -> usize {
    match m {
        Measurement::Named(values) => values.len(),
        Measurement::Series(rows) => rows.len(),
    }
}

/// Params inside a model's own declared bounds, as a fraction of each range.
/// Generating the fractions rather than the values keeps one generator honest
/// for every model, whatever its parameters mean or how many it has.
fn params_within_bounds(
    model: &dyn qmrust_core::core::model::Model,
    fractions: &[f64],
) -> Vec<f64> {
    model
        .param_bounds()
        .iter()
        .zip(fractions.iter().cycle())
        .map(|(&(lo, hi), &f)| {
            // An unbounded parameter still needs a value; its bounds say
            // nothing about scale, so a plain positive number is the honest
            // choice.
            if !lo.is_finite() || !hi.is_finite() {
                return 1.0;
            }
            lo + f * (hi - lo)
        })
        .collect()
}

proptest! {
    /// `forward` produces exactly one sample per volume, whatever the params.
    ///
    /// The shell labels each acquired volume with one of these samples by
    /// identity. A count that disagrees with `n_volumes()` is not a wrong
    /// number, it is a fit that assembles the wrong signal or fails outright,
    /// and it depends on config the model normalizes rather than on anything a
    /// unit test picks.
    #[test]
    fn forward_yields_one_sample_per_volume(fractions in prop::collection::vec(0.05f64..0.95, 1..6)) {
        for (name, model) in every_model() {
            let params = params_within_bounds(model.as_ref(), &fractions);
            let m = model.forward(&params, &unit_aux(model.as_ref()));
            prop_assert_eq!(
                samples(&m), model.n_volumes(),
                "{}: forward produced {} samples for {} volumes",
                name, samples(&m), model.n_volumes()
            );
        }
    }

    /// `fit` returns exactly one value per name it declares.
    ///
    /// The engine writes these into named maps positionally. A length that
    /// disagrees with `output_names()` mislabels every map after the gap, which
    /// is the failure that produces plausible, wrong output rather than an error.
    #[test]
    fn fit_yields_one_value_per_declared_output(fractions in prop::collection::vec(0.05f64..0.95, 1..6)) {
        for (name, model) in every_model() {
            let aux = unit_aux(model.as_ref());
            let params = params_within_bounds(model.as_ref(), &fractions);
            let fitted = model.fit(&model.forward(&params, &aux), &aux);
            prop_assert_eq!(
                fitted.len(), model.output_names().len(),
                "{}: fit returned {} values for {} outputs",
                name, fitted.len(), model.output_names().len()
            );
        }
    }
}

/// Each registered model built from its **BIDS** recipe: options only, with the
/// acquisition left to the sidecars. That is the input the `--bids-dir` path
/// hands a model, so it is what the contract below is about.
fn every_model_without_an_acquisition() -> Vec<(&'static str, serde_yaml::Value)> {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    qmrust_core::registry::all()
        .iter()
        .map(|entry| {
            let path = format!("{root}/{}", entry.doc.recipes.bids);
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("{}: cannot read {path}: {e}", entry.name));
            let v: serde_yaml::Value = serde_yaml::from_str(&text)
                .unwrap_or_else(|e| panic!("{}: {path} is not YAML: {e}", entry.name));
            (entry.name, v)
        })
        .collect()
}

/// Models whose measurement is a `Series`: their volumes are identified by a
/// row of protocol values rather than by role name. The identity properties
/// below are about exactly that machinery, and a `Named` model is excluded by
/// its own declaration rather than by being listed here.
fn every_series_model() -> Vec<(&'static str, Box<dyn Model>)> {
    every_model()
        .into_iter()
        .filter(|(_, m)| matches!(m.measurement(), MeasurementKind::Series { .. }))
        .collect()
}

/// Every parameter a model fits is documented with a meaning and a unit.
///
/// `symbols` is the only place a parameter's unit is stated, and the playground
/// reads it to label a simulated ground-truth value. A parameter missing from it
/// is a form row with no unit and a docs page that does not mention a quantity
/// the model fits. Derived from each model's own `param_names`, so a model added
/// later is covered without being listed here.
#[test]
fn every_fitted_parameter_declares_a_meaning_and_a_unit() {
    for (name, model) in every_model() {
        let entry = qmrust_core::registry::all()
            .iter()
            .find(|e| e.name == name)
            .expect("a built model is a registered one");
        for param in model.param_names() {
            assert!(
                entry.doc.symbols.iter().any(|(sym, _, _)| *sym == param),
                "{name}: fits '{param}' but declares no symbol entry for it, \
                 so it has no unit to show"
            );
        }
    }
}

/// Every name a model exports as a BIDS map must be one it actually produces.
///
/// The writer looks each up in the fit result by name; one that is not there is
/// not an error, it is a map silently never written.
#[test]
fn bids_outputs_name_real_outputs() {
    for (name, model) in every_model() {
        let outputs = model.output_names();
        for (out, suffix, _unit) in model.bids_outputs() {
            assert!(
                outputs.iter().any(|n| n == out),
                "{name}: bids_outputs exports '{out}' as {suffix}, \
                 which is not one of its outputs {outputs:?}"
            );
        }
    }
}

/// A model describes from its own BIDS recipe, and says what the sidecars must
/// supply.
///
/// The `--bids-dir` path builds a model before any sidecar is read, to ask what
/// to look for. A model that cannot be built without its acquisition, or that
/// declares no protocol while its recipe omits one, could never be resolved
/// from a dataset.
#[test]
fn a_bids_recipe_describes_and_declares_what_the_sidecars_owe_it() {
    for (name, value) in every_model_without_an_acquisition() {
        let entry = qmrust_core::registry::by_name(name).expect(name);
        let model = (entry.describe)(&value)
            .unwrap_or_else(|e| panic!("{name}: does not describe from its BIDS recipe: {e:#}"));
        // `mt_ratio` has no acquisition at all, and says so with an empty
        // schema; anything else must name what it needs.
        if model.n_volumes() == 0 {
            assert!(
                !model.protocol_schema().is_empty(),
                "{name}: describes zero volumes and declares no protocol, \
                 so nothing could ever resolve it"
            );
        }
    }
}

/// A model that needs an acquisition must refuse to build without one.
///
/// Building anyway fits whatever its config defaults happen to be while the
/// output's `Protocol` reports the dataset's. Conditioned on the model's own
/// schema, so a model with no acquisition (`mt_ratio`) is excluded by what it
/// declares rather than by name.
///
/// `qmt_spgr` is the one model this does not hold for: its config carries a
/// default saturation grid (`qmt_default_mtdata`), so it builds without one and
/// would fit that grid rather than the dataset's. Named here because the
/// alternative is a property that quietly excludes it, which is how the gap
/// stopped being visible in the first place.
#[test]
fn a_model_needing_an_acquisition_refuses_to_build_without_one() {
    for (name, value) in every_model_without_an_acquisition() {
        if name == "qmt_spgr" {
            continue;
        }
        let entry = qmrust_core::registry::by_name(name).expect(name);
        let Ok(described) = (entry.describe)(&value) else {
            continue;
        };
        if described.protocol_schema().is_empty() {
            continue;
        }
        assert!(
            (entry.build)(&value, &Protocol::default()).is_err(),
            "{name}: built from a recipe with no acquisition, though it declares \
             {:?}",
            described
                .protocol_schema()
                .iter()
                .map(|p| p.name)
                .collect::<Vec<_>>()
        );
    }
}

/// `forward` tags its samples with the identities the BIDS path builds.
///
/// A `Series` model's volume identities come from the resolved per-volume
/// protocol; `forward` tags its samples from the model's own rows. Any protocol
/// param one side emits and the other does not joins the identity on one side
/// only, and no predicted sample matches its volume. The fit still works (it
/// queries one key by name), so the only symptom is a silently missing forward
/// curve in the app.
#[test]
fn forward_samples_carry_the_identities_the_bids_path_builds() {
    for (name, model) in every_series_model() {
        let MeasurementKind::Series { rows } = model.measurement() else {
            unreachable!("filtered to Series")
        };
        let proto = Protocol {
            volumes: rows.clone(),
            global: Default::default(),
        };
        let ids = qmrust_core::engine::build_volume_ids(model.measurement(), &proto, rows.len())
            .unwrap_or_else(|e| panic!("{name}: volume ids: {e:#}"));
        let params = params_within_bounds(model.as_ref(), &[0.5]);
        let signal = model.forward(&params, &unit_aux(model.as_ref()));
        let samples = signal.series();
        assert_eq!(samples.len(), ids.len(), "{name}: sample/volume count");
        for (id, sample) in ids.iter().zip(samples) {
            let qmrust_core::core::model::VolumeId::Params(row) = id else {
                panic!("{name}: a Series model must yield param-row identities")
            };
            assert_eq!(
                *row, sample.params,
                "{name}: volume identity {row:?} has no matching forward sample {:?}",
                sample.params
            );
        }
    }
}

/// `fit` assembles its signal by identity, not by position.
///
/// The order volumes reach a model is the dataset's, not the model's. A
/// positional read pairs every value with the wrong protocol row and produces a
/// plausible, wrong map rather than an error.
#[test]
fn fit_is_invariant_to_the_order_samples_arrive_in() {
    for (name, model) in every_series_model() {
        let aux = unit_aux(model.as_ref());
        let params = params_within_bounds(model.as_ref(), &[0.5]);
        let signal = model.forward(&params, &aux);
        let mut reversed: Vec<Sample> = signal
            .series()
            .iter()
            .map(|s| Sample {
                params: s.params.clone(),
                value: s.value,
            })
            .collect();
        reversed.reverse();
        let forward_fit = model.fit(&signal, &aux);
        let reversed_fit = model.fit(&Measurement::Series(reversed), &aux);
        for (i, (a, b)) in forward_fit.iter().zip(&reversed_fit).enumerate() {
            assert!(
                a == b || (a.is_nan() && b.is_nan()),
                "{name}: output {i} changed with sample order: {a} vs {b}"
            );
        }
    }
}

/// A sample whose identity the model does not expect is a mislabeled
/// measurement, and must not be read as if it were one of the model's own.
///
/// The engine catches the panic and records the voxel as unfitted; silently
/// substituting a value would fit one volume's signal under another's protocol.
#[test]
fn fit_rejects_a_measurement_whose_identity_it_does_not_know() {
    for (name, model) in every_series_model() {
        let bogus = Measurement::Series(vec![Sample {
            params: std::collections::BTreeMap::from([("NotAParam".to_string(), 1.0)]),
            value: 1.0,
        }]);
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            model.fit(&bogus, &unit_aux(model.as_ref()))
        }));
        assert!(
            caught.is_err(),
            "{name}: fitted a measurement carrying none of its identities"
        );
    }
}

/// The properties above iterate `every_model()`. A model missing from that list
/// is silently untested rather than failing, and a property that covers nothing
/// passes fastest of all. This is what makes the two above mean what they say.
#[test]
fn every_registered_model_reaches_the_properties() {
    let built: Vec<&str> = every_model().iter().map(|(name, _)| *name).collect();
    let registered: Vec<&str> = qmrust_core::registry::all()
        .iter()
        .map(|e| e.name)
        .collect();
    assert_eq!(built, registered, "models missing from the property runs");
}

// ---------------------------------------------------------------------------
// Numerical: a noise-free signal fits back to the parameters that made it.
// ---------------------------------------------------------------------------

use qmrust_core::models::vfa_t1::config::{FitType, VfaT1Config};
use qmrust_core::models::vfa_t1::fit::VfaT1Fitter;

fn vfa_fitter(flip_angles: Vec<f64>, tr: f64, fit_type: FitType) -> VfaT1Fitter {
    let mut cfg = VfaT1Config {
        flip_angles,
        repetition_time: Some(tr),
        fit_type,
    };
    cfg.flip_angles.sort_by(|a, b| a.partial_cmp(b).unwrap());
    VfaT1Fitter::new(&cfg)
}

proptest! {
    /// VFA recovers the T1 that generated the signal, for any well-conditioned
    /// acquisition.
    ///
    /// The generated ranges are the claim. T1 spans grey and white matter
    /// through CSF at every field strength in use; TR spans what an SPGR
    /// protocol actually runs; the two angles straddle the Ernst angle, which
    /// is the condition under which VFA is worth acquiring at all. B1 spans a
    /// realistic transmit inhomogeneity.
    ///
    /// The linearization's conditioning depends on TR/T1: as it goes to zero
    /// the fitted slope approaches 1, and T1 = -TR/ln(slope) divides by a
    /// vanishing logarithm. The lower bound on the ratio is what keeps this a
    /// statement about the model rather than about floating point.
    #[test]
    fn vfa_recovers_the_t1_that_generated_the_signal(
        t1 in 0.3f64..4.5,
        m0 in 100.0f64..5000.0,
        tr in 0.005f64..0.050,
        b1 in 0.7f64..1.3,
        low in 2.0f64..8.0,
        high in 12.0f64..30.0,
    ) {
        prop_assume!(tr / t1 > 0.005);
        let fitter = vfa_fitter(vec![low, high], tr, FitType::Linear);
        let signal = fitter.forward(t1, m0, b1);
        let out = fitter.fit_voxel(&signal, b1);
        let (fitted_t1, fitted_m0) = (out[0], out[1]);
        prop_assert!(
            (fitted_t1 - t1).abs() / t1 < 1e-6,
            "T1 {t1} -> {fitted_t1} (TR {tr}, B1 {b1}, angles {low}/{high})"
        );
        prop_assert!(
            (fitted_m0 - m0).abs() / m0 < 1e-6,
            "M0 {m0} -> {fitted_m0} (TR {tr}, B1 {b1}, angles {low}/{high})"
        );
    }

    /// The nonlinear fit recovers the same signal at least as well as the
    /// linearization it is seeded from. Fitting the equation itself cannot be
    /// worse than fitting a rearrangement of it on data with no noise, so a
    /// violation means the solver is leaving the basin it started in.
    #[test]
    fn the_nonlinear_fit_is_no_worse_on_noise_free_data(
        t1 in 0.3f64..4.5,
        m0 in 100.0f64..5000.0,
        tr in 0.005f64..0.050,
        low in 2.0f64..8.0,
        high in 12.0f64..30.0,
    ) {
        prop_assume!(tr / t1 > 0.005);
        let signal = vfa_fitter(vec![low, high], tr, FitType::Linear).forward(t1, m0, 1.0);
        let linear = vfa_fitter(vec![low, high], tr, FitType::Linear).fit_voxel(&signal, 1.0);
        let nonlinear = vfa_fitter(vec![low, high], tr, FitType::Nonlinear).fit_voxel(&signal, 1.0);
        let err = |v: f64| (v - t1).abs() / t1;
        prop_assert!(
            err(nonlinear[0]) <= err(linear[0]) + 1e-9,
            "nonlinear {} worse than linear {} for T1 {t1}",
            nonlinear[0], linear[0]
        );
    }
}

/// Every model ships a sim recipe, and all four sim modes run off it.
///
/// `Recipes.sim` is not optional, so a model registered later is covered here
/// the moment it is registered, with no per-model line. Each mode's contract is
/// asserted against what the model itself declares rather than against a
/// remembered number.
#[test]
fn every_model_simulates_from_its_declared_sim_recipe() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    for entry in qmrust_core::registry::all() {
        let name = entry.name;
        let path = format!("{root}/{}", entry.doc.recipes.sim);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{name}: cannot read {path}: {e}"));
        let raw: serde_yaml::Value = serde_yaml::from_str(&text)
            .unwrap_or_else(|e| panic!("{name}: {path} is not YAML: {e}"));
        let cfg: qmrust_core::config::Config = serde_yaml::from_str(&text)
            .unwrap_or_else(|e| panic!("{name}: {path} is not a config: {e}"));
        assert_eq!(cfg.model, name, "{name}: {path} names another model");

        let sim = cfg
            .sim
            .as_ref()
            .unwrap_or_else(|| panic!("{name}: {path} has no sim block"));

        let model = (entry.build)(&raw, &Protocol::default())
            .unwrap_or_else(|e| panic!("{name}: {path} does not build: {e:#}"));

        // signal: one value per volume the model consumes.
        let signal = qmrust_core::sim::run_signal(&cfg, &raw)
            .unwrap_or_else(|e| panic!("{name} signal: {e:#}"));
        assert_eq!(
            signal.signal.len(),
            model.n_volumes(),
            "{name}: signal length must equal the volume count",
        );
        assert!(
            signal.signal.iter().all(|v| v.is_finite()),
            "{name}: a noise-free forward signal must be finite",
        );

        // Every stats-reporting mode reports one row per parameter the fitter
        // estimates, which is the intersection of param_names and output_names.
        let estimated = model
            .param_names()
            .iter()
            .filter(|p| model.output_names().iter().any(|o| o == *p))
            .count();
        assert!(
            estimated > 0,
            "{name}: no parameter is both declared and reported, so sim reports nothing",
        );

        let sv = qmrust_core::sim::run_single_voxel(&cfg, &raw)
            .unwrap_or_else(|e| panic!("{name} single-voxel: {e:#}"));
        assert_eq!(
            sv.stats.len(),
            estimated,
            "{name}: single-voxel stats count"
        );
        assert_eq!(sv.per_trial.len(), sim.trials, "{name}: one fit per trial");
        assert_eq!(
            sv.noisy_signal.len(),
            model.n_volumes(),
            "{name}: the noisy signal covers every volume",
        );

        let sens = qmrust_core::sim::run_sensitivity(&cfg, &raw)
            .unwrap_or_else(|e| panic!("{name} sensitivity: {e:#}"));
        let sweep = sim
            .sweep
            .as_ref()
            .unwrap_or_else(|| panic!("{name}: {path} has no sweep block"));
        assert_eq!(sens.points.len(), sweep.steps, "{name}: one point per step");
        assert_eq!(sens.swept_param, sweep.param, "{name}: swept parameter");
        for p in &sens.points {
            assert_eq!(p.stats.len(), estimated, "{name}: sweep point stats count");
        }

        let mc = qmrust_core::sim::run_montecarlo(&cfg, &raw)
            .unwrap_or_else(|e| panic!("{name} montecarlo: {e:#}"));
        assert_eq!(mc.trials, sim.trials, "{name}: montecarlo trial count");
        assert_eq!(mc.stats.len(), estimated, "{name}: montecarlo stats count");
        assert_eq!(
            mc.per_trial_input.len(),
            sim.trials,
            "{name}: one input row per montecarlo trial",
        );
        assert_eq!(
            mc.per_trial_fitted.len(),
            sim.trials,
            "{name}: one fitted row per montecarlo trial",
        );
        for row in &mc.per_trial_input {
            assert_eq!(
                row.len(),
                mc.stats.len(),
                "{name}: an input row must cover every reported parameter",
            );
        }
        for row in &mc.per_trial_fitted {
            assert_eq!(
                row.len(),
                mc.stats.len(),
                "{name}: a fitted row must cover every reported parameter",
            );
        }
    }
}
