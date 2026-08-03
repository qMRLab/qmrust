# qmrust architecture

Native-Rust quantitative-MRI fitting, structured as a **functional core / imperative
shell** so that the numerical models are pure and portable (they compile to
WebAssembly unchanged) while all I/O, CLI, and platform glue live at the edges.

The guiding goal: **you contribute a model by writing one module and adding one line
to a registry** — no edits scattered across the CLI, the config parser, the engine,
or the simulator.

---

## The workspace: four crates

```
qmrust/                         Cargo workspace
├── crates/
│   ├── qmrust-core/   ── FUNCTIONAL CORE ──  pure; no I/O; compiles to wasm32
│   ├── qmrust-cli/    ── IMPERATIVE SHELL ─  the `qmrust` binary: files, CLI, progress
│   ├── qmrust-wasm/   ── IMPERATIVE SHELL ─  the browser cdylib: wasm-bindgen bindings
│   └── rust-bids/     ── SHARED ── wasm-clean qMRI-BIDS layout resolver
├── recipes/                     example `--config` manifests (bids/non-bids/sim, YAML)
├── docs/                        agents/ARCHITECTURE.md (this file) + MyST human-docs site
├── ci/integration_osf.sh        end-to-end fit against qMRLab's OSF datasets
├── ci/datasets.sh               which archives, and how each becomes BIDS (shared)
└── .github/workflows/           ci.yml (lint · native · wasm · integration) + docs.yml (MyST → Pages)
```

**Dependency direction is strict and one-way:**

```
qmrust-cli   ─┐
qmrust-wasm  ─┼──►  qmrust-core   (core depends on NEITHER)
rust-bids    ─┘
```

`qmrust-core` never depends on `qmrust-cli`, `qmrust-wasm`, or `rust-bids` — the arrow
only ever points inward, into core, never back out — and never touches `std::fs` on the
wasm target, and never pulls in `clap`, `nifti`, `matfile`, `indicatif`, or `owo-colors`.
That purity is what lets the exact same fitting/simulation code run in a terminal and in
a browser tab with identical numerical results. `rust-bids` depends on `qmrust-core`
(for `Protocol`) the same way `qmrust-cli`/`qmrust-wasm` do — it is a consumer of core,
not part of it.

### `qmrust-core` — the functional core

```
crates/qmrust-core/src/
├── core/model.rs      the Model trait + value types (the contributor surface)
├── models/            per-model math + config + Model impl + builder
│   ├── inversion_recovery/{config,fit,model}.rs
│   ├── mt_sat/{config,fit,model}.rs
│   └── qmt_spgr/{config,fit,adapter,lineshape,ode,pulse,sf}.rs
├── mtsat_b1/          MTsat B1 correction: 5-state Bloch–McConnell FLASH
│                      sequence sim, surface fit, M0b/R1 calibration,
│                      correction factor, FitValues artifact
├── registry.rs        name / BIDS-suffix → builder  (the one dispatch point)
├── engine.rs          the parallel voxel-fitting engine (FitStrategy)
├── sim/               forward signal, noise, sim→fit round-trips, reports
├── config.rs          parse_config(&str) → (Config, Value)   (parsing only, no fs)
├── fitting.rs         FitResults type
└── quad.rs            numerical quadrature helper
```

Pure. Config **parsing** lives here (`serde_yaml` is wasm-safe); config **file
reading** does not.

### `qmrust-cli` — the terminal shell

The `qmrust` binary. Owns everything the core deliberately excludes:

- `main.rs` — `clap` argument parsing + subcommand dispatch (thin).
- `commands.rs` — the handlers: read files, resolve the model via the registry, load
  auxiliary maps, drive the engine, write NIfTI outputs.
- `io/{mat,nifti}.rs` — MATLAB `.mat` and NIfTI readers/writers (`matfile`, `nifti`, `std::fs`).
- `progress.rs` — an `indicatif` progress bar passed to the engine as a callback.

