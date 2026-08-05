# Simulation

Every model in qmrust implements a forward signal as well as a fit, so it can
generate data as well as consume it. `qmrust sim` exposes four modes that
answer four different questions, and each one writes a JSON report (and,
optionally, an SVG plot).

Simulation reads no image files. The acquisition comes from the config, so
`sim` configs are built on the non-BIDS recipes — those are the ones that
carry the acquisition arrays.

## The `sim` block

A sim config is a model config plus a `sim:` block:

```yaml
model: qmt_spgr
qmt_spgr:
  model: Ramani
  fitting:
    fx: [false, false, true, true, false, false]
sim:
  params: { F: 0.15, kr: 25.0, R1f: 1.0, R1r: 1.0, T2f: 0.028, T2r: 1.1e-5 }
  b1: 1.0
  b0: 0.0
  noise: { type: rician, snr: 100.0 }
  seed: 0
  trials: 100
  sweep: { param: F, start: 0.05, stop: 0.30, steps: 10 }
  distributions:
    F:  { mean: 0.15, std: 0.02 }
    kr: { mean: 25.0, std: 3.0 }
```

`params` names the model's own parameters — the same names its documentation
page lists under **Signal model**. `seed` makes every noisy mode
reproducible: the same seed yields the same trials on every platform, native
or wasm.

Every registered model ships a sim recipe under `recipes/sim/`, declared by
its registry entry. `qmrust catalog --json` reports the path for each model.

## `signal` — what does the model predict?

Noise-free forward signal for one parameter set. qMRLab has no equivalent method: every
other mode below wraps one of its `Sim_*` methods, but a plain forward signal is not
among them.

```bash
qmrust sim signal --config recipes/sim/qmt_sim_ramani.yaml \
  --output signal.json --plot signal.svg
```

Use it to sanity-check a protocol before acquiring it: if two saturation
offsets produce nearly the same signal, they are not buying you information.

## `single-voxel` — can the fit recover the truth?

Simulates one voxel, optionally `trials` times with noise, and fits each
trial back. Corresponds to qMRLab's `Sim_Single_Voxel_Curve`.

```bash
qmrust sim single-voxel --config recipes/sim/qmt_sim_ramani.yaml \
  --output sv.json --plot sv.svg
```

The report carries per-parameter truth, mean, standard deviation, bias and
RMSE. This is the first thing to run when a fit on real data looks wrong: if
the model cannot recover its own noise-free signal, the problem is the
protocol or the config, not the data.

## `sensitivity` — where does it break down?

Sweeps one parameter across a range and reports bias and standard deviation
at each point. Corresponds to qMRLab's `Sim_Sensitivity_Analysis`.

```bash
qmrust sim sensitivity --config recipes/sim/qmt_sim_ramani.yaml \
  --output sens.json --plot sens.svg
```

Driven by the `sweep:` block. Use it to find the range over which a parameter
is actually identifiable.

## `montecarlo` — how does it behave over a population?

Draws parameters from the `distributions:` block and reports error statistics
over the draws. Corresponds to qMRLab's `Sim_Multi_Voxel_Distribution`.

```bash
qmrust sim montecarlo --config recipes/sim/qmt_sim_ramani.yaml \
  --output mc.json --plot mc.svg
```

## In the browser

The same four modes run in wasm through `sim(mode, cfg_yaml)`, with identical
numbers, and the [playground](../playground.md) exposes them directly: switch
the recipe card from Data to Simulate, and the model's own sim recipe becomes
the editable recipe. Simulation reads no image data, so it works whether or not
a dataset is loaded.

Long runs execute in a worker rather than on the page's main thread, so a sweep
of a few thousand fits leaves the page responsive and cancellable. It is the
same single call with the same seed, so a browser run and a CLI run of the same
recipe agree exactly.
