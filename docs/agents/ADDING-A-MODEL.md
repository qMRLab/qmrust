# Adding a model — agent checklist

Read this when asked to add or convert a qMRI model in qmrust. See
[`ARCHITECTURE.md`](ARCHITECTURE.md) for the system map and
[`DATA-PIPELINE.md`](DATA-PIPELINE.md) for the BIDS/sidecar machinery this
doc's contract section summarizes.

## Invariants (do not violate)

- **`qmrust-core` stays pure.** No dependency on `qmrust-cli`/`qmrust-wasm`/
  `rust-bids`; no `clap`/`nifti`/`indicatif`/`owo-colors`; no `std::fs`/
  `matfile` on `wasm32-unknown-unknown` (gate with `#[cfg(not(target_arch =
  "wasm32"))]`).
- **Object safety.** `Model` is used as `Box<dyn Model>` — no generics or
  associated types on the trait.
- **One registry line, no branching.** Do not add `match cfg.model { ... }`
  anywhere in the CLI, engine, sim, or config. `registry::all()` is the only
  dispatch point.
- **Each model owns its config.** A per-model `serde` struct parsed from that
  model's own YAML sub-tree; the top-level `Config` only holds shared fields
  (`model`, `sim`).
- **BIDS-native units (SI), no internal conversion.** Time in seconds
  (`RepetitionTime`/`EchoTime`/`InversionTime`, fitted time constants like
  T1/T2), frequency in Hz, field in tesla, angle in radians except BIDS-MRI's
  `FlipAngle` (degrees). Convert non-BIDS sources (qMRLab `.mat` in ms) at the
  shell boundary — never inside `qmrust-core`.
- **Behaviour-preserving refactors.** Fit output must not drift; diff output
  maps against fixtures when in doubt (CI's OSF integration job runs the real
  pipelines).

## The `Model` trait — current method list

`crates/qmrust-core/src/core/model.rs`. Confirm against source before citing
— do not assume a method exists.

```rust
pub trait Model: Send + Sync {
    fn param_names(&self) -> Vec<&'static str>;
    fn output_names(&self) -> Vec<String>;
    fn param_bounds(&self) -> Vec<(f64, f64)>;
    fn fixed_mask(&self) -> Vec<bool>;
    fn required_inputs(&self) -> Vec<InputSpec>;
    fn measurement(&self) -> MeasurementKind;

    fn strategy(&self) -> FitStrategy { FitStrategy::Voxelwise }

    fn forward(&self, params: &[f64], aux: &Aux) -> Measurement;
    fn fit(&self, m: &Measurement, aux: &Aux) -> Vec<f64>;

    fn bids(&self) -> Option<BidsSpec> { None }
    fn protocol_schema(&self) -> Vec<ProtoParam> { vec![] }
    fn bids_outputs(&self) -> Vec<(&'static str, &'static str, &'static str)> { vec![] }
}
```

There is **no `n_acquisitions`** — removed in favor of `measurement()`. Do
not reintroduce it.

- `measurement() -> MeasurementKind`: `Named { roles: &'static [&'static
  str] }` for fixed role-labeled volumes, or `Series { rows: Vec<BTreeMap
  <String, f64>> }` for a variable-length series whose canonical per-volume
  identity rows the model owns (e.g. IR: one `{"InversionTime": ti}` per
  TI). `forward`/`fit` read the `Measurement` the engine builds by identity
  (`m.role(name)` / `m.series()`) — never by position.
- `protocol_schema() -> Vec<ProtoParam>` (default `vec![]`, opt-in):
  `ProtoParam { name, source, scope }`. `Source::Field(key)` — sidecar value
  by key. `Source::Derived(fn(&dyn Meta) -> Result<f64>)` — computed from
  several sidecar fields; must be a plain `fn` pointer (not a closure) to
  keep `Model` object-safe. `Source::Option(key)` — non-BIDS fallback read
  from `--config`. `Scope::PerVolume` | `Scope::Global`.
- `required_inputs() -> Vec<InputSpec>`: `InputSpec { name, required, bids:
  Option<BidsMap> }`, `BidsMap { suffix, entity: Option<&str> }`. Model reads
  via `aux.get(name)`.
- `bids_outputs() -> Vec<(&'static str, &'static str, &'static str)>`
  (default `vec![]`): **3-tuple** `(output_name, BIDS-derivatives suffix,
  unit)` — e.g. IR `("T1", "T1map", "s")`, qMT `("kr", "kRmap", "1/s")`,
  `""` for a unitless quantity. Every first element must be a real
  `output_names()` entry; diagnostics (residuals, indices) are omitted.
  `qmrust fit --bids-dir` uses this to write `derivatives/qmrust/...`.
- `bids() -> Option<BidsSpec>`: `BidsSpec { suffix, entities }`.

## The registry

`crates/qmrust-core/src/registry.rs`:

