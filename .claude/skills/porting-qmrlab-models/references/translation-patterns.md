# Translating MATLAB math into a qmrust model

Phase 2 of the port: turn the written statement from Phase 1 into a `Model` +
`ModelConfig` impl. This doc covers the shape the scaffold hands you, the two
compile-time constraints every model must satisfy (core purity, object
safety), the `ModelConfig` hooks as a template method, and the recurring
MATLAB-to-Rust idioms.

## The pipeline the scaffold hands you

`.claude/skills/porting-qmrlab-models/scaffold_model.sh <name> <Suffix>` (run from
the repo root) clones `inversion_recovery` (the living reference model) into
`crates/qmrust-core/src/models/<name>/` and renames its symbols. The result is a three-file layout, each file with one job:

- **`config.rs`** — `<Camel>Config`, a plain data struct
  (`Deserialize + Serialize + Default`) holding every option and protocol
  field the model needs, plus its own `validate_options`/`validate_protocol`
  inherent methods (see `crates/qmrust-core/src/models/inversion_recovery/config.rs`
  for the shape: `IrConfig` with `inversion_times: Vec<f64>`, a `method`
  option, a `t1_range`, and two validation methods called from the
  `ModelConfig` impl in `model.rs`).
- **the math file** — the pure signal equation and fitter, no `Model`/`ModelConfig`
  awareness at all. In the reference model this is `fit.rs`: `IrFitter` with
  `forward(t1, a, b) -> Vec<f64>` (the qMRLab `equation`) and `fit_voxel`
  (the qMRLab `fit`), built once from `&IrConfig` via `IrFitter::new`. A
  model with a heavier math core (lineshapes, ODEs, special functions) splits
  this further — see `crates/qmrust-core/src/models/qmt_spgr/` (`fit.rs`,
  `lineshape.rs`, `ode.rs`, `sf.rs`, `pulse.rs`) — but the fitter type is
  still the one thing `model.rs` constructs and calls.
- **`model.rs`** — `impl Model for <Camel>Model` (wraps the fitter),
  `impl ModelConfig for <Camel>Config` (the hooks below), and three free
  functions the registry calls: `build(v, proto)` →
  `core::model::build_model::<Config>(v, proto)`, `describe(v)` →
  `describe_model::<Config>(v)`, `dump(v)` → `dump_model::<Config>(v)`. See
  `crates/qmrust-core/src/models/inversion_recovery/model.rs` for all three,
  each a one-line call into the shared pipeline in
  `crates/qmrust-core/src/core/model.rs`.

`mod.rs` just wires the three files together and re-exports
`build`/`describe`/`dump` (`crates/qmrust-core/src/models/inversion_recovery/mod.rs`).

### The four TODO(port) markers

The scaffold prepends a `// TODO(port): ...` banner to exactly the pieces that
still hold IR's logic, plus one in the BIDS grouping grammar:

1. `config.rs` — replace IR's config fields with the target model's options
   and protocol arrays.
2. the math file — replace the IR signal equation and fitter with the
   target model's.
3. `model.rs` — replace IR's protocol mapping, `bids()`, and output
   declarations with the target model's.
4. `crates/rust-bids/src/default_grouping.yaml` — the appended
   `<Suffix>: sequential_set: by: [inv]` block defaults to IR-style grouping;
   set the real grouping entities (e.g. `[mt, flip]` for `QMTSPGR`).

Grep the new model directory for `TODO(port)` before calling the port done —
an unreplaced banner means a file still runs IR's math under the new name.

## Purity: `qmrust-core` has no I/O

`qmrust-core` (and therefore every model under `crates/qmrust-core/src/models/`)
must build for `wasm32-unknown-unknown`. That means no `std::fs`, no `nifti`,
no `matfile`, no BIDS-traversal or CLI dependency anywhere reachable from a
non-test, non-wasm-excluded path. Verify after every edit:

```bash
cargo build -p qmrust-core --target wasm32-unknown-unknown
```

