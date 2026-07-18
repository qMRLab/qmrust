# qmrust

qmrust is a native-Rust toolkit for quantitative MRI (qMRI) model fitting. The
numerical core is written once, as pure functions, and runs unchanged in a
terminal (as a fast CLI) and in a browser tab (compiled to WebAssembly) — so a
fit you run locally and a fit run client-side in a web app produce identical
numbers.

## The workspace at a glance

qmrust is a four-crate Cargo workspace:

| Crate | Role |
|---|---|
| `qmrust-core` | Pure functional core — the `Model` trait, per-model math, the registry, the fitting engine, simulation. Compiles to `wasm32-unknown-unknown`. |
| `qmrust-cli` | The `qmrust` binary — CLI, `.mat`/NIfTI file I/O, progress bars. |
| `qmrust-wasm` | Browser `cdylib` — `wasm-bindgen` bindings over the core. |
| `rust-bids` | A wasm-clean BIDS layout resolver that groups qMRI datasets into fittable collections. |

The dependency arrow only ever points inward, into `qmrust-core`; the core
never touches files, a CLI framework, or JavaScript. See
[architecture](architecture.md) for why that separation matters.

## Where to go next

- New to the project? Start with [Getting started](getting-started.md) to
  build the CLI and run your first fit or simulation.
- Curious how it's put together? [Architecture](architecture.md) covers the
  functional-core / imperative-shell split in a few paragraphs.
- Want to add a model? [Models](models.md) is a short contributor guide.
- Working with BIDS datasets? [BIDS](bids.md) explains the `rust-bids`
  resolver.
- Interested in the browser build? See [Browser & wasm](browser.md).
