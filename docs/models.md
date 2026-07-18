# Adding a model

qmrust fits several qMRI models (inversion-recovery T1, qMT-SPGR, ...) and is
designed so that adding another one touches exactly two places: a new module,
and one line in a registry. Nothing else — not the CLI, not the config parser,
not the fitting engine, not the simulator — needs to know a new model exists.

## Why: two seams, not scattered branches

- **The `Model` trait** (`qmrust_core::core::model::Model`) is the whole
  contributor surface. It is object-safe (used as `Box<dyn Model>`), so no
  generics or associated types — just methods: `param_names`, `output_names`,
  `param_bounds`, `fixed_mask`, `required_inputs`, `n_acquisitions`,
  `forward`, `fit`, and optionally `strategy`/`bids`. The core only ever sees
  ordered `f64` slices and a scalar `Aux` bundle (`aux.get("B1map")`) — never
  files, JSON, or a config format. That's what keeps it portable to wasm.
- **The registry** (`registry::all()` in `registry.rs`) is the whole dispatch
  point: a static list of `ModelEntry { name, bids_suffix, build }`. The CLI,
  the simulator, and the wasm bindings all resolve a model by calling
  `registry::by_name` — there is no `match cfg.model { ... }` anywhere else in
  the codebase.

## Checklist: add a model

1. New directory `crates/qmrust-core/src/models/<name>/`: a `config.rs` (a
   `serde` struct + a `validate()` method), the pure math, and a `model.rs`
   (`impl Model` + a `pub fn build`).
2. Register the module in `models/mod.rs`.
3. Add **one** `ModelEntry` to `registry::all()` in `registry.rs` (name + BIDS
   suffix + `build`).
4. Add tests: a forward → fit round-trip, and config parse/validate.

That's it. Use `models/inversion_recovery/` as the minimal reference model;
`models/qmt_spgr/` shows a nested-config model that also declares auxiliary
inputs (B1/B0/R1 maps) via `required_inputs()`.

## Going deeper

For the full trait definition, the supporting value types (`Aux`,
`InputSpec`, `FitStrategy`, `Protocol`, `BidsSpec`), and a worked
line-by-line example, see
[`docs/agents/ARCHITECTURE.md`](agents/ARCHITECTURE.md#the-model-trait--the-single-contributor-surface).
