# qmrust architecture

Native-Rust quantitative-MRI fitting, structured as a **functional core / imperative
shell** so that the numerical models are pure and portable (they compile to
WebAssembly unchanged) while all I/O, CLI, and platform glue live at the edges.

The guiding goal: **you contribute a model by writing one module and adding one line
to a registry** — no edits scattered across the CLI, the config parser, the engine,
or the simulator.

---

## The workspace: three crates

```
qmrust/                         Cargo workspace
├── crates/
│   ├── qmrust-core/   ── FUNCTIONAL CORE ──  pure; no I/O; compiles to wasm32
│   ├── qmrust-cli/    ── IMPERATIVE SHELL ─  the `qmrust` binary: files, CLI, progress
│   └── qmrust-wasm/   ── IMPERATIVE SHELL ─  the browser cdylib: wasm-bindgen bindings
├── prots/                       example protocol / sim configs (YAML)
├── ci/integration_osf.sh        end-to-end fit against qMRLab's OSF datasets
└── .github/workflows/ci.yml     lint · native · wasm · integration
```

**Dependency direction is strict and one-way:**

```
qmrust-cli  ─┐
             ├──►  qmrust-core   (core depends on NEITHER)
qmrust-wasm ─┘
```

`qmrust-core` never depends on `qmrust-cli` or `qmrust-wasm`, never touches `std::fs`
on the wasm target, and never pulls in `clap`, `nifti`, `matfile`, `indicatif`, or
`owo-colors`. That purity is what lets the exact same fitting/simulation code run in a
terminal and in a browser tab with identical numerical results.

### `qmrust-core` — the functional core

```
crates/qmrust-core/src/
├── core/model.rs      the Model trait + value types (the contributor surface)
├── models/            per-model math + config + Model impl + builder
│   ├── inversion_recovery/{config,fit,model}.rs
│   └── qmt_spgr/{config,fit,adapter,lineshape,ode,pulse,sf}.rs
├── registry.rs        name / BIDS-suffix → builder  (the one dispatch point)
├── engine.rs          the parallel voxel-fitting engine (FitStrategy)
├── sim/               forward signal, noise, sim→fit round-trips, reports
├── config.rs          parse_config(&str) → (Config, Value)   (parsing only, no fs)
├── protocol.rs        ProtocolSource → Protocol
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
`dump-sf`.

### `qmrust-wasm` — the browser shell

A `cdylib` exposing the core to JavaScript via `wasm-bindgen`. Two layers:

- `api.rs` — **pure** marshalling (`&str` config, typed slices, JSON aux, results).
  Unit-tested on the **native** target, so its correctness is verified without a browser.
- `wasm.rs` — thin `#[wasm_bindgen]` wrappers (compiled only for `wasm32`) that convert
  JS values and call `api`.

`wasm-bindgen`, `js-sys`, `serde-wasm-bindgen`, and `wasm-bindgen-rayon` are
**wasm-target-only** dependencies — they never enter the native build.

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
    fn n_acquisitions(&self) -> usize;            // expected number of volumes

    fn strategy(&self) -> FitStrategy { FitStrategy::Voxelwise }   // fit granularity

    fn forward(&self, params: &[f64], aux: &Aux) -> Vec<f64>;      // noise-free signal
    fn fit(&self, signal: &[f64], aux: &Aux) -> Vec<f64>;          // signal → outputs

    fn bids(&self) -> Option<BidsSpec> { None }   // BIDS grouping suffix + entity map
}
```

The core never sees files, JSON, typed arrays, or config formats — only ordered `f64`
slices and a scalar `Aux` bundle. That is the whole reason it is portable.

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
- **`Protocol { volumes, global }`** — a BIDS-sidecar-shaped acquisition protocol (one
  metadata map per volume + shared globals). Empty means "model, read your protocol from
  your own config." Produced by `ProtocolSource` (YAML config today; `.mat` overrides;
  BIDS sidecars are the next source).
- **`BidsSpec { suffix, entities }`** — the model's BIDS identity (e.g. `IRT1`, `MTS`).

---

## The registry — the one dispatch point

`registry.rs` is the single place that maps a model name (and a BIDS suffix) to the
function that builds it:

```rust
pub type Builder = fn(&serde_yaml::Value, &Protocol) -> Result<Box<dyn Model>>;

pub struct ModelEntry { pub name: &'static str, pub bids_suffix: &'static str, pub build: Builder }

pub fn all() -> &'static [ModelEntry] { &[
    ModelEntry { name: "inversion_recovery", bids_suffix: "IRT1", build: models::inversion_recovery::build },
    ModelEntry { name: "qmt_spgr",           bids_suffix: "MTS",  build: models::qmt_spgr::build },
]}

pub fn by_name(name: &str) -> Option<&'static ModelEntry>;
pub fn by_bids_suffix(suffix: &str) -> Option<&'static ModelEntry>;
```

The CLI, the simulator, and the wasm bindings all resolve models through `by_name`.
There is **no `match cfg.model { … }` scattered anywhere else** — adding a `ModelEntry`
here is the only wiring a new model needs.

---

## Data flow

### Fit (CLI)

```
YAML config ─► config::parse_config ─► (Config, raw Value)
   registry::by_name(cfg.model).build(raw, protocol) ─► Box<dyn Model>
   shell loads model.required_inputs() as 3-D maps ─► AuxMaps
   engine::run(model, data4d, mask, aux, progress) ─► FitResults (name → 3-D map)
   io::nifti writes each map
