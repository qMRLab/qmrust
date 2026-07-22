# Optional and sim-critical image inputs

Most models take more than the acquired series: a B1 map, a B0 map, an
observed R1 map. There are three distinct mechanisms for wiring these, and
they answer three different questions. Conflating them breaks either a BIDS
fit or a sim round-trip.

## `required_inputs()` — the fit-resolution contract

`Model::required_inputs` (`crates/qmrust-core/src/core/model.rs`) returns one
`InputSpec` per auxiliary scalar the model can consume:

```rust
pub struct InputSpec {
    pub name: &'static str,           // logical name: aux.get(name)
    pub required: bool,               // hard requirement vs. sensible default
    pub bids: Option<BidsMap>,        // how to locate it in a BIDS dataset
}
```

This is what a BIDS fit uses to resolve auxiliary files before any fitting
happens. `resolve_aux_and_mask` in `crates/qmrust-cli/src/commands.rs` walks
`required_inputs()`, looks each one up by its `BidsMap` suffix/entity, and
then branches on `required`:

- **`required: true`, not found** — hard error. The fit refuses to start
  rather than silently substitute a default for an input the model cannot
  work without.
- **`required: false`, not found** — left absent (`None` in the `Aux`
  bundle); the model's own `fit`/`forward` supplies whatever default makes
  sense for that scalar (e.g. `aux.get("B1map").unwrap_or(1.0)`).
- **found**, either way — loaded and passed through, regardless of
  `required`.

`required` therefore only controls what happens on a *miss*. It says nothing
about whether the model's math actually changes behavior when the value is
present — that is a fitter-internal decision, covered next.

## Used-if-present: an aux gated by a config flag

Some auxiliary inputs are read only when supplied, and whether the fitter
*wants* them at all is itself a config-time choice. qMT-SPGR's R1 constraint
is the reference case: `QmtSpgrConfig.fitting.use_r1map_to_constrain_r1f`
turns the constraint on or off, and the fitter's internal option struct
(`crates/qmrust-core/src/models/qmt_spgr/model.rs`) reads it as:

```rust
if opt.r1map {
    if let Some(r1obs) = opt.r1obs {
        r1f = compute_r1(f, kf, r1r, r1obs);
    }
}
```

Even with the flag on, a missing R1 map is not an error: the model falls
back to solving `R1f` as a free parameter (`opt.r1obs: None`). This is why
`QmtModel::required_inputs()` declares `R1map` with `required: false`
unconditionally — the flag changes what the fitter *does* with R1 if it
shows up, not whether a BIDS fit may proceed without it.

**This must stay `required: false` even when the flag is on.** Promoting it
to `required: true` whenever
`use_r1map_to_constrain_r1f` is set would look like a reasonable
generalization ("the config says to use it, so require it"), but it breaks
a legitimate no-aux fit: `use_r1map_to_constrain_r1f` defaults to `true` for
qMT-SPGR, and the `qmtspgr_bids_fit_matches_mat_fit` round-trip test
(`crates/qmrust-cli/src/commands.rs`) fits that exact configuration against
a dataset with no R1 map on disk. Hard-requiring R1 there would turn a
passing fit into a hard error for every user who wants the constraint
available but doesn't have an R1 map yet.

## `sim_required_aux()` — the sim-side counterpart

A used-if-present aux creates a different problem on the simulation side.
`resolve_aux_and_mask` has real files to fall back on being absent from; a
simulation only has whatever the `sim` config block supplies. If the sim
block omits R1 and the model silently falls back to "solve `R1f` freely",
the simulation exercises a different code path than the one the config
claims to configure — a meaningless test of the R1 constraint.

`Model::sim_required_aux` (`crates/qmrust-core/src/core/model.rs`) is the
seam that prevents this:

```rust
/// Auxiliary inputs (by logical name) that this *configured* model actively
/// uses, such that a simulation omitting them would silently fail to
/// exercise the model's real fitting behaviour. The sim layer requires the
/// sim block to supply each. Empty (the default) means no sim-critical aux.
fn sim_required_aux(&self) -> Vec<&'static str> {
    vec![]
}
```

`QmtModel` (`crates/qmrust-core/src/models/qmt_spgr/adapter.rs`) overrides
it by reading the same flag the fitter reads, at construction time:

```rust
fn sim_required_aux(&self) -> Vec<&'static str> {
    if self.use_r1map {
        vec!["R1map"]
    } else {
        vec![]
    }
}
```

`validate_sim_inputs` (`crates/qmrust-core/src/sim/model.rs`) enforces this
generically against `&dyn Model`, with no per-model branching:

```rust
pub fn validate_sim_inputs(model: &dyn Model, sim: &SimConfig) -> Result<()> {
    let provided = sim_aux(sim);
    for name in model.sim_required_aux() {
        if provided.get(name).is_none() {
            bail!(
                "this model requires sim input '{name}' — supply it in the sim block \
                 (e.g. sim.r1 for R1map, sim.b1/sim.b0 for B1map/B0map)"
            );
        }
    }
    Ok(())
}
```

A model that needs no aux (the default `vec![]`) passes trivially. A model
that only sometimes needs one — like qMT with its config flag — reports it
only when that flag makes the aux load-bearing. Any new model with a similar
config-gated aux wires this the same way: read the config's own flag inside
`sim_required_aux`, dispatched through the trait object, never a
`match cfg.model { ... }` in `validate_sim_inputs` or its caller.

## The rule of judgment

`required_inputs()`'s `required` flag and `sim_required_aux()`'s membership
are independent decisions with different failure modes — BIDS-fit hard error
vs. sim configuration mismatch — and getting either wrong reads as a plausible
generalization right up until it breaks a test.

Before promoting an input to `required: true`, or writing any rule like "if
config flag X is set, input Y is required," check it against the tests, not
against intuition:

- the `#[ignore]`d round-trip tests `bids_fit_matches_mat_fit` /
  `qmtspgr_bids_fit_matches_mat_fit` (`crates/qmrust-cli/src/commands.rs`),
  which assert BIDS-path fits equal `.mat`-path fits exactly;
- `ci/integration_osf.sh`, which exercises the real pipelines against
  qMRLab's OSF datasets.

The qMT R1 case above is the cautionary example: "require R1map when the flag
is on" looks sound until you recall `qmtspgr_bids_fit_matches_mat_fit` fits
under that exact default with no R1map present. The test is the arbiter, not
the plausibility of the generalization.
