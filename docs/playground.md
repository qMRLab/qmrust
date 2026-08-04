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

## Simulate

Switch the navbar toggle from **Data** to **Simulate** and the recipe card
holds the model's own sim recipe instead of a dataset's config. Simulation
reads no image data, so it works whether or not a dataset loaded. Pick one of
the four modes (Signal, Voxel, Sensitivity, Multi-Voxel), press **Simulate**,
and read the chart and stats table that mode produces; see
[Simulation](guide/simulation.md) for what each mode answers. A sweep runs in
a background worker so the page stays responsive and cancellable, and the
numbers match `qmrust sim` exactly: same call, same seed.

**Sensitivity** draws one panel per reported parameter, fitted value against
the swept input in the parameter's own units, with mean plus or minus one
standard deviation error bars: the swept parameter's own panel carries a
diagonal identity line, so a point on it is perfect recovery, while every
other parameter's panel carries a horizontal line at its constant truth.

**Multi-Voxel** draws two rows per parameter: a per-voxel scatter of fitted
against input value with a diagonal identity line, and beneath it a histogram
of that parameter's error with lines at zero and at the mean error.

## What you are looking at

The bundled slices are downsampled so each payload stays small; a full-volume
fit is what the CLI is for. The fit itself is not approximated — same
convergence criteria, same bounds, same output maps.

To run the same thing on your own data, see [Getting started](getting-started.md)
for the CLI or [Browser & wasm](guide/browser.md) for the JavaScript API.
