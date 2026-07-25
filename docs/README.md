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

## Agent-facing docs

`agents/` documents the architecture and data pipeline for coding agents. It is
not part of the user site.
