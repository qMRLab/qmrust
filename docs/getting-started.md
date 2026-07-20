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

Notice `--config` above still points at `irt1_config.yaml`, but in a BIDS fit
it no longer needs to carry the inversion times — the model declares which
acquisition parameters it needs from the JSON sidecars, so `--config` is just
algorithm options (fit bounds, etc.). See
[From sidecar metadata to `Protocol`](bids.md#from-sidecar-metadata-to-protocol)
for how that mapping works.

Fitted maps are written under `--output-dir` as a **BIDS-derivatives** tree:
`<output-dir>/qmrust/<subject>[/<session>]/anat/<subject>[_<session>]_<Suffix>.nii.gz`
(+ a JSON sidecar per map, and a derivatives `dataset_description.json`).
Only the maps a model declares via `bids_outputs()` are written (e.g. IRT1's
`T1` → `T1map`) — diagnostic outputs like `res`/`idx` are not. See
[DATA-PIPELINE.md](agents/DATA-PIPELINE.md) for the full output contract.

## Create a BIDS example from qMRLab data

`qmrust bidsify` converts a qMRLab `.mat` dataset into a BIDS layout whose
voxel data is byte-identical to the source `.mat` (no rescale, no dtype
narrowing — every volume round-trips as `f64`):

```bash
cargo run -p qmrust-cli -- bidsify \
  --model inversion_recovery \
  --mat-data IRData.mat --mask Mask.mat \
  --config prots/irt1_config.yaml \
  --subject 01 --out ds-qmrust
```

This writes `ds-qmrust/sub-01/anat/sub-01_inv-<i>_IRT1.nii.gz` (+
`{InversionTime}` sidecars), `dataset_description.json`, `participants.tsv`,
and the mask under `ds-qmrust/derivatives/qmrust/sub-01/anat/
sub-01_desc-brain_mask.nii.gz`. Only `inversion_recovery`/IRT1 is supported
today; QMTSPGR bidsify is a tracked follow-up.

`scripts/make_bids_examples.sh` automates this end to end: it fetches
qMRLab's OSF IR demo dataset, runs `bidsify`, then fits the resulting BIDS
dataset with `qmrust fit --bids-dir` and confirms the result is voxel-identical
to fitting the original `.mat` (in-mask) and within qMRLab's own reference
tolerance (`FitResults/T1.nii.gz`). The generated dataset itself is not
committed (large data stays out of the repo); re-run the script to regenerate
it.

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

## Units

qmrust works in BIDS/SI units end to end: protocol times (`inversion_times`,
`repetition_time` in config; `InversionTime`, `RepetitionTime` in sidecars) are in
**seconds**, and fitted time-constant maps (e.g. IRT1's `T1map`) are in **seconds** too —
so a fitted T1 of `0.9` means 900 ms, not 0.9 ms. This is a deliberate divergence from
qMRLab (which uses milliseconds): a qMRLab `FitResults/T1.nii.gz` reference differs from
qmrust's `T1map` by a factor of 1000. See the "Units — BIDS-native (SI)" principle in
[`CLAUDE.md`](../CLAUDE.md) for the full rule.

## Next steps

- Adding your own model? See [Models](models.md).
- Working from a BIDS dataset instead of individual files? See [BIDS](bids.md).
