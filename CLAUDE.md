# CLAUDE.md

Guidance for working in this repo. Read [`docs/agents/ARCHITECTURE.md`](docs/agents/ARCHITECTURE.md)
for the full design; this file is the operational quick-reference.

## What this is

Native-Rust quantitative-MRI fitting, built as a **functional core / imperative shell**
Cargo workspace so the numerical models are pure and run identically in a terminal and in
a browser (WebAssembly).

## Workspace map

- `crates/qmrust-core` — **pure functional core**: the `Model` trait, per-model math,
  the registry, the fitting engine, simulation, config *parsing*. Compiles to
  `wasm32-unknown-unknown`.
- `crates/qmrust-cli` — the `qmrust` binary: CLI, file I/O (`.mat`/NIfTI), progress.
- `crates/qmrust-wasm` — browser `cdylib`: `wasm-bindgen` bindings over the core.

## The one rule that must never break

**`qmrust-core` stays pure.** It must NOT:
- depend on `qmrust-cli` or `qmrust-wasm` (dependency arrow points inward only);
- use `clap`, `nifti`, `indicatif`, `owo-colors`;
- use `std::fs` or `matfile` on the wasm target (they're gated `#[cfg(not(target_arch = "wasm32"))]`).

If you need file/CLI/JS behaviour, it belongs in `qmrust-cli` or `qmrust-wasm`, not core.
Verify with: `cargo build -p qmrust-core --target wasm32-unknown-unknown`.

## Adding a model (the common task)

1. New dir `crates/qmrust-core/src/models/<name>/`: `config.rs` (a `serde` struct +
   `validate()`), the pure math, and `model.rs` (`impl Model` + `pub fn build`).
2. Register it in `models/mod.rs`.
3. Add **one** `ModelEntry` to `registry::all()` in `registry.rs` (name + BIDS suffix + `build`).
4. Add tests (forward→fit round-trip; config parse/validate).

That's it — do **not** add `match cfg.model` branches in the CLI, engine, sim, or config.
The `Model` trait is the whole contributor surface; the registry is the whole dispatch
point. Auxiliary inputs (B1/B0/R1, …) are *declared* via `required_inputs()`; the shell
loads them and the model reads scalars via `aux.get("B1map")`. Use IR
(`models/inversion_recovery/`) as the minimal reference; qMT (`models/qmt_spgr/`) shows a
nested-config model with aux inputs.

## Invariants to respect

- **Object safety.** `Model` is used as `Box<dyn Model>` — no generics/associated types on
  the trait.
- **Behaviour-preserving refactors.** Fitting output must not drift; validate against the
  fixtures (the CI OSF job runs the real pipelines). When in doubt, diff output maps.
- **Threaded wasm is behind the `threads` feature** (nightly + `wasm-bindgen-rayon`). Do
  NOT put its atomics/`build-std` flags in a committed `.cargo/config.toml` — that breaks
  the stable native and default-wasm builds. Keep them in the wasm CI job only.
- **Each model owns its config**; the top-level `Config` holds only shared fields
  (`model`, `sim`).

## Commands

```bash
cargo build --workspace
cargo test  --workspace
cargo fmt --all --check                                 # CI format gate
cargo clippy --workspace --all-targets -- -D warnings   # CI lint gate (must be clean)
cargo run -p qmrust-cli -- fit  --mat-dir <dir> --config prots/<cfg>.yaml --output-dir <out>
cargo run -p qmrust-cli -- sim  single-voxel --config prots/<cfg>.yaml --output <out>.json
cargo build -p qmrust-core --target wasm32-unknown-unknown   # core must stay wasm-clean
```

Before claiming work is done: `cargo test --workspace`, `cargo fmt --all --check`, and
`cargo clippy --workspace --all-targets -- -D warnings` must all pass.

## Notes

- Large test data is **not** committed; CI fetches qMRLab's datasets from OSF
  (`ci/integration_osf.sh`). Locally you supply your own `--mat-dir`/`--mat-data`.
- Config files live in `prots/`; the browser build's API + build recipe are documented in
  `crates/qmrust-wasm/README.md`.
