# Getting started

This page gets you from a fresh clone to a fitted map or a simulated signal.
If you just want the shape of the project first, read [index](index.md) or
[architecture](architecture.md) instead.

## Build

```bash
cargo build --workspace
```

This builds all four crates. To get an optimized `qmrust` binary:

```bash
cargo build --release          # binary at target/release/qmrust
cargo install --path crates/qmrust-cli   # optional: onto your PATH
```

## Run a fit

Every fit needs a **config** (protocol + fitting parameters) and some data.
Example configs live in [`prots/`](../prots/) — they're fully explicit YAML,
so a run is self-documenting.

```bash
cargo run -p qmrust-cli -- fit \
  --mat-dir <dir-with-MTdata/R1map/B1map/B0map/Mask.mat> \
  --config prots/qmt_config_sledpikerp.yaml \
  --output-dir <out>
```

Or fit inversion-recovery T1 from a single 4D NIfTI:

```bash
cargo run -p qmrust-cli -- fit \
  --data ir_data.nii.gz \
  --config prots/irt1_config.yaml \
  --output-dir <out>
```

Print the fully-resolved config (defaults applied, validated) before running:

```bash
cargo run -p qmrust-cli -- dump-config --config prots/qmt_config_sledpikerp.yaml
```

## Fit a BIDS dataset

If your data is already laid out as a BIDS dataset, point at the dataset root
instead of individual files:

```bash
cargo run -p qmrust-cli -- fit \
  --bids-dir <path-to-bids-dataset> \
  --config prots/irt1_config.yaml \
  --output-dir <out>
```

This scans the dataset, groups files into collections per the config's model
(e.g. an inversion-time series for IRT1), and fits each subject (and session,
if present), writing `<out>/<subject>[/<session>]/<map>.nii.gz`. v1 scope:
sequential collections (IRT1-style) and models with no required auxiliary
input — a model needing B1/B0/R1 maps or a named collection like MTS isn't
BIDS-fittable yet; use `--mat-dir`/`--data` with explicit `--r1map`/`--b1map`/
`--b0map` for those. See [BIDS](bids.md) for how `rust-bids` resolves the
dataset layout.

## Run a simulation

`qmrust sim` generates a forward signal from ground-truth parameters,
optionally adds noise, and fits it back — useful for sanity-checking a
protocol without any real data.

```bash
cargo run -p qmrust-cli -- sim single-voxel \
  --config prots/qmt_sim_ramani.yaml \
  --output sv.json
```

Other sim subcommands: `signal` (forward only), `sensitivity` (parameter
sweep), `montecarlo` (fit over parameter distributions). See the top-level
[`README.md`](../README.md) for the full flag reference and per-model input
requirements.

## Verify before you push

```bash
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build -p qmrust-core --target wasm32-unknown-unknown   # core must stay wasm-clean
```

## Next steps

- Adding your own model? See [Models](models.md).
- Working from a BIDS dataset instead of individual files? See [BIDS](bids.md).
