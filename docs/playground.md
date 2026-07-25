---
title: Playground
subtitle: Fit real data in your browser
---

# Playground

Everything below runs in **your browser**. `qmrust-core` compiles to
WebAssembly unchanged, so the numbers here come from exactly the same Rust
code the command-line tool runs — no server, no upload, no Python.

Pick a model, edit its recipe if you like — as a form or as raw YAML, they
stay in sync — press **Fit slice**, then click a voxel in either viewer to see
its measured data against the model's forward signal at the fitted
parameters. Edits are local to your browser and are what actually gets
fitted, so changing a fit option and re-fitting visibly changes the map.
Available for models whose fitted parameters are all written as output maps;
a model with nuisance parameters that are not exported as maps (its fit still
succeeds; only the per-voxel curve view is unavailable) reports why when you
click.

:::{iframe} ./playground/index.html
:width: 100%
:height: 830px
Live fitting in WebAssembly. Each model ships one downsampled slice from the
qmrust example dataset.
:::

## What you are looking at

The bundled slices are downsampled so each payload stays small; a full-volume
fit is what the CLI is for. The fit itself is not approximated — same
convergence criteria, same bounds, same output maps.

To run the same thing on your own data, see [Getting started](getting-started.md)
for the CLI or [Browser & wasm](guide/browser.md) for the JavaScript API.
