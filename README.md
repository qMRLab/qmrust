# qmrust

It is qMRLab (https://qmrlab.org) written in Rust, so it is fast, and the fitting code is portable: the same routines run on the command line and, compiled to WebAssembly, inside a web browser with no server.

qmrust is a growing library of models, each one self-contained so a new model
can be added without disturbing the others. Today it fits:

- Inversion recovery T1
- Quantitative magnetization transfer (qMT-SPGR), with the Ramani and
  Sled-Pike sub-models

More are on the way, and the project is built to make adding them simple.

## Getting started

Check out docs: qmrlab.org/qmrust

Build the tool:

```bash
cargo build --release
```

The binary lands at `target/release/qmrust`. Run `cargo install --path .` to
call `qmrust` from anywhere.

Fit a dataset:

```bash
qmrust fit --bids-dir path/to/dataset \
  --config recipes/bids/irt1_config.yaml \
  --output-dir results
```

qmrust reads BIDS datasets directly, taking each scan's acquisition details
from its sidecar. When your data is not in BIDS, point it at plain NIfTI or
qMRLab `.mat` files instead and the acquisition details come from the config.
The `recipes/` folder holds ready-to-edit example configs, one per model,
grouped by how you feed in the data. Run `qmrust fit --help` for the full list
of inputs.

## Simulating

You can also generate a signal from known parameters, add noise, and fit it
back. This helps you check a model or see how reliably a parameter can be
recovered.

```bash
qmrust sim single-voxel --config recipes/sim/qmt_sim_ramani.yaml --output result.json
```

## Learning more

- `recipes/README.md` walks through the example configs and how to run BIDS and
  non-BIDS data.
- `docs/` covers the available models, the data pipeline, and how to add a
  model of your own.

## Background

qmrust grows out of [qMRLab](https://qmrlab.org), the MATLAB toolbox for
quantitative MRI. It reimplements selected models natively so they run quickly
and in more places, and its results are made to line up with qMRLab's for
validation. Values follow BIDS conventions in SI units, so a T1 map is in
seconds rather than qMRLab's milliseconds.