```

`engine::run` dispatches on `model.strategy()`; `run_voxelwise` fits masked, non-empty
voxels in parallel (`rayon`), building a per-voxel `Aux` and calling `model.fit`.

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

---

## How a model is defined

A model is a directory under `crates/qmrust-core/src/models/<name>/` with three concerns
kept together:

1. **Config** (`config.rs`) — a `serde`-deserializable struct for the model's own YAML
   sub-tree, with a `validate()` method. Each model owns its config; the top-level
   `Config` only knows the shared fields (`model`, `sim`) — there is no monolithic config
   struct listing every model's fields.
2. **Math** (`fit.rs`, and for qMT `lineshape.rs`/`ode.rs`/`pulse.rs`/`sf.rs`) — the pure
   signal equation and the fitter. No I/O, no config-file types.
3. **Adapter + builder** (`model.rs` / `adapter.rs`) — an `impl Model` that delegates to
   the math, and a `build` function the registry calls.

### Worked example — inversion recovery

```rust
// impl Model for IrModel  (delegates to the pure IrFitter)
fn param_names(&self)    -> Vec<&'static str> { IrFitter::param_names().to_vec() }   // [T1, a, b]
fn output_names(&self)   -> Vec<String>       { self.output_names.clone() }          // [T1, b, a, res, …]
fn required_inputs(&self)-> Vec<InputSpec>    { vec![] }                             // IR needs no aux maps
fn n_acquisitions(&self) -> usize             { self.n_ti }
fn forward(&self, p: &[f64], _aux: &Aux) -> Vec<f64> { self.fitter.forward(p[0], p[1], p[2]) }
fn fit(&self, sig: &[f64], _aux: &Aux)   -> Vec<f64> { self.fitter.fit_voxel(&Array1::from(sig)) }
fn bids(&self) -> Option<BidsSpec> { Some(BidsSpec { suffix: "IRT1", entities: IR_ENTITIES }) }

// the registry builder: parse this model's config, apply any protocol override, validate, box it
pub fn build(v: &serde_yaml::Value, proto: &Protocol) -> Result<Box<dyn Model>> {
    let mut cfg: IrConfig = serde_yaml::from_value(v.clone())?;
    // e.g. a .mat file may override inversion times via the resolved Protocol
    if !proto.volumes.is_empty() { /* pull InversionTime values from proto */ }
    cfg.validate()?;
    Ok(Box::new(IrModel::new(cfg)))
}
```

qMT reads its config from a nested `qmt_spgr:` key and declares aux inputs with BIDS
locators:

```rust
fn required_inputs(&self) -> Vec<InputSpec> { vec![
    InputSpec { name: "R1map", required: false, bids: Some(BidsMap { suffix: "T1map",  entity: None }) },
    InputSpec { name: "B1map", required: false, bids: Some(BidsMap { suffix: "TB1map", entity: None }) },
    InputSpec { name: "B0map", required: false, bids: Some(BidsMap { suffix: "B0map",  entity: None }) },
]}
fn fit(&self, signal: &[f64], aux: &Aux) -> Vec<f64> {
    let b1 = aux.get("B1map").unwrap_or(1.0);   // shell supplied it; model just reads it
    /* … */
}
```

### The checklist to add a model

1. Create `crates/qmrust-core/src/models/<name>/` with `config.rs`, the math, and
   `model.rs` (`impl Model` + `pub fn build`).
2. Register the module in `models/mod.rs`.
3. Add **one** `ModelEntry` to `registry::all()` (name + BIDS suffix + `build`).
4. Add unit tests (forward→fit round-trip; config parse/validate).

Nothing in `qmrust-cli`, `qmrust-wasm`, `engine`, or `config` needs to change. If the
model needs a new auxiliary input, declare it in `required_inputs()` — the CLI loads any
map it recognises by logical name, and the shell (not the core) owns where that data
comes from.

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
- **Inputs are declared, not hardcoded.** Models declare auxiliary inputs (with BIDS
  locators); the shell resolves them. The compute layer only ever sees named scalars.
- **Behaviour-preserving by contract.** Refactors are validated against byte-identical
  fit outputs (the CI OSF job runs the real pipelines end-to-end).
- **Seams over speculation (YAGNI).** `FitStrategy::MatrixWise` and the BIDS
  `Protocol`/`ProtocolSource` are declared seams, implemented when a real consumer needs
  them, not before.

---

## BIDS-first design

qMRI-BIDS is treated as an **imperative-shell concern**, so it never touches the pure
core. A model declares its BIDS identity (`bids()` → suffix + entities) and its inputs'
BIDS locators (`InputSpec.bids`); the shell uses those to locate a file collection and
read acquisition metadata from JSON sidecars into a `Protocol`, which is handed to the
model's `build`. The model's `forward`/`fit` still only see ordered params + `Aux`. The
seams (`BidsSpec`, `Protocol`, `ProtocolSource`) are in place; the sidecar reader is the
next increment.

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
pipelines). Large test fixtures are fetched from OSF, never committed.