```rust
pub type Builder = fn(&serde_yaml::Value, &Protocol) -> Result<Box<dyn Model>>;
pub struct ModelEntry { pub name: &'static str, pub bids_suffix: &'static str, pub build: Builder }
pub fn all() -> &'static [ModelEntry];
pub fn by_name(name: &str) -> Option<&'static ModelEntry>;
pub fn by_bids_suffix(suffix: &str) -> Option<&'static ModelEntry>;
```

`by_name`/`by_bids_suffix` are the only lookups the CLI, sim, and wasm
bindings use.

## Checklist

1. New dir `crates/qmrust-core/src/models/<name>/`:
   - `config.rs` — a `serde`-deserializable struct for the model's own YAML
     sub-tree + a `validate()` method.
   - pure math (signal equation + fitter).
   - `model.rs` — `impl Model for <Name>Model` + `pub fn build(v:
     &serde_yaml::Value, proto: &Protocol) -> Result<Box<dyn Model>>` that:
     parses `config.rs`'s struct from `v`, applies any protocol override,
     calls `.validate()`, constructs the model, then calls
     `validate_against_protocol(&model.measurement(), proto)?` before boxing
     — fail loudly at build time, not per-voxel.
2. Register the module in `models/mod.rs`.
3. Add **one** `ModelEntry` to `registry::all()` in `registry.rs`.
4. Tests: forward→fit round-trip; config parse/validate; if `bids_outputs()`
   is non-empty, assert every entry names a real `output_names()` value.

Reference models:
- `models/inversion_recovery/` — minimal. `protocol_schema()` maps
  `InversionTime` straight off the sidecar; no aux inputs; `Series`
  measurement.
- `models/qmt_spgr/` — nested config (`qmt_spgr:` sub-key), aux inputs
  (B1/B0/R1 via `required_inputs()` + `BidsMap`), `Series` measurement keyed
  by `(Angle, Offset)`. Its own `protocol_schema()` (Angle/Offset mapping) is
  in place — check current source for its exact shape before citing.

## BIDS-only inputs — what the shell resolves for you

qmrust fits BIDS/BIDS-like layouts only (`--bids-dir`, or `--mat-dir`/
`--mat-data` converted via `bidsify`). `rust-bids` parses the whole dataset —
raw tree + every `derivatives/<pipeline>/` — into a flat table
(`parse_to_table`), queried by `table_filter(rows, &[(column, value)])`.

- **Vocabulary** (`rust_bids::Vocabulary`, `crates/rust-bids/src/vocab.rs`):
  canonical BIDS entities/suffixes/datatypes transcribed from the spec, plus
  every registered model's `bids_suffix` at compile time (no config needed
  for a model already in the registry). `Vocabulary::bids()` = built-in only;
  `Vocabulary::from_config(cfg)` layers a dataset's `custom_entities`/
  `custom_suffixes` on top. An unrecognized suffix stays in the table but is
  warned about (permissive but loud).
- **Non-official BIDS layout is declared, not hardcoded**, in `BidsConfig`
  (`crates/rust-bids/src/config.rs`):
  - `custom_suffixes: [QMTSPGR]` — non-official suffix, stays
    `.bidsignore`-exempt.
  - `custom_entities: [{ key: cest, name: cestPool }]` — non-official entity
    key → full name.
  - Grouping: `loop_over: [subject, session, run, task]` (collection
    identity) plus per-suffix `sequential_set: { by: [...] }` (ordered
    series, e.g. IRT1 `by: [inversion]`, QMTSPGR `by: [mtransfer, flip]`) or
    `named_set: { <role>: {...}, required: [...] }` (fixed role slots, e.g.
    MTS PDw/MTw/T1w).
- **Aux + mask resolution** (`qmrust-cli::commands::resolve_aux_and_mask`): a
  fit resolves each `required_inputs()` entry from the table by the
  collection's full identity + declared suffix (+ `BidsMap.entity`), found
  in raw or any derivatives pipeline; a missing required input is a hard
  error, a missing optional one leaves the model's default. The mask is
  declared separately in `--config`'s `mask:` block (`suffix` default
  `mask`, plus entity constraints, e.g. `desc: brain`) — never auto-picked,
  since a dataset may hold several; an under-specified `mask:` matching
  several files is a hard error; no `mask:` block means no masking.

  ```yaml
  mask:
    desc: brain
  ```

## Units

BIDS-native SI, no internal conversion — see "Units" in [`CLAUDE.md`](../../CLAUDE.md)
and [`ARCHITECTURE.md`](ARCHITECTURE.md#units) for the full rule.

## Verify before calling it done

```bash
cargo test  --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build -p qmrust-core --target wasm32-unknown-unknown
cargo build -p rust-bids   --target wasm32-unknown-unknown
cargo test  -p qmrust-wasm --target wasm32-unknown-unknown --no-run
```

All must pass before the model is considered added.

## Going deeper

- [`ARCHITECTURE.md`](ARCHITECTURE.md) — system map, data flow, full
  supporting-type reference.
- [`DATA-PIPELINE.md`](DATA-PIPELINE.md) — BIDS layout resolution, sidecar
  merge, the model input contract, and the output/provenance side in full.
