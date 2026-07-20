# qmrust

qmrust is a native-Rust toolkit for quantitative MRI (qMRI) model fitting.
Point it at a BIDS dataset and it fits qMRI models (like inversion recovery or
qMT-SPGR) to your data — as a fast command-line tool, or directly in the
browser via WebAssembly, with identical numbers either way.

**New here? Start with [Getting started](getting-started.md)** to build the
CLI and run your first fit or simulation.

## Where to go next

- New to the project? Start with [Getting started](getting-started.md) to
  build the CLI and run your first fit or simulation.
- Working with BIDS datasets? [BIDS](bids.md) explains the `rust-bids`
  resolver.
- Interested in the browser build? See [Browser & wasm](browser.md).

Those three pages make up the **User Guide**.

## Under the hood

qmrust is built as a functional core / imperative shell — the numerical
models are pure Rust and run unchanged natively and in wasm. If you're
contributing code or just curious how it fits together, see the
**Developer Guide**: [Architecture](architecture.md) and [Models](models.md).