Subcommands: `fit`, `sim {signal|single-voxel|sensitivity|montecarlo}`, `dump-config`,
`dump-sf`, `bidsify` (qMRLab `.mat` or NIfTI → byte-identical BIDS dataset; see
[`DATA-PIPELINE.md`](DATA-PIPELINE.md)), `mtsat-b1` (simulate the FLASH
sequence surface with the 5-state Bloch–McConnell engine and self-calibrate against a
reference MTS dataset, producing the `FitValues` artifact consumed by `mt_sat`'s
`b1_correction`).

### `qmrust-wasm` — the browser shell

A `cdylib` exposing the core to JavaScript via `wasm-bindgen`. Two layers:

- `api.rs` — **pure** marshalling (`&str` config, typed slices, JSON aux, results).
  Unit-tested on the **native** target, so its correctness is verified without a browser.
- `wasm.rs` — thin `#[wasm_bindgen]` wrappers (compiled only for `wasm32`) that convert
  JS values and call `api`.

`wasm-bindgen`, `js-sys`, `serde-wasm-bindgen`, and `wasm-bindgen-rayon` are
**wasm-target-only** dependencies — they never enter the native build.

### `rust-bids` — the BIDS layout resolver

A wasm-clean, standalone qMRI-BIDS layout resolver, kept as its own crate rather than
folded into `qmrust-core` because it is generalizable beyond this workspace. It groups a
raw dataset into `Collection`s, builds each image's inheritance-merged `Sidecar`, and
evaluates a model's `protocol_schema()` against it to produce a `qmrust_core::Protocol` —
the intended BIDS front door for both the CLI and a future Tauri app, independent of the
`qmrust-core` purity rule (it is not part of core). See
[`DATA-PIPELINE.md`](DATA-PIPELINE.md) for the full walkthrough.

---

## The `Model` trait — the single contributor surface

Everything a model must provide lives in one object-safe trait
(`qmrust_core::core::model::Model`). Object-safe means the registry can hold
`Box<dyn Model>` and the engine/sim/CLI/wasm all drive models through the same
dynamic interface.

```rust
pub trait Model: Send + Sync {
    fn param_names(&self) -> Vec<&'static str>;   // ground-truth params, forward() order
    fn output_names(&self) -> Vec<String>;        // fitted map names, fit() return order
    fn param_bounds(&self) -> Vec<(f64, f64)>;    // per-param (lower, upper)
    fn fixed_mask(&self) -> Vec<bool>;            // true = not independently recovered
    fn required_inputs(&self) -> Vec<InputSpec>;  // auxiliary maps (B1/B0/R1, …)
    fn measurement(&self) -> MeasurementKind;     // measurement shape + identities read by

    fn strategy(&self) -> FitStrategy { FitStrategy::Voxelwise }   // fit granularity

    fn forward(&self, params: &[f64], aux: &Aux) -> Measurement;   // noise-free, identity-tagged
    fn fit(&self, m: &Measurement, aux: &Aux) -> Vec<f64>;         // identity-keyed measurement → outputs
    fn fit_block(&self, ms: &[Measurement], aux: &[Aux]) -> Vec<Vec<f64>>   // block fit; defaults to per-voxel fit()

    fn n_volumes(&self) -> usize;                 // volumes this model's protocol describes
    fn bids_volume(&self, index: usize) -> BidsVolume;   // BIDS write descriptor for the i-th volume

    fn bids(&self) -> Option<BidsSpec> { None }   // BIDS grouping suffix + entity map

    fn protocol_schema(&self) -> Vec<ProtoParam> { vec![] }   // sidecar/config → Protocol mapping
    fn bids_outputs(&self) -> Vec<(&'static str, &'static str, &'static str)> { vec![] }   // (output, suffix, unit)
}
```

The core never sees files, JSON, typed arrays, or config formats — only ordered `f64`
params, an identity-keyed `Measurement`, and a scalar `Aux` bundle. That is the whole
reason it is portable.

### Supporting value types

- **`Aux`** — per-voxel (or per-sim) scalar auxiliary values keyed by logical name:
  `aux.get("B1map") -> Option<f64>`. The shell builds it; the model reads it. The model
  never knows whether the value came from a `.mat` map, a NIfTI, a BIDS sidecar, or a JS
  object.
