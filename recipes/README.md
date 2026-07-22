# Recipes

Reusable `--config` manifests, grouped by how input files map to model roles.

- `bids/` — for `qmrust fit --bids-dir <dataset>`. Per-volume identity
  (InversionTime, FlipAngle, Angle/Offset, …) comes from BIDS filenames + JSON
  sidecars, and collections are assembled by the grouping manifest (built-in,
  or `--grouping <file>`). The acquisition arrays are omitted — every model
  composes its protocol from the resolved sidecars through the same build
  pipeline. Auxiliary maps (B1/B0/R1) are resolved from the dataset by suffix;
  an optional `mask:` block selects which mask to apply.
- `non-bids/` — for `qmrust fit --data <4D.nii>` or `--mat-data`/`--mat-dir`.
  There are no sidecars, so the config carries the acquisition arrays
  (`inversion_times`, qMT `mtdata`). Auxiliary maps are mapped by CLI flag —
  `--r1map`, `--b1map`, `--b0map`, `--mask` — one per role.
- `sim/` — simulation configs for `qmrust sim` (no input files).

Every model works through both paths — the difference is only where the
acquisition and auxiliary inputs come from (BIDS sidecars vs config + CLI
flags), not which models are supported.

## Examples

BIDS:

    qmrust fit --bids-dir data/ds-irt1 --config recipes/bids/irt1_config.yaml --output-dir out
    qmrust fit --bids-dir data/ds-qmt  --config recipes/bids/qmt_config_ramani.yaml --output-dir out

Non-BIDS (.mat):

    qmrust fit --mat-data IRData.mat --mask Mask.mat \
      --config recipes/non-bids/irt1_config.yaml --output-dir out_ir