The one place the reference tree reads a `.mat` file —
`crates/qmrust-core/src/models/qmt_spgr/sf.rs`'s `load_reference_arrays`, which
cross-checks the Rust lineshape against a MATLAB-exported table — is gated
`#[cfg(test)]` (its test module further `#[cfg(all(test, not(target_arch =
"wasm32")))]`, since `matfile` isn't wasm-portable). That is the pattern: code
reading a MATLAB fixture to prove a port lives behind `#[cfg(test)]` (or
`#[cfg(not(target_arch = "wasm32"))]` for genuinely native-only non-test code),
never in the model's shipped path.

## Object safety: `Model` is `Box<dyn Model>`-safe by construction

The registry holds every model as `Box<dyn Model>`
(`crates/qmrust-core/src/core/model.rs`'s `#[test] fn model_is_object_safe`
is a compile-time proof of this). Two consequences for a new model:

- **No generics or associated types on `Model` itself.** Put any
  model-specific generic parameter on the fitter/config type behind the
  trait impl, never on a `Model` method signature — a generic method isn't
  object-safe and won't compile against `dyn Model`.
- **`Source::Derived` must be a plain `fn` pointer, not a capturing
  closure.** `Source::Derived(fn(&dyn Meta) -> AnyResult<f64>)`
  (`crates/qmrust-core/src/core/model.rs`) exists so a model can declare a
  BIDS-metadata derivation (e.g. combining two sidecar fields) without
  `Model`/`ProtoParam` naming a generic closure type, which would break
  object safety. Write it as a free function, or a closure that captures
  nothing (which Rust coerces to a fn pointer) — see
  `crates/rust-bids/src/protocol.rs`'s `derived_schema` test helper,
  `Source::Derived(|m| { ... })`, for a zero-capture closure in this shape.
  A closure that captures model state will not typecheck here; if the
  derivation needs per-model data, resolve it a different way (e.g. fold it
  into `ingest_protocol` instead of `protocol_schema`).

## The `ModelConfig` hooks as template-method slots

`crate::core::model::build_model::<C>` (`crates/qmrust-core/src/core/model.rs`)
is the one pipeline every model runs: parse config → `validate_options` →
`ingest_protocol` → `validate_protocol` → `into_model` → check the built
model's `measurement()` against the resolved protocol. A model contributes
only these four hooks on its `ModelConfig` impl — it never reimplements the
sequencing:

- **`validate_options(&mut self)`** — config-intrinsic checks that need no
  protocol: option enums are set, ranges are sane. `IrConfig`'s impl
  (`crates/qmrust-core/src/models/inversion_recovery/config.rs`) rejects a
  missing fit `method` or an inverted T1 range here, but does *not* require
  `inversion_times` to be populated yet — that's a protocol concern.
- **`ingest_protocol(&mut self, proto: &Protocol)`** — fold the BIDS-resolved
  per-volume protocol into the config's own acquisition arrays. Default is a
  no-op (a model whose acquisition is entirely config-sourced, e.g.
  `QmtSpgrConfig`, never overrides it). `IrModel`'s impl
  (`crates/qmrust-core/src/models/inversion_recovery/model.rs`) pulls
  `InversionTime` out of each `proto.volumes` entry into
  `self.inversion_times`, but only when `proto` is non-empty — an empty
  `Protocol` means "use the config as written" (the non-BIDS path).
- **`validate_protocol(&mut self)`** — completeness checks that need the
  final acquisition arrays: `IrConfig::validate_protocol` requires at least
  three inversion times and sorts them ascending, run only after ingestion
  so it sees the BIDS-resolved values too.
- **`into_model(self) -> Box<dyn Model>`** — construct the fit-ready model
  from the finalized config. `IrModel::new(self)` builds the fitter once
  from the config; nothing after this point re-reads the config.

`describe_model`/`dump_model` run only `validate_options` (structural
interrogation and config-dump don't need a protocol); only `build_model` runs
the full sequence and the post-construction `validate_against_protocol`
check. Put a check in the earliest hook it can run in — a check that needs
no protocol but lives in `validate_protocol` runs too late for `describe`/`dump`
to catch it.

## MATLAB → Rust idioms

- **1-based, column-major indexing → 0-based, row-major (or explicit
  strides).** qMRLab's `x(1)` is Rust's `x[0]`. A MATLAB 3-D array flattened
  to a `.mat` numeric vector is column-major: element `(i,j,k)` (0-based) of
  an `(nA, nO, nT)` array sits at flat index `i + j*nA + k*nA*nO`. See
  `crates/qmrust-core/src/models/qmt_spgr/sf.rs`'s `load_reference_arrays`
  for exactly this unpack, done with explicit nested loops rather than any
  clever reshape — the loop nesting order documents the stride, which a
  one-line reshape would hide.
- **Vectorized `.*`/`./`/`sum(...)` → explicit loops or `ndarray`/slice
  ops.** MATLAB's elementwise array ops over an entire `Prot` table become a
  Rust loop over the same length (or a `.iter().zip(...).map(...)`), not a
  hidden broadcast — `IrFitter::forward` in
  `crates/qmrust-core/src/models/inversion_recovery/fit.rs` maps over
  `self.ti` explicitly rather than doing the MATLAB-style batch arithmetic
  MATLAB syntax hides.
- **`properties`/`buttons`/`Prot` fields → typed config fields.** Every
  qMRLab property that feeds the equation or fit becomes a named,
  typed field on `<Camel>Config` (or a small nested struct, e.g. `T1Range`,
  `ZoomConfig` in `IrConfig`) — never a stringly-typed catch-all map. A
  `buttons` string choice (e.g. `"Magnitude"`/`"Complex"`) becomes a Rust
  enum (`FitMethod`), not a `String` compared by value at call sites.
- **Guard against silently shipping IR's equation.** The scaffold clones a
  real, working model; a port that only renames symbols without touching
  the math file compiles, passes the round-trip test (because IR's forward
  and IR's fit are mutually consistent), and is entirely wrong. Prove the
  new equation actually replaced the old one: `IrFitter::forward` is
  `a + b*exp(-TI/T1)` — if the new model's forward output is numerically
  indistinguishable from that on the target model's own protocol, the port
  didn't happen. Confirm the new `forward` reproduces the value from the
  written-down equation (Phase 1) at a hand-computed point before trusting
  the round-trip test at all.