- **`InputSpec { name, required, bids: Option<BidsMap> }`** — declares one auxiliary
  input. `name` is what the compute layer reads via `aux.get(name)`; `bids` (suffix +
  entity) tells the shell how to locate it in a BIDS dataset. The shell loads exactly
  what each model declares — there is no hardcoded R1/B1/B0 list anywhere.
- **`FitStrategy { Voxelwise, MatrixWise }`** — how the engine iterates. `Voxelwise`
  (independent per-voxel, parallel) is implemented; `MatrixWise` is a declared seam for
  future joint/dictionary methods (`bail!` until a model needs it).
- **`Protocol { volumes, global }`**, **`ProtoParam`/`Source`/`Scope`**, **`Meta`**, and
  **`BidsSpec { suffix, entities }`** — together, the model input contract for BIDS/sidecar
  metadata: a model declares its BIDS identity and a declarative mapping from sidecar
  fields (or config) onto its acquisition protocol; the shell resolves it into a
  `Protocol` and hands it to `build`. Full detail (including the `Source::{Field,
  Derived, Option}` variants and how `resolve_protocol` evaluates them) is in
  [`DATA-PIPELINE.md`](DATA-PIPELINE.md).
- **`bids_outputs() -> Vec<(&'static str, &'static str, &'static str)>`** — which of a
  model's `output_names()` are genuine quantitative maps worth exporting as BIDS
  derivatives: a 3-tuple `(output_name, BIDS-derivatives suffix, unit)`, e.g. IR's
  `("T1", "T1map", "s")` or qMT's `("kr", "kRmap", "1/s")` (`""` for a unitless
  quantity). Diagnostics (residuals, indices, …) are omitted. Default `vec![]`. Used by
  `qmrust fit --bids-dir` to write `derivatives/qmrust/...` (see
  [`DATA-PIPELINE.md`](DATA-PIPELINE.md#6-the-output-side--bids_outputs-and-the-derivatives-layout)).
- **`MeasurementKind { Named { roles }, Series { rows } }`** — a model's declared
  measurement shape: a fixed set of role-labeled volumes, or a variable-length series
  whose canonical per-volume identity `rows` (e.g. one `{"InversionTime": ti}` per TI) the
  model owns.
- **`Measurement { Named(BTreeMap<role, f64>), Series(Vec<Sample>) }`** — the per-voxel
  measurement handed to `forward`/`fit`, read via `.role(name)` / `.series()` — never by
  position.
- **`Sample { params, value }`** — one acquired volume's value tagged with the identity
  row (e.g. `{"InversionTime": 0.5}`, seconds — see [Units](#units)) that names it.
- **`VolumeId { Role(&str), Params(BTreeMap<String, f64>) }`** — the identity the shell
  attaches to one data volume before the engine assembles it into a `Measurement`.

Measurements are identity-keyed, not positional: the engine matches each supplied volume
to a model's declared identity by value, so fitting is order-independent — reordering the
acquisition list yields identical fitted results. An identity with no match fails loudly
(a panic for that voxel) instead of silently assembling the wrong signal.

---

## The registry — the one dispatch point

`registry.rs` is the single place that maps a model name (and a BIDS suffix) to the
function that builds it:

```rust
pub type Builder = fn(&serde_yaml::Value, &Protocol) -> Result<Box<dyn Model>>;
pub type Describer = fn(&serde_yaml::Value) -> Result<Box<dyn Model>>;
pub type Dumper = fn(&serde_yaml::Value) -> Result<String>;
pub type Effective = fn(&serde_yaml::Value, &Protocol) -> Result<EffectiveConfig>;

pub struct ModelEntry { pub name: &'static str, pub bids_suffix: &'static str, pub build: Builder, pub describe: Describer, pub dump: Dumper, pub effective: Effective }

pub fn all() -> &'static [ModelEntry] { &[
    ModelEntry { name: "inversion_recovery", bids_suffix: "IRT1", build: models::inversion_recovery::build, describe: models::inversion_recovery::describe, dump: models::inversion_recovery::dump, effective: models::inversion_recovery::effective },
    ModelEntry { name: "qmt_spgr",           bids_suffix: "QMTSPGR", build: models::qmt_spgr::build, describe: models::qmt_spgr::describe, dump: models::qmt_spgr::dump, effective: models::qmt_spgr::effective },
]}

pub fn by_name(name: &str) -> Option<&'static ModelEntry>;
pub fn by_bids_suffix(suffix: &str) -> Option<&'static ModelEntry>;
```

The CLI, the simulator, and the wasm bindings all resolve models through `by_name`.

Each model's `build`/`describe` are one-liners delegating to a **single shared
build pipeline** in `core::model` (`build_model::<C>` / `describe_model::<C>`):
reject a recipe that restates an acquisition the resolved protocol supplies →
parse the config → `validate_options` → `ingest_protocol` (fold the
BIDS-resolved per-volume protocol into the model's acquisition arrays) →
`validate_protocol` → construct → `validate_against_protocol`. Protocol
ingestion lives in that one place, so **every** model sources its acquisition
from BIDS identically — a model cannot be built skipping it, and there is no
per-model protocol-folding to get wrong. `describe` runs only the
config-intrinsic validation (no protocol), letting the BIDS shell read a
model's `protocol_schema()`/`bids_outputs()` before any data is resolved.
There is **no `match cfg.model { … }` scattered anywhere else** — adding a `ModelEntry`
here is the only wiring a new model needs.

---

## Data flow

**Every entry point converges on one path.** A file list, a BIDS collection, a qMRLab
`.mat` dataset, or a browser call each resolve to the same two things — a `Box<dyn Model>`
and identity-tagged volumes — and from there a single engine path runs. A new input source
is a new *front door*, never a new fitting path. *Instance:* the BIDS flow below differs
from the plain-file flow only in how it reaches a `Protocol` and an ordered volume set;
both feed the identical `build_volume_ids` → `engine::run`.

**Volumes are matched by identity, not position.** Each volume is tagged with what it *is*
— a role, or its acquisition parameters — before the engine assembles the `Measurement`,
so reordering the acquisition list yields identical fits, and a volume whose identity has
no match fails loudly rather than silently mis-assembling the signal.

### Fit (CLI)

```
YAML config ─► config::parse_config ─► (Config, raw Value)
   registry::by_name(cfg.model).build(raw, protocol) ─► Box<dyn Model>
   shell loads model.required_inputs() as 3-D maps ─► AuxMaps
   shell labels each data volume with a VolumeId (Role or Params)
   engine::run(model, data4d, volume_ids, mask, aux, progress) ─► FitResults (name → 3-D map)
   io::nifti writes each map
```

`engine::run` dispatches on `model.strategy()`; `run_voxelwise` fits masked, non-empty
voxels in parallel (`rayon`), assembling each voxel's per-volume values and their
`VolumeId`s into an identity-keyed `Measurement` (matching `model.measurement()`), building
a per-voxel `Aux`, and calling `model.fit_block` on blocks of voxels. There is no positional
signal slice anywhere in this path — a reordered volume list produces the same `Measurement`
and the same fit.

Blocking is a performance seam, not a semantic one: `fit_block` defaults to calling `fit`
per voxel, and a model that overrides it must return exactly what `fit` would. Its purpose
is to let a model amortize a shared read-only structure — a search grid, a dictionary —
across the block instead of re-sweeping it per voxel. If a block panics the engine re-runs
it voxel by voxel, so a single bad voxel is recorded as a failed fit rather than taking its
neighbours down with it.

### Fit from a BIDS dataset (CLI)

```
qmrust fit --bids-dir <dir> ─► StdFs (native DatasetFs) ─► rust_bids::collections_for
   for each Collection: resolve_protocol + load 4-D volumes
   resolve_aux_and_mask(table, model, identity, mask_spec) ─► AuxMaps + Option<mask>
   build_volume_ids(model.measurement(), protocol) ─► engine::run ─► FitResults
   io::nifti writes output_dir/qmrust/<subject>[/<session>]/<datatype>/<subject>[_<session>]_<Suffix>.nii.gz
```

A BIDS collection is just another way to arrive at a `Protocol` and an ordered volume
set, feeding the same order-free `build_volume_ids` → `engine::run` path as the
file-based flow above. `resolve_aux_and_mask` (`qmrust-cli/src/commands.rs`) resolves
each of the model's `required_inputs()` from the dataset's flat table by the
collection's full identity + declared BIDS suffix — found in raw *or* any
`derivatives/<pipeline>/` — and, separately, the brain mask declared under `--config`'s
`mask:` key (a suffix + entity constraints, e.g. `desc: brain`); an under-specified
`mask:` matching several files is a hard error rather than a silent pick, and no
`mask:` block means no masking. See [`DATA-PIPELINE.md`](DATA-PIPELINE.md) for how
collections are resolved and how sidecars are merged. Both collection shapes
are BIDS-fittable: a `Sequential` collection is re-identified from its resolved
`Protocol` by value; a `Named`/MTS-style collection is stacked in the model's
declared role order (its grouping `named_set` role names must match the model's
`measurement()` roles).

Output is written in the BIDS-derivatives convention too — `output_dir/qmrust/<subject>
[/<session>]/<datatype>/<subject>[_<session>]_<Suffix>.nii.gz`, per each model's declared
`bids_outputs()`, with `<datatype>` decided by the suffix itself
(`rust_bids::datatype_for_suffix`: `anat/` for a tissue parameter, `fmap/` for a field
map) rather than by the model — and `qmrust bidsify` is the reverse direction, turning a qMRLab `.mat`
or NIfTI dataset into a byte-identical BIDS input for this path. See [`DATA-PIPELINE.md`
](DATA-PIPELINE.md#6-the-output-side--bids_outputs-and-the-derivatives-layout) for both.

### Simulate (CLI / core)

`sim::{run_signal, run_single_voxel, run_sensitivity, run_montecarlo}` build a model
via the registry and call `model.forward` / `model.fit` directly with an `Aux` derived
from the `sim:` config block. Reports serialize to JSON.

### Browser (wasm)

```
JS ─► wasm.rs #[wasm_bindgen] wrapper ─► api.rs (pure) ─► qmrust-core
```

Exposed API: `list_models`, `fit_voxel`, `forward`, `fit_volume`, `sim`, plus
`init_thread_pool` (feature `threads`). Whole-volume `fit_volume` uses `rayon`, so it
requires the threaded build (`wasm-bindgen-rayon`); `fit_voxel`/`forward`/`sim` run on
the default single-threaded build. Acquisition parameters must be in the config YAML
passed to the API — there is no `.mat`/BIDS protocol source in the browser.

Every `ModelEntry`'s `Recipes.sim` is a compile-time obligation, not an `Option`: a
model cannot be registered without a sim recipe, so the playground's Simulate mode and
`crates/qmrust-core/tests/properties.rs` (which runs all four sim modes off each
model's declared recipe) both cover a model the moment it is added, with no per-model
line anywhere else. `NoiseKind` (`qmrust_core::sim::noise`) is the single home for the
names a `sim.noise.type` field accepts; `SimConfig::validate` delegates to it,
`qmrust catalog --json` emits the list top-level as `noise_kinds` (not per model), and
the playground reads that field out of `docs/playground/data/index.json` to populate
the noise-type dropdown rather than restating the list in JavaScript.

The playground (`docs/playground/`) drives `sim` through three modules kept apart by
what each owns: `sim.js` wires the Simulate page mode's controls and renders a report
into the chart and stats table; `sim-worker.js` holds a second, independent wasm
instance and runs the actual `sim(mode, yaml)` call off the main thread, because a
sweep of a few thousand fits would otherwise freeze the page and the RNG stream is
consumed sequentially across trials and sweep points, so the call cannot be chunked
without drifting from a native run using the same seed; `sim-series.js` is the pure
report-to-chart-series mapping (four modes in, `{ values, labels }`-shaped series out),
with no DOM and no wasm, which is what makes it testable under `node --test` the same
way `sim.js` and the worker cannot be.

---

## How a model is defined

A model is a directory under `crates/qmrust-core/src/models/<name>/` with three concerns
kept together:

1. **Config** (`config.rs`) — a `serde`-deserializable struct for the model's own YAML
   sub-tree, `Default`, with `validate_options()`/`validate_protocol()` methods.
   Each model owns its config; the top-level `Config` only knows the shared fields
   (`model`, `sim`) — there is no monolithic config struct listing every model's fields.
2. **Math** (`fit.rs`, and for qMT `lineshape.rs`/`ode.rs`/`pulse.rs`/`sf.rs`) — the pure
   signal equation and the fitter. No I/O, no config-file types.
3. **Adapter + builder** (`model.rs` / `adapter.rs`) — an `impl Model` that delegates to
   the math, an `impl ModelConfig` supplying the build-pipeline hooks, and one-line
   `build`/`describe` entry points the registry calls.

### Worked example — inversion recovery

```rust
// impl Model for IrModel  (delegates to the pure IrFitter)
fn param_names(&self)    -> Vec<&'static str> { IrFitter::param_names().to_vec() }   // [T1, a, b]
fn output_names(&self)   -> Vec<String>       { self.output_names.clone() }          // [T1, b, a, res, …]
fn required_inputs(&self)-> Vec<InputSpec>    { vec![] }                             // IR needs no aux maps
fn measurement(&self) -> MeasurementKind {
    // One {"InversionTime": ti} identity row per fitter TI, canonical order.
    MeasurementKind::Series { rows: ir_rows(&self.fitter) }
}
fn forward(&self, p: &[f64], _aux: &Aux) -> Measurement {
    // Tag each forward-simulated value with the TI that produced it.
    let samples = self.fitter.ti().iter().zip(self.fitter.forward(p[0], p[1], p[2]))
        .map(|(&ti, value)| Sample { params: BTreeMap::from([("InversionTime".into(), ti)]), value })
        .collect();
    Measurement::Series(samples)
}
fn fit(&self, m: &Measurement, _aux: &Aux) -> Vec<f64> {
    // Assemble the signal in the fitter's own TI order by matching each
    // expected TI to its sample BY VALUE — never by array position. An
    // unmatched TI panics rather than silently mis-assembling the signal.
    let samples = m.series();
    let signal: Vec<f64> = self.fitter.ti().iter()
        .map(|&ti| samples.iter()
            .find(|s| s.params.get("InversionTime") == Some(&ti))
            .map(|s| s.value)
            .unwrap_or_else(|| panic!("measurement has no sample with InversionTime={ti}")))
        .collect();
    self.fitter.fit_voxel(&Array1::from_vec(signal))
}
fn bids(&self) -> Option<BidsSpec> { Some(BidsSpec { suffix: "IRT1", entities: IR_ENTITIES }) }
fn protocol_schema(&self) -> Vec<ProtoParam> {
    // InversionTime comes straight off each volume's sidecar, one per volume.
    vec![ProtoParam { name: "InversionTime", source: Source::Field("InversionTime"), scope: Scope::PerVolume }]
}

// The config implements ModelConfig; build/describe/effective delegate to the
// shared pipeline (core::model::build_model / describe_model /
// effective_model). ingest_protocol is the only model-specific step — it
// folds the BIDS-resolved Protocol into the config's acquisition array; the
// shared driver handles the rest (validate options → ingest → validate
// protocol → construct → validate_against_protocol).
impl ModelConfig for IrConfig {
    const NAME: &'static str = "inversion_recovery";
    const SUBKEY: Option<&'static str> = None;                 // IR reads top-level keys
    // The config keys ingest_protocol overwrites, so a UI can lock them: a
    // resolved protocol always wins, so offering them as editable would
    // discard whatever the reader typed.
    const PROTOCOL_KEYS: &'static [&'static str] = &["inversion_times"];
    fn validate_options(&mut self) -> Result<()> { /* method, t1_range, zoom */ }
    fn ingest_protocol(&mut self, proto: &Protocol) -> Result<()> {
        // BIDS sidecars supply the InversionTimes here.
        if !proto.volumes.is_empty() { /* pull InversionTime values into self.inversion_times */ }
        Ok(())
    }
    fn validate_protocol(&mut self) -> Result<()> { /* ≥3 TIs, sort ascending */ }
    fn into_model(self) -> Box<dyn Model> { Box::new(IrModel::new(self)) }
}
pub fn build(v: &serde_yaml::Value, proto: &Protocol) -> Result<Box<dyn Model>> {
    crate::core::model::build_model::<IrConfig>(v, proto)
}
pub fn describe(v: &serde_yaml::Value) -> Result<Box<dyn Model>> {
    crate::core::model::describe_model::<IrConfig>(v)
}
pub fn effective(v: &serde_yaml::Value, proto: &Protocol) -> Result<EffectiveConfig> {
    crate::core::model::effective_model::<IrConfig>(v, proto)
}
```

qMT reads its config from a nested `qmt_spgr:` key, declares aux inputs with BIDS
locators, and reads a `Series` measurement keyed by `(Angle, Offset)` rather than TI:

```rust
fn required_inputs(&self) -> Vec<InputSpec> { vec![
    InputSpec { name: "R1map", required: false, bids: Some(BidsMap { suffix: "R1map",  entity: None }) },
    InputSpec { name: "B1map", required: false, bids: Some(BidsMap { suffix: "TB1map", entity: None }) },
    InputSpec { name: "B0map", required: false, bids: Some(BidsMap { suffix: "B0map",  entity: None }) },
]}
fn fit(&self, m: &Measurement, aux: &Aux) -> Vec<f64> {
    let b1 = aux.get("B1map").unwrap_or(1.0);   // shell supplied it; model just reads it
    // m.series() is matched to this model's protocol rows by (Angle, Offset), not position.
    /* … */
}
```

### The checklist to add a model

1. Create `crates/qmrust-core/src/models/<name>/` with `config.rs`, the math, and
   `model.rs` (`impl Model` + `pub fn build`).
2. Register the module in `models/mod.rs`.
3. Add **one** `ModelEntry` to `registry::all()` (name + BIDS suffix + `build` +
   `describe` + `dump` + `effective`). If the config has an acquisition axis
   `ingest_protocol` overwrites, declare it in `PROTOCOL_KEYS` too. It is what
   `build_model` reads to refuse a recipe that restates a resolved acquisition,
   and what the playground's recipe form reads to show those fields as the
   dataset's rather than as editable.
4. If the model introduces a new BIDS suffix, add a grouping block for it to
   `crates/rust-bids/src/default_grouping.yaml` (`sequential_set` or `named_set`)
   so `qmrust fit --bids-dir` can assemble its volumes; without it a fit of the
   new suffix errors unless the dataset supplies its own `--config` grouping.
5. Add unit tests (forward→fit round-trip; config parse/validate).

Nothing in `qmrust-cli`, `qmrust-wasm`, `engine`, or `config` needs to change. If the
model needs a new auxiliary input, declare it in `required_inputs()` — the CLI loads any
map it recognises by logical name, and the shell (not the core) owns where that data
comes from.

See [`ADDING-A-MODEL.md`](ADDING-A-MODEL.md) for a dense, checklist-first version of this
section (exact signatures, invariants, and the verification commands), and
[`docs/dev/adding-a-model.md`](../dev/adding-a-model.md) for the developer-facing guide.

---

## Modularity principles

- **One trait, one registry line.** The `Model` trait is the entire contributor surface;
  the registry is the entire dispatch surface. No per-model branching leaks elsewhere.
- **Functional core / imperative shell.** Pure math + trait in `core`; all I/O, CLI, and
  platform bindings in `cli`/`wasm`. The dependency arrow only points inward.
- **Core purity = portability.** Because the core avoids `std::fs`/`clap`/`matfile` on
  wasm, the browser build reuses the exact fitting code and produces identical numbers.
- **Each model owns its config.** Per-model `serde` structs parsed from the model's own
  YAML sub-tree; no monolithic config struct.
- **Inputs are declared, not hardcoded.** A model names the auxiliary inputs it needs;
  the shell finds and loads them. The compute layer only ever sees named scalars.
  *Instance:* qMT declares `R1map`/`B1map`/`B0map` with BIDS locators and reads them by
  name — no R1/B1/B0 list exists anywhere in the shell.
- **A derived artifact travels with the frame it was built in.** When the shell turns
  data into an artifact and later feeds that artifact back to transform data, both
  directions must run through the *same* computation, and the artifact must carry the
  parameters that fix its frame of reference. Nothing in the types catches an artifact
  calibrated in one frame and applied in another — so the reuse is made structural, not
  hoped for. *Instance:* `mt_sat`'s B1-correction surface is built and applied by the one
  `mt_sat` computation, and its `FitValues` carries the `b1_ref` and sequence parameters
  it was calibrated at.
- **Behaviour-preserving by contract.** A refactor is only correct if it leaves the fit
  outputs byte-identical. *Instance:* the CI OSF job re-runs the real pipelines end-to-end
  and diffs the maps.
- **Seams over speculation (YAGNI).** Leave a typed seam where a capability is foreseeable
  but unbuilt — not speculative machinery. *Instance:* `FitStrategy::MatrixWise` is a
  `bail!` seam until a model needs it; the BIDS sidecar→`Protocol` path began as a seam and
  is now the `rust-bids` crate.

---

## BIDS-first design

**Acquisition metadata is data, not configuration.** A model reads its acquisition from
the dataset — flip angles, inversion times, offsets already live in JSON sidecars — so
`--config` carries algorithm options and the non-BIDS fallback only, never a second copy
of numbers the data already holds.

**BIDS is an imperative-shell concern; the core never sees it.** A model only *declares*
its BIDS identity and metadata mapping (`bids()`, `InputSpec.bids`, `protocol_schema()`);
the shell (`rust-bids` + the CLI) resolves those declarations into a `Protocol` before
`build`, and `forward`/`fit` still see only ordered params + `Aux`. That is what keeps one
copy of the fitting code pure and portable. *Instance:* `qmrust fit --bids-dir` groups the
dataset and folds each collection's sidecars into a `Protocol`; `--mat-dir`/`--mat-data`
reach the same path after `qmrust bidsify` converts qMRLab `.mat` data to BIDS.

**Known terms are resolved, never guessed.** The layout is matched against
`rust_bids::Vocabulary` — canonical BIDS entities and suffixes from the spec, plus every
registered model's `bids_suffix` at compile time, plus a dataset's own declared
`custom_entities`/`custom_suffixes`. *Instance:* `QMTSPGR` is recognized with no config
because it is a registered suffix, while a lab's non-official suffix is recognized only if
the dataset itself declares it. See [`DATA-PIPELINE.md`](DATA-PIPELINE.md) for the full
mapping mechanism, the `rust-bids` crate, and what's deferred.

---

## Units

qmrust is BIDS-native (SI): time in seconds, frequency in Hz, field in tesla — see the
"Units — BIDS-native (SI)" principle in [`CLAUDE.md`](../../CLAUDE.md) for the full rule
and the qMRLab (ms) divergence it implies. Not restated here to avoid drift.

---

## Building, testing, and the CI gates

```bash
cargo build --workspace                                   # native build
cargo test  --workspace                                   # all crates' tests
cargo fmt --all --check                                   # format gate
cargo clippy --workspace --all-targets -- -D warnings     # lint gate
cargo build -p qmrust-cli --release                       # the qmrust binary
cargo build -p qmrust-core --target wasm32-unknown-unknown  # core is wasm-clean
```

The threaded browser build is nightly-only (it rebuilds `std` with atomics):

```bash
# see crates/qmrust-wasm/README.md for the full RUSTFLAGS recipe + COOP/COEP note
wasm-pack build crates/qmrust-wasm --target web --features threads -- -Z build-std=std,panic_abort
```

CI (`.github/workflows/ci.yml`) runs four jobs: **lint** (fmt + clippy), **native**
(test + release binary), **wasm** (threaded `wasm-pack` build + headless-browser test),
and **integration-osf** (downloads qMRLab's datasets from OSF and runs the real fit
pipelines). Large test fixtures are fetched from OSF, never committed. A separate
`.github/workflows/docs.yml` builds the MyST human-docs site under `docs/` and deploys it
to GitHub Pages on changes there.
