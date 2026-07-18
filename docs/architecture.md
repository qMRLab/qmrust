# Architecture

qmrust is built as a **functional core / imperative shell**: the numerical
model code is pure (no files, no CLI, no JavaScript) and lives in one crate,
`qmrust-core`, which compiles to `wasm32-unknown-unknown` as well as native.
Everything platform-specific — reading `.mat`/NIfTI files, parsing CLI
arguments, drawing a progress bar, talking to JavaScript — lives at the edges,
in `qmrust-cli` and `qmrust-wasm`. That split is what lets the exact same
fitting/simulation code run identically in a terminal and in a browser tab.

## The one rule

`qmrust-core` must never depend on `qmrust-cli` or `qmrust-wasm`, and must
never use `clap`, `nifti`, `indicatif`, `owo-colors`, or `std::fs`/`matfile` on
the wasm target. The dependency arrow only ever points inward:

```
qmrust-cli   ─┐
qmrust-wasm  ─┼──►  qmrust-core
rust-bids    ─┘
```

## The contributor surface

Everything a model needs to provide lives in one object-safe trait,
`Model` — forward signal, fit, parameter names/bounds, and which auxiliary
inputs (B1/B0/R1, …) it needs. A single registry (`registry::all()`) maps a
model name and BIDS suffix to a builder function; the CLI, simulator, and
wasm bindings all resolve models through it. There is no `match cfg.model`
scattered through the codebase — adding a model is one module plus one
registry line. See [Models](models.md) for the contributor checklist.

## Fourth crate: `rust-bids`

`rust-bids` is a standalone, wasm-clean BIDS layout resolver that groups raw
dataset files into fittable `Collection`s and can build a `qmrust_core::Protocol`
from BIDS sidecars. It depends on `qmrust-core` (as a consumer, like the CLI
and wasm crates do) but is not part of the core itself. See [BIDS](bids.md).

## Going deeper

This page is intentionally short. For the full design — the `Model` trait in
detail, the registry, the engine's fit strategies, per-crate directory maps,
and worked examples of adding a model — read
[`docs/agents/ARCHITECTURE.md`](agents/ARCHITECTURE.md).
