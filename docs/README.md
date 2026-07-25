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

# figures + playground slices (needs the example dataset)
python3 scripts/make_docs_figures.py --bids-dir <ds-qmrust-test> \
  --catalog catalog.json --bundle-slice
```

CI runs `gen_model_docs.py --check` and fails when a committed page differs
from what the registry would generate. If you added a model, that check is
what reminds you to regenerate.

## Playground

The playground is a standalone app in `playground/`, embedded via an iframe
because MyST sanitizes page HTML. Its wasm package is built by CI and is not
committed:

```bash
cd crates/qmrust-wasm && wasm-pack build --target web --out-dir ../../docs/playground/pkg
```

`playground.md`'s iframe references the app with a relative `src`
(`./playground/index.html`). MyST does not rewrite iframe paths the way it
rewrites images, and book-theme's client router serves `/playground` without
a trailing slash, so a relative `src` alone resolves to a different (broken)
target depending on how a visitor arrives at the page. CI's build step copies
the app to `_build/html/playground/playground/` and then rewrites that
`src` to an absolute, `BASE_URL`-prefixed path
(`${BASE_URL}/playground/playground/index.html`) in the built page only —
never in the committed Markdown, whose relative form stays deployment-target
agnostic for local preview. A local `myst start`/`myst build` preview keeps
the unrewritten relative `src`, so the playground page's own iframe should be
checked directly by URL, at `/playground/playground/index.html`, when
previewing locally.

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
  - `config` — the full text of the model's non-BIDS recipe YAML, ready to
    hand to `wasm`/CLI fit calls as-is.
  - `files` — `{data, mask}`, the filenames (relative to this directory) of
    the two `.nii.gz` payloads above; `mask` is `null` when the collection
    has none.
  - `probes` — a short list of oracle voxels for verifying a browser fit
    against the CLI's, numerically instead of by eye. Each entry is
    `{"x": <col>, "y": <row>, "expected": {<output name>: <value>, ...}}`,
    where `x`/`y` index the `(nx, ny)` plane the same way `dims[0]`/`dims[1]`
    do. Voxels are chosen deterministically: the in-mask voxels nearest the
    mask centroid (or, with no mask, the slice center plus a handful of fixed
    offsets around it), most-central first. Every probe was produced by
    fitting the *exact* `<model>.nii.gz` (and, when present, its aux inputs at
    the same downsample factor) with `qmrust fit`, so a probe is present only
    when every one of that model's outputs was finite at that voxel. A
    scrambled read of the data volume (wrong axis order, wrong plane) will
    fail these checks immediately, rather than only looking wrong on screen.

`index.json` is unchanged: `{"models": [<model name>, ...]}`, one entry per
model with a bundle.

## Agent-facing docs

`agents/` documents the architecture and data pipeline for coding agents. It is
not part of the user site.
