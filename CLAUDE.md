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
- `crates/rust-bids` — **wasm-clean, standalone qMRI-BIDS layout resolver**: flat-table
  parse → declarative grouping (`BidsConfig`) → `Collection`, with all I/O behind the
  `DatasetFs` trait; bridges a collection's sidecars to `qmrust_core::Protocol`. A consumer
  of core, not part of it (and generalizable beyond this workspace).

## The one rule that must never break

**`qmrust-core` stays pure.** It must NOT:
- depend on `qmrust-cli`, `qmrust-wasm`, or `rust-bids` (dependency arrow points inward only);
- use `clap`, `nifti`, `indicatif`, `owo-colors`;
- use `std::fs` or `matfile` on the wasm target (they're gated `#[cfg(not(target_arch = "wasm32"))]`).

If you need file/CLI/JS behaviour, it belongs in `qmrust-cli` or `qmrust-wasm`, not core.
Verify with: `cargo build -p qmrust-core --target wasm32-unknown-unknown`.

## Clean codebase — a hard principle

**Clean context, clean code. No garbage in the repo.** Every commit leaves the tree free of:
- dead code, commented-out blocks, speculative stubs, or unused scaffolding;
- internal development-phase codenames or process references (e.g. "Plan A/Plan B", task
  numbers, "the next increment") — write what the code *is*, not the story of how it got here;
- stale or confusing mentions. Rename or supersede something and you update **every**
  reference and delete what it replaced — no orphaned names, no duplicated sources of truth.

Keep docs in lockstep with the code: `docs/agents/ARCHITECTURE.md` must always match the
current design, and the human docs under `docs/` follow **progressive disclosure** — lead with
the essential what/why, then details. When in doubt, delete rather than keep "just in case".

**Comments explain the code, not its author.** A comment or docstring states what the code
does and the invariant or contract behind it, from the first principles of the domain — never
the decision-making, alternatives weighed, or the narrative of how the code came to be. No
"I chose X", no "this used to be Y", no "note: tricky because earlier…", no reviewer-directed
asides, no task/plan references. Write for whoever reads this line a year from now: they need
the *what* and the *why it must be so*, not the story of how it got written. If a comment would
only make sense to someone who watched it being written, delete it.

## Adding a model (the common task)

1. New dir `crates/qmrust-core/src/models/<name>/`: `config.rs` (a `serde` struct +
   `validate()`), the pure math, and `model.rs` (`impl Model` + `pub fn build`).
2. Register it in `models/mod.rs`.
3. Add **one** `ModelEntry` to `registry::all()` in `registry.rs` (name + BIDS suffix + `build`).
4. Add tests (forward→fit round-trip; config parse/validate).

That's it — do **not** add `match cfg.model` branches in the CLI, engine, sim, or config.
The `Model` trait is the whole contributor surface; the registry is the whole dispatch
point. Auxiliary inputs (B1/B0/R1, …) are *declared* via `required_inputs()`; the shell
loads them and the model reads scalars via `aux.get("B1map")`. A model declares its
measurement shape via `measurement() -> MeasurementKind` (`Named { roles }` for a fixed
set of role-labeled volumes, or `Series { rows }` for a variable-length series with its
own canonical per-volume identity rows), and `forward`/`fit` read the identity-keyed
`Measurement` they're handed by identity — `m.role("MTw")` / `s.params["InversionTime"]`
— never by position. The engine assembles that keyed `Measurement` from the shell's
per-volume `VolumeId`s; `build` validates the supplied `Protocol` against the model's own
declared measurement (`validate_against_protocol`), failing loudly at build rather than
per-voxel. If the model's protocol (e.g. `InversionTime`) can be read from a BIDS JSON
sidecar, declare it via `protocol_schema() -> Vec<ProtoParam>` — `Source::Field(key)` for
a value read straight off the sidecar, `Source::Derived(fn(&dyn Meta) -> Result<f64>)`
for one computed from several sidecar fields (a pure, image-scoped fn, not a closure), or
`Source::Option(key)` for a non-BIDS fallback read from `--config`. The shell
(`rust-bids::resolve_protocol`) evaluates the schema against each image's
inheritance-merged `Sidecar` into the `Protocol`; `protocol_schema()` defaults to `vec![]`
so this is opt-in — a model that skips it just reads its own `--config` as before, and
`--config` for a migrated model narrows to algorithm options plus the `Source::Option`
fallback. Use IR (`models/inversion_recovery/`) as the minimal reference — its
`protocol_schema()` maps `InversionTime`; qMT (`models/qmt_spgr/`) shows a nested-config
model with aux inputs (its own protocol mapping is a deferred follow-up).

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
cargo run -p qmrust-cli -- fit  --bids-dir <dir> --config prots/<cfg>.yaml --output-dir <out>  # v1: no-aux, sequential (e.g. IRT1)
cargo run -p qmrust-cli -- sim  single-voxel --config prots/<cfg>.yaml --output <out>.json
cargo build -p qmrust-core --target wasm32-unknown-unknown   # core must stay wasm-clean
cargo build -p rust-bids   --target wasm32-unknown-unknown   # rust-bids must stay wasm-clean too
cargo test  -p qmrust-wasm --target wasm32-unknown-unknown --no-run  # wasm bindings + browser tests must compile (CI runs them in a headless browser)
```

Before claiming work is done: `cargo test --workspace`, `cargo fmt --all --check`, and
`cargo clippy --workspace --all-targets -- -D warnings` must all pass.

## Notes

- Large test data is **not** committed; CI fetches qMRLab's datasets from OSF
  (`ci/integration_osf.sh`). Locally you supply your own `--mat-dir`/`--mat-data`.
- Config files live in `prots/`; the browser build's API + build recipe are documented in
  `crates/qmrust-wasm/README.md`.
