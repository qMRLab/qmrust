# Documentation

The site is [MyST](https://mystmd.org/guide) and builds to `_build/html`:

```bash
cd docs && myst start          # live preview
cd docs && myst build --html   # static site
```

## What is generated, and what is written by hand

Everything under `models/`, plus `figures/`, `playground/data/` and the
method card grid inside `index.md`, is **generated**. Do not edit those
files — your changes will be overwritten, and CI will fail before they
merge.

| Content | Source of truth |
|---|---|
| Model pages, gallery, landing card grid | `ModelEntry.doc` in `crates/qmrust-core/src/registry.rs` + the `Model` trait |
| Example figures | `scripts/make_docs_figures.py` over a bidsified example dataset |
| Playground slices | the same script, with `--bundle-slice` |
| Guides, getting started, dev pages | hand-written Markdown in this directory |
| Citations | `references.bib` |

## Regenerating

```bash
cargo build --release -p qmrust-cli
./target/release/qmrust catalog > catalog.json

# pages (fast, no data needed)
python3 scripts/gen_model_docs.py --catalog catalog.json

# figures + playground slices (needs the example datasets)
python3 scripts/make_docs_figures.py --bids-dir ~/Desktop/qmrust-osf \
  --catalog catalog.json --bundle-slice
```

`--bids-dir` is the *parent* of one BIDS dataset root per model
(`ds-<lowercased BIDS suffix>/`, e.g. `ds-irt1/`), as built by
`scripts/make_bids_examples.sh`. A model whose root is absent is skipped with a
warning, so the docs build never depends on the data being present.

CI runs `gen_model_docs.py --check` and fails when a committed page differs
from what the registry would generate. If you added a model, that check is
what reminds you to regenerate.

## Playground

The playground is a standalone app in `playground/`, embedded via an iframe
because MyST sanitizes page HTML. It renders with
[NiiVue](https://github.com/niivue/niivue), which reads the bundled `.nii.gz`
files directly — no hand-rolled NIfTI parsing anywhere in it. Its wasm package is
built by CI and is not committed:

```bash
cd crates/qmrust-wasm && wasm-pack build --target web --out-dir ../../docs/playground/pkg
```

### Module layout

Plain ES modules, loaded by the browser directly — there is no bundler, and
adding a file needs no build step, only an `import`. `index.html` is markup and
`app.css` is style; `app.js` is the entry point, and each other module owns one
region of the page.

| Module | Owns |
|---|---|
| `app.js` | startup, and the `wire*` functions that attach each region's listeners |
| `state.js` | the mutable state more than one module touches, and its invariants |
| `dom.js` | `$`, the navbar status line, display formatting, CSS colour tokens |
| `bundles.js` | the wasm module and the per-model payload JSON |
| `dataset.js` | fetching, unzipping and resolving a BIDS dataset; the download ring |
| `drop.js` | a reader's own dataset, dropped or browsed to |
| `nifti.js` | the array boundary: NVImage ⇄ `fit_volume`'s C-order buffers |
| `stats.js` | percentiles, the display window, the ROI summary — one convention |
| `recipe.js` | the generic YAML-tree form walk, the editor, syntax highlighting |
| `model.js` | loading a model's data: the BIDS path and the pre-baked fallback |
| `fit.js` | fitting the slice in row-blocks, and the maps that come back |
| `inputs.js` | the Inputs card: its two tabs, the frame slider, the file tree |
| `viewers.js` | the fitted-map panel, and what both viewers share |
| `level.js` | the window/level widget, and `setWindow` — the one place a window changes |
| `roi.js` | ROI statistics and the pen |
| `curve.js` | the ECharts voxel-fit chart, and the hover/crosshair marks |
| `modal.js` | both modals, and the single NiiVue instance they share |

Three rules keep the graph honest, and are worth preserving:

- **Acyclic.** `state.js`, `dom.js` and `stats.js` are leaves; `app.js` is
  imported by nothing. A cycle means two modules are really one region.
- **State is shared only where it must be.** `app` in `state.js` holds what more
  than one module touches, and is `Object.seal`ed so a misspelled field throws
  rather than silently becoming a new one. Anything one module owns stays a
  `let` in that module.
- **Export nothing unused.** A helper only its own module calls is not exported.

### Vendored NiiVue

`playground/vendor/niivue.js` is a committed, pre-bundled copy of
`@niivue/niivue`: the published ESM has bare imports (`gl-matrix`, `fflate`,
`nifti-reader-js`, `zarrita`, `@lukeed/uuid`, `@ungap/structured-clone`,
`array-equal`) that a browser can't resolve directly, and the docs build must
not reach the network at page-load time. Regenerate it with:

```bash
mkdir /tmp/niivue-build && cd /tmp/niivue-build
npm init -y
npm install @niivue/niivue@0.69.0 esbuild
echo 'export { Niivue, NVImage, NVMesh, SLICE_TYPE, DRAG_MODE } from "@niivue/niivue";' > entry.js
npx esbuild entry.js --bundle --format=esm --minify --outfile=niivue.js
cp niivue.js <repo>/docs/playground/vendor/niivue.js
```

### Vendored js-yaml

`playground/vendor/js-yaml.js` is a committed, pre-bundled copy of `js-yaml`,
used by the recipe editor's Form ⇄ YAML sync. Regenerate it with:

```bash
mkdir /tmp/jsyaml-build && cd /tmp/jsyaml-build
npm init -y
npm install js-yaml esbuild
echo 'export { load, dump } from "js-yaml";' > entry.js
npx esbuild entry.js --bundle --format=esm --minify --outfile=js-yaml.js
cp js-yaml.js <repo>/docs/playground/vendor/js-yaml.js
```

### Vendored fflate

`playground/vendor/fflate.js` is the upstream `fflate` ESM browser build, taken
verbatim — it extracts the fetched dataset archives. `fflate` has no
dependencies and its published `esm/browser.js` has no bare imports, so unlike
NiiVue and js-yaml it needs no local bundling step; copying the file *is* the
build, which also makes the vendored copy byte-verifiable against upstream:

```bash
curl -sSL -o <repo>/docs/playground/vendor/fflate.js \
  https://unpkg.com/fflate@0.8.2/esm/browser.js
# expected: 8cc1f687e0159e977addb6b85e274dbd11e622cf151f4fcb7b85d49622ea43e7
shasum -a 256 <repo>/docs/playground/vendor/fflate.js
```

MIT licensed. Only `unzipSync`/`gunzipSync` are used; `.nii.gz` entries are
handed to NiiVue still gzip-compressed, which reads them directly.

### Vendored highlight.js

`playground/vendor/highlight-{core,yaml,json}.js` are highlight.js's own prebuilt
ESM modules, taken verbatim — they colour the recipe editor (YAML) and the
sidecar previewer (JSON). Like `fflate` these need
no bundling step, so the vendored copies are byte-verifiable against upstream:

```bash
V=11.10.0
curl -sSL -o <repo>/docs/playground/vendor/highlight-core.js \
  https://cdn.jsdelivr.net/npm/@highlightjs/cdn-assets@$V/es/core.min.js
for L in yaml json; do
  curl -sSL -o <repo>/docs/playground/vendor/highlight-$L.js \
    https://cdn.jsdelivr.net/npm/@highlightjs/cdn-assets@$V/es/languages/$L.min.js
done
# expected:
#   224f718d73f37e1b85fbab697b57d75dc2013e57bc0485a8fa8534070f3e543a  highlight-core.js
#   61dffad838f9a8cca2e2479e1e14c193d604de02bf5d177528e7f8b82c2626c2  highlight-yaml.js
#   ea5900cde2028a6405cf69b0ec3284014ad050a73e64d26371ef4e7b44402173  highlight-json.js
shasum -a 256 <repo>/docs/playground/vendor/highlight-*.js
```

BSD-3-Clause. No highlight.js theme is shipped: `index.html` maps the `hljs-*`
token classes onto the page's own palette, so the editor follows light/dark with
everything else. Highlighting is presentation only — `js-yaml` remains the sole
parser, and the editor's `valid`/`invalid` pill comes from it.

### Vendored ECharts

`playground/vendor/echarts.js` is Apache ECharts' own prebuilt ESM bundle, taken
verbatim — it draws the single-voxel fit chart. Like the others it needs no
bundling step:

```bash
curl -sSL -o <repo>/docs/playground/vendor/echarts.js \
  https://cdn.jsdelivr.net/npm/echarts@5.5.1/dist/echarts.esm.min.js
# expected: 342d276d87314bfd6af8c4dd48b5cbbf8cb556642aa8a770d740057dbc996588
shasum -a 256 <repo>/docs/playground/vendor/echarts.js
```

Apache-2.0, and ~1 MB — by far the largest vendored dependency after NiiVue, so
it should earn its place. It replaced a hand-drawn canvas plot (axes, ticks and
labels included) rather than being added beside one, and it is the intended home
for the charts that follow: a voxelwise scatter, and per-ROI box plots. No ECharts
theme is shipped; each chart's colours come from this page's CSS custom
properties, so charts follow light/dark like everything else.

The window/level histogram and colour bar are deliberately *not* ECharts: they
are small bespoke controls wired directly to NiiVue's colormap and to the
volume's display window, which a charting library would only get in the way of.

`playground.md`'s iframe references the app with a relative `src`
(`./playground/index.html`). MyST does not rewrite iframe paths the way it
rewrites images, and book-theme's client router serves `/playground` without
a trailing slash, so a relative `src` alone resolves to a different (broken)
target depending on how a visitor arrives at the page. CI's build step copies
the app to `_build/html/app/` — a sibling of the page's own route directory,
which already owns `playground/index.html` — and then rewrites that `src` to
an absolute, `BASE_URL`-prefixed path (`${BASE_URL}/app/index.html`) in the
built page only — never in the committed Markdown, whose relative form stays
deployment-target agnostic for local preview. A local `myst start`/`myst
build` preview keeps the unrewritten relative `src`, so the playground page's
own iframe should be checked directly by URL, at
`/playground/playground/index.html`, when previewing locally.

### Playground data contract

`--bundle-slice` writes, per model, into `docs/playground/data/`:

- **`<model>.nii.gz`** — the downsampled single-slice acquisition, as a real
  NIfTI-1 file, gzip-compressed, **float32**, shape `(nx, ny, 1, nt)` (`nt` is
  the number of acquired volumes/roles). Reading it back with any NIfTI reader
  (nibabel, NiiVue, `docsfig.nifti.read_nii`) reproduces exactly the slice
  `make_docs_figures.py` sampled from the BIDS dataset, downsampled by the
  integer `factor` recorded in the sidecar JSON.
- **`<model>_mask.nii.gz`** — present only when the collection has a mask;
  same in-plane dims as the data file (`(nx, ny, 1)`), float32, values `0.0`
  or `1.0`.
- **`<model>_<auxname>.nii.gz`** — one per auxiliary input the collection
  resolved (`B1map`, `B0map`, `R1map`), present only for models that declare
  and find one. Same in-plane dims and downsample `factor` as the data file
  (`(nx, ny, 1)`), float32. Shipped so a model whose recipe uses an aux input
  to constrain the fit (e.g. `qmt_spgr`'s `use_r1map_to_constrain_r1f`) is
  reproducible in the browser exactly as the CLI fit it — without the aux,
  those outputs cannot match `probes`.
- **`<model>.json`** — the sidecar metadata:
  - `model`, `title` — registry identity and display title.
  - `dims` — `[nx, ny, 1, nt]`, matching the `.nii.gz` header.
  - `factor` — the integer downsample factor applied (`1` = full resolution).
  - `volume_ids` — one entry per plane, in the same order as the `nt` axis.
    For a `series` model this is the identity row (`{key: value, ...}`) read
    from the sidecar; for a `named` model it is the role name string
    (`"MTon"`, `"MToff"`, ...), in canonical role order.
  - `labels` — one human-readable string per plane, same order as
    `volume_ids`.
  - `params` — the model's fit parameter names, in fit order.
  - `outputs` — `[{name, unit}]` for every non-diagnostic output map.
  - `enums` — `[{key, values}]`: config keys restricted to a fixed set of
    values (`crates/qmrust-core/src/registry.rs`'s `ModelDoc::enums`), for
    rendering a dropdown instead of free text in the playground's Form view.
    `key` is a dotted path from the config root (e.g. `"qmt_spgr.model"`,
    `"fit_type"`); a model with no such fields has an empty list.
  - `config` — the full text of the model's non-BIDS recipe YAML, ready to
    hand to `wasm`/CLI fit calls as-is.
  - `files` — `{data, mask, aux}`: `data`/`mask` are the `.nii.gz` filenames
    above (`mask` is `null` when the collection has none); `aux` is an object
    mapping aux input name (e.g. `"R1map"`) to its `.nii.gz` filename, empty
    when the model resolved none. A consumer that fits this bundle must load
    every entry in `aux` and pass it through alongside `data`/`mask` — a
    partial fit (data only) will not reproduce `probes` for a model whose
    recipe constrains the fit with an aux input.
  - `probes` — a short list of oracle voxels for verifying a browser fit
    against the CLI's, numerically instead of by eye. Each entry is
    `{"x": <i>, "y": <j>, "expected": {<output name>: <value>, ...}}`, where
    `x`/`y` are indices into the `.nii.gz` array's first and second axes
    respectively — i.e. `data[x, y, 0, t]` for plane `t` — the same order as
    `dims[0]`/`dims[1]` and NIfTI's own on-disk voxel-index order. This is
    **not** row/column image-display order (where the first index is
    usually the row = the second spatial axis); a consumer that addresses
    voxels by `(row, col)` must swap before comparing against `x`/`y`. Voxels
    are chosen deterministically: the in-mask voxels nearest the mask
    centroid (or, with no mask, the slice center plus a handful of fixed
    offsets around it), most-central first. Every probe was produced by
    fitting the *exact* `<model>.nii.gz` bundle (data, mask, and every file
    in `files.aux`) with `qmrust fit`, so a probe is present only when every
    one of that model's outputs was finite at that voxel, and
    `_verify_probes` (in `make_docs_figures.py`) re-reads the written
    `.nii.gz` immediately after and asserts each probe's `(x, y)` addresses
    the same sample it was derived from — so a transposed convention fails
    at generation time, not only downstream. A scrambled read of the data
    volume (wrong axis order, wrong plane) will fail these checks
    immediately, rather than only looking wrong on screen.

`index.json` is unchanged: `{"models": [<model name>, ...]}`, one entry per
model with a bundle.

## Agent-facing docs

`agents/` documents the architecture and data pipeline for coding agents. It is
not part of the user site.
