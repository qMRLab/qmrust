# Browser & wasm

Because `qmrust-core` is pure and platform-agnostic, the same fitting and
simulation code that powers the CLI also compiles to WebAssembly and runs in a
browser tab. `qmrust-wasm` is the crate that makes that available to
JavaScript.

## What runs in the browser

`qmrust-wasm` is a `cdylib` split into two layers:

- **`api.rs`** — pure marshalling between plain Rust/JSON types and
  `qmrust-core` (`&str` config, typed slices, JSON aux, results). It has no
  `wasm_bindgen` in it, so it's unit-tested on the native target.
- **`wasm.rs`** — a thin `#[wasm_bindgen]` layer, compiled only for
  `wasm32`, that converts JS-shaped values (typed arrays, `JsValue`, JSON
  strings) and calls into `api`.

Exposed functions: `list_models`, `fit_voxel`, `forward`, `fit_volume`, `sim`,
plus `init_thread_pool` (behind the `threads` feature). `fit_voxel`/`forward`/
`sim` run single-threaded by default; whole-volume `fit_volume` uses `rayon`
and needs the threaded build (`wasm-bindgen-rayon`, nightly + `build-std`).

One important constraint carries over from the core's purity: there is no
`.mat`/BIDS protocol source in the browser, so acquisition parameters (e.g.
inversion times, qMT protocol rows) must be given explicitly in the config
YAML string passed to every `api`/`wasm` call. See
[`crates/qmrust-wasm/README.md`](../crates/qmrust-wasm/README.md) for the full
function-by-function API reference and build recipe.

## Planned: a browser UI

`qmrust-wasm` is a library, consumed however a downstream JS project wires it
up; this repository ships no browser UI. The intended direction is one built on
the existing seams: a browser page (for example a Tauri app) where someone drags
in a BIDS dataset, `rust-bids` (see [BIDS](bids.md)) resolves it client-side
through the `DatasetFs` seam against a JS/Tauri directory listing, and
`qmrust-wasm`'s `fit_volume`/`sim` fit it without a server round-trip.
