# qmrust

qmrust is a native-Rust toolkit for quantitative MRI. Point it at a BIDS
dataset and it fits qMRI models — inversion-recovery T1, multi-echo T2, MTR,
MTsat, qMT-SPGR — as a fast command-line tool or directly in your browser via
WebAssembly, with identical numbers either way.

```bash
qmrust fit --bids-dir ds-mydata --config recipes/bids/irt1_config.yaml \
  --output-dir out
```

**New here?** [Getting started](getting-started.md) builds the CLI and runs
your first fit. **Curious first?** The [playground](playground.md) fits real
data in this page, in your browser.

## Methods

<!-- BEGIN generated: model gallery -->
<!-- END generated: model gallery -->

## Guides

- [Getting started](getting-started.md) — build the CLI, run a fit, read the outputs.
- [BIDS](guide/bids.md) — how collections, sidecars, auxiliary maps and masks are resolved.
- [Fitting without BIDS](guide/non-bids.md) — 4D NIfTI and qMRLab `.mat` inputs.
- [Simulation](guide/simulation.md) — forward signals, sim→fit round-trips, sensitivity.
- [Browser & wasm](guide/browser.md) — the same core, compiled to WebAssembly.

## Contributing

qmrust is a functional core with an imperative shell: the numerical models are
pure Rust and run unchanged natively and in wasm. See
[Architecture](dev/architecture.md) and [Adding a model](dev/adding-a-model.md).
