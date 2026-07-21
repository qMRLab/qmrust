# Recipes

Reusable `--config` manifests, grouped by how input files map to model roles.

- `bids/` — for `qmrust fit --bids-dir <dataset>`. Per-volume identity
  (InversionTime, FlipAngle, …) comes from BIDS filenames + JSON sidecars, and
  collections are assembled by the grouping manifest (built-in, or
  `--grouping <file>`). Protocol arrays are omitted; the `mask:` block selects
  which mask in the dataset to apply.
- `non-bids/` — for `qmrust fit --data <4D.nii>` or `--mat-data`/`--mat-dir`.
  There are no sidecars, so the config carries the protocol arrays
  (`inversion_times`, qMT `mtdata`). Auxiliary inputs are mapped by CLI flag —
  `--r1map`, `--b1map`, `--b0map`, `--mask` — one per role.
- `sim/` — simulation configs for `qmrust sim` (no input files).

Note: qMT needs auxiliary inputs and `fit --bids-dir` is no-aux/sequential in
v1, so qMT recipes live under `non-bids/` only.

## Examples

BIDS:

    qmrust fit --bids-dir data/ds-irt1 --config recipes/bids/irt1_config.yaml --output-dir out

Non-BIDS (.mat):

    qmrust fit --mat-data IRData.mat --mask Mask.mat \
      --config recipes/non-bids/irt1_config.yaml --output-dir out_ir
