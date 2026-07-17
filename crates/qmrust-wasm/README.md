# qmrust-wasm

Browser (WebAssembly) bindings for `qmrust` quantitative-MRI model fitting.

The crate is split into two layers:

- **`api`** — pure Rust marshalling between plain Rust/JSON types and
  `qmrust-core`. No `wasm_bindgen` involved, so every function is
  unit-tested on the native target (`cargo test -p qmrust-wasm`).
- **`wasm`** — a thin `#[wasm_bindgen]` layer (compiled only for
  `wasm32`) that converts JS-shaped values (typed arrays, `JsValue`,
  JSON strings) and delegates to `api`.

## API

All functions are free functions exported from the wasm module (i.e.
`import { fit_voxel, ... } from "qmrust-wasm"` after building with
`wasm-pack`).

### `list_models() -> JsValue`

Returns a JS array of registered model names (e.g. `["inversion_recovery",
"qmt_spgr", ...]`).

### `fit_voxel(cfg_yaml: &str, signal: &[f64], aux_json: &str) -> Vec<f64>`

Fits a single voxel's signal against the model named in `cfg_yaml` (a YAML
config string, same format as the native CLI). `signal` is the measured
signal (Float64Array on the JS side). `aux_json` is a JSON object of
scalar auxiliary inputs, e.g. `{"B1map": 1.2}`; pass `""` if the model
needs none. Returns fitted parameter values in the model's
`output_names` order.

> **Acquisition parameters must be in `cfg_yaml`.** In the browser there
> is no `.mat`/BIDS sidecar to read protocol information from, so
> anything the native CLI would otherwise pull from acquisition
> metadata — inversion times, qMT protocol rows/timing, etc. — must be
> given explicitly in the YAML config string passed to every `api`/`wasm`
> function.

### `forward(cfg_yaml: &str, params: &[f64], aux_json: &str) -> Vec<f64>`

The inverse of `fit_voxel`: computes the noise-free forward signal for
`params` (in the model's `param_names` order).

### `fit_volume(cfg_yaml: &str, data: &[f64], dims: &[usize], mask: Option<Vec<u8>>, aux_json: &str) -> JsValue`

Fits every voxel in a volume.

- `data` — the full volume, flattened in **C order** as `[nx, ny, nz,
  nt]` (i.e. the `nt`/measurement axis varies fastest).
- `dims` — exactly `[nx, ny, nz, nt]` (length 4; any other length is a
  hard error).
- `mask` — optional, flattened `[nx, ny, nz]`, one `u8` per voxel;
  nonzero means "fit this voxel". Omit (`undefined`/`null` on the JS
  side) to fit every voxel.
- `aux_json` — a JSON object mapping an auxiliary input name to a flat,
  C-order `[nx, ny, nz]` number array, e.g. `{"B1map": [...]}`. Pass
  `""` for no auxiliary maps.

Returns a plain JS object `{ [outputName: string]: number[] }`, one
C-order `[nx, ny, nz]` array per model output (e.g. `T1`, `T2`).
Unfitted voxels (outside the mask) are `NaN`.

> **Requires the threaded build.** `fit_volume` parallelizes across
> voxels with rayon, which needs a running wasm thread pool. It only
> works in the `threads` build (see "Two builds" below) after calling
> `initThreadPool`. The default single-threaded build supports
> `fit_voxel`, `forward`, and `sim` only — calling `fit_volume` there
> will fail (no thread pool) or hang.

### `sim(mode: &str, cfg_yaml: &str) -> String`

Runs a simulation and returns its report as a JSON string. `mode` is one
of `"signal"`, `"single-voxel"`, `"sensitivity"`, `"montecarlo"`; `cfg_yaml`
is the same YAML config format used by the native `qmrust sim` CLI
(including its `sim:` section).

### `initThreadPool` (feature `threads` only)

Re-exported from `wasm-bindgen-rayon`. See "Threaded build" below.

All fallible functions throw a JS `Error` (mapped from `JsError`) on
failure rather than returning a Rust `Result`; wrap calls in `try/catch`
on the JS side.

## Data-layout contract

- Volumes and masks are always **flat, C-order** arrays: for `dims =
  [nx, ny, nz, nt]`, index `(x, y, z, t)` lives at
  `((x * ny + y) * nz + z) * nt + t`, and spatial-only arrays (masks,
  aux maps, output maps) use `((x * ny + y) * nz + z)`.
- Masks are `u8`, nonzero = include the voxel.
- Auxiliary scalar inputs (single-voxel `fit_voxel`/`forward`) and
  auxiliary maps (`fit_volume`) are both passed by JSON object keyed on
  the input's name (e.g. `B1map`, `R1map`); the accepted names are
  model-specific.

## Two builds

### Default (stable, single-threaded)

```bash
wasm-pack build crates/qmrust-wasm --target web
```

Builds on the stable Rust toolchain, requires no special HTTP headers,
and can be served from any static host. Use this unless you need the
multithreaded fitting path.

### `threads` feature (nightly, multithreaded)

Requires a nightly toolchain with `build-std` and WASM atomics/bulk-memory:

```bash
rustup run nightly wasm-pack build crates/qmrust-wasm --target web \
  --features threads \
  -- -Z build-std=std,panic_abort
```

with

```bash
RUSTFLAGS="-C target-feature=+atomics,+bulk-memory,+mutable-globals"
```

set in the environment.

The resulting page **must** be served with cross-origin isolation
headers, or `SharedArrayBuffer` (and therefore the thread pool) will be
unavailable in the browser:

```
Cross-Origin-Opener-Policy: same-origin
Cross-Origin-Embedder-Policy: require-corp
```

Before calling `fit_volume`, initialize the thread pool once:

```js
await init(); // wasm-bindgen glue init
await initThreadPool(navigator.hardwareConcurrency);
```

## JS usage example

```js
import init, { list_models, fit_voxel, fit_volume } from "./pkg/qmrust_wasm.js";

await init();

console.log(list_models()); // ["inversion_recovery", "qmt_spgr", ...]

const cfg = `
model: inversion_recovery
method: complex
inversion_times: [350, 500, 650, 800, 950, 1100, 1250, 1400, 1700]
`;

const signal = new Float64Array([/* 9 measured points */]);
const params = fit_voxel(cfg, signal, ""); // [T1, ...]

// Whole-volume fit (threaded build only needs initThreadPool() first):
const dims = new Uint32Array([nx, ny, nz, nt]);
const maps = fit_volume(cfg, volumeData, dims, maskBytes, "");
console.log(maps.T1); // Float64Array-like output, C-order [nx,ny,nz]
```

## Testing

- Native unit tests for the `api` layer: `cargo test -p qmrust-wasm`.
- Browser smoke tests for the `wasm` layer live in `tests/browser.rs` and
  run headless in CI:
  `wasm-pack test --headless --chrome crates/qmrust-wasm`.
