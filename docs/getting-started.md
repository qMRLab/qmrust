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
Example configs live in [`recipes/`](../recipes/), grouped by input type. The
non-BIDS configs (`recipes/non-bids/`) are fully explicit YAML that list the
whole acquisition protocol, so a run is self-documenting. BIDS configs
(`recipes/bids/`) leave the protocol out; those values come from the dataset's
JSON sidecars.

```bash
cargo run -p qmrust-cli -- fit \
  --mat-dir <dir-with-MTdata/R1map/B1map/B0map/Mask.mat> \
  --config recipes/non-bids/qmt_config_sledpikerp.yaml \
  --output-dir <out>
```

Or fit inversion-recovery T1 from a single 4D NIfTI:

```bash
cargo run -p qmrust-cli -- fit \
  --data ir_data.nii.gz \
  --config recipes/non-bids/irt1_config.yaml \
  --output-dir <out>
```

Print the fully-resolved config (defaults applied, validated) before running:

```bash
cargo run -p qmrust-cli -- dump-config --config recipes/non-bids/qmt_config_sledpikerp.yaml
```

## Fit a BIDS dataset

If your data is already laid out as a BIDS dataset, point at the dataset root
instead of individual files:

```bash
cargo run -p qmrust-cli -- fit \
  --bids-dir <path-to-bids-dataset> \
  --config recipes/bids/irt1_config.yaml \
  --output-dir <out>
```

This scans the dataset, groups files into collections per the config's model
(e.g. an inversion-time series for IRT1, or an Angle/Offset series for
QMTSPGR), and fits each subject (and session, if present), writing
`<out>/qmrust/<subject>[/<session>]/anat/<subject>[_<session>]_<suffix>.nii.gz`.
Each model composes its
acquisition protocol from the sidecars and resolves any auxiliary maps it
declares (B1/B0/R1) from the dataset by suffix — so aux-requiring models like
qMT fit through `--bids-dir` too. *Named* collections (fixed role slots, e.g.
MTR's mt-on/mt-off) fit as well: their role-labeled volumes are mapped onto the
model's declared roles, so the grouping's `named_set` role names must match the
model's `measurement()` roles. See [BIDS](bids.md) for how `rust-bids` resolves
the dataset layout.

Notice `--config` above points at `recipes/bids/irt1_config.yaml`, not the
non-BIDS one — a BIDS fit's config doesn't carry the inversion times: the
model declares which acquisition parameters it needs from the JSON sidecars,
so `--config` is just algorithm options (fit bounds, etc.) plus the `mask:`
block that disambiguates which mask to apply. See
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
  --config recipes/non-bids/irt1_config.yaml \
  --subject 01 --out ds-qmrust
```

This writes `ds-qmrust/sub-01/anat/sub-01_inv-<i>_IRT1.nii.gz` (+
`{InversionTime}` sidecars), `dataset_description.json`, `participants.tsv`,
and the mask under `ds-qmrust/derivatives/preprocessed/sub-01/anat/
sub-01_desc-brain_mask.nii.gz`.

`bidsify` also supports `qmt_spgr`, whose BIDS identity is the custom,
non-official suffix `QMTSPGR` (see [BIDS](bids.md)). Point it at a directory
of qMRLab's qMT `.mat` files instead of a single file, and append it as a
second subject in the same dataset:

```bash
cargo run -p qmrust-cli -- bidsify \
  --model qmt_spgr \
  --mat-dir <dir-with-MTdata/R1map/B1map/B0map/Mask.mat> \
  --config recipes/non-bids/qmt_config_ramani.yaml \
  --subject 02 --out ds-qmrust
```

This writes `ds-qmrust/sub-02/anat/sub-02_flip-<f>_mt-<m>_QMTSPGR.nii.gz` for
each of the 10 MT-weighted volumes (2 flip angles × 5 offsets), each with a
sidecar carrying the acquisition metadata the fit reads back by identity:

```json
{"Angle": 142.0, "Offset": 443.0, "RepetitionTime": 0.03, "MTPulseDuration": 0.008}
```

and a root `.bidsignore` containing `*QMTSPGR*` (so general BIDS validators
skip the non-official suffix; qmrust's own layout resolver discovers it
regardless — see [BIDS](bids.md)). Any computed inputs present in `--mat-dir`
are written byte-identical to a `preprocessed` derivatives pipeline: B1/B0
field maps under `ds-qmrust/derivatives/preprocessed/sub-02/fmap/`
(`_TB1map`/`_B0map`), the R1 map and brain mask under that pipeline's `anat/`
(`_R1map`/`_desc-brain_mask`).

Fitting the resulting `QMTSPGR` collection the same way as IRT1:

```bash
cargo run -p qmrust-cli -- fit \
  --bids-dir ds-qmrust \
  --config recipes/bids/qmt_config_ramani.yaml \
  --output-dir ds-qmrust/derivatives
```

writes the six qMT maps qmt_spgr declares via `bids_outputs()` —
`sub-02_Fmap.nii.gz`, `_kRmap`, `_R1Fmap`, `_R1Rmap`, `_T2Fmap`, `_T2Rmap` —
under `ds-qmrust/derivatives/qmrust/sub-02/anat/`.

`scripts/make_bids_examples.sh` automates both examples end to end: it
fetches qMRLab's OSF IR and qMT demo datasets, runs `bidsify` for each into
the same `ds-qmrust` (sub-01 IRT1, sub-02 QMTSPGR), fits both via
`qmrust fit --bids-dir`, and confirms the IRT1 result is voxel-identical to
fitting the original `.mat` (in-mask) and within qMRLab's own reference
tolerance (`FitResults/T1.nii.gz`). The generated dataset itself is not
committed (large data stays out of the repo); re-run the script to regenerate
it.

## Run a simulation

`qmrust sim` generates a forward signal from ground-truth parameters,
optionally adds noise, and fits it back — useful for sanity-checking a
protocol without any real data.

```bash
cargo run -p qmrust-cli -- sim single-voxel \
  --config recipes/sim/qmt_sim_ramani.yaml \
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
