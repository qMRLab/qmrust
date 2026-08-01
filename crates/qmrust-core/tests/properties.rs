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
use qmrust_core::core::model::{Aux, Measurement, Protocol};

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
