# Fitting without BIDS

qmrust's inputs are BIDS-shaped by design, but a dataset that is not BIDS —
a stacked 4D NIfTI, or a qMRLab `.mat` file — fits through the same engine.
The only difference is **where the acquisition and the auxiliary maps come
from**: BIDS reads them from filenames and JSON sidecars, the non-BIDS path
reads them from your config file and from command-line flags.

Nothing about the fit changes. The same model, the same protocol values, and
the same identity matching produce the same maps either way.

## The two halves of a config

Every recipe in `recipes/` comes in both variants, and the difference between
them is exactly the acquisition:

`recipes/bids/irt1_config.yaml` omits the inversion times — the model's
`protocol_schema()` reads `InversionTime` from each volume's sidecar.

`recipes/non-bids/irt1_config.yaml` declares them, because there are no
sidecars to read:

```yaml
model: inversion_recovery
inversion_times: [0.350, 0.500, 0.650, 0.800, 0.950, 1.100, 1.250, 1.400, 1.700]
repetition_time: 2.5
```

All times are in seconds. qmrust performs no unit conversion: a config in
milliseconds fits a model in milliseconds and yields wrong numbers silently.

## Series data: one 4D file

A model whose acquisition is a series (inversion recovery, multi-echo T2,
qMT-SPGR) reads a single 4D NIfTI whose fourth axis is the acquisition axis,
in the order the config declares:

```bash
qmrust fit --data IRData.nii.gz --mask Mask.nii.gz \
  --config recipes/non-bids/irt1_config.yaml --output-dir out_ir
```

qMRLab `.mat` data works the same way with `--mat-data`:

```bash
qmrust fit --mat-data IRData.mat --mask Mask.mat \
  --config recipes/non-bids/irt1_config.yaml --output-dir out_ir
```

## Named data: one file per role

A model whose acquisition is a fixed set of roles (MTR's `MTon`/`MToff`,
MTsat's `MTw`/`PDw`/`T1w`) has no meaningful volume order, so the roles are
named rather than counted. For a stacked 4D file the volumes are read in the
model's declared role order; to be explicit, convert to BIDS first with
`qmrust bidsify --nii-dir`, which names each file after its role.

## Auxiliary maps come from flags

Where a BIDS fit resolves a B1, B0 or R1 map by its suffix in the dataset, a
non-BIDS fit takes one flag per map:

| flag | what it supplies |
|---|---|
| `--mask` | binary mask; voxels outside it are left `NaN` |
| `--b1map` | relative transmit field (1.0 = nominal) |
| `--b0map` | off-resonance field, in Hz |
| `--r1map` | longitudinal relaxation rate, in 1/s |

Each accepts NIfTI or `.mat`. A model documents which of these it consumes on
its own page, under **Inputs**.

## Converting to BIDS instead

If you plan to fit a dataset more than once, converting it is usually less
work than maintaining flags. `qmrust bidsify` writes a BIDS layout whose
sidecars carry the protocol your config declares:

```bash
qmrust bidsify --model inversion_recovery --mat-data IRData.mat --mask Mask.mat \
  --config recipes/non-bids/irt1_config.yaml --subject 01 --out ds-mydata
```

From then on the BIDS recipe applies, and [the BIDS guide](bids.md) describes
how collections, auxiliary maps and masks are resolved.
