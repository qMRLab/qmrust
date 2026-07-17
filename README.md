# qmrust

Native-Rust quantitative MRI fitting — a fast port of selected [qMRLab](https://qmrlab.org) models.

**Models available**

| Config (`prots/`) | Model | Fits |
|---|---|---|
| `irt1_config.yaml` | Inversion Recovery T1 (Barral RD-NLS) | T1 mapping |
| `qmt_config_ramani.yaml` | qMT-SPGR · Ramani | F, kr, R1f, R1r, T2f, T2r |
| `qmt_config_sledpikerp.yaml` | qMT-SPGR · SledPikeRP | F, kr, R1f, R1r, T2f, T2r |

## Build

```bash
cargo build --release          # binary at target/release/qmrust
```
Optional — install onto your PATH so you can call `qmrust` anywhere:
```bash
cargo install --path .         # → ~/.cargo/bin/qmrust
```
Examples below use `./target/release/qmrust`; swap in `qmrust` if installed.

## Usage per config

### qMT-SPGR — SledPikeRP
Fits from `MTdata` + `R1map` + `B1map` + `B0map` + `Mask`. The `--mat-dir` mode
auto-loads all five `.mat` files from a folder by name.

```bash
./target/release/qmrust fit \
  --mat-dir ~/Desktop/qmrust_test/qmt_spgr \
  --config prots/qmt_config_sledpikerp.yaml \
  --output-dir ~/Desktop/qmrust_test/qmt_spgr/FitResults_rust
```

### qMT-SPGR — Ramani
Same inputs; just swap the config:
```bash
./target/release/qmrust fit \
  --mat-dir ~/Desktop/qmrust_test/qmt_spgr \
  --config prots/qmt_config_ramani.yaml \
  --output-dir ./FitResults_ramani
```

Instead of `--mat-dir`, you can pass maps individually (`.mat` or NIfTI):
```bash
./target/release/qmrust fit \
  --mat-data MTdata.mat --mask Mask.mat \
  --r1map R1map.mat --b1map B1map.mat --b0map B0map.mat \
  --config prots/qmt_config_sledpikerp.yaml --output-dir ./out
```

**qMT outputs** (8 maps): `F`, `kr`, `R1f`, `R1r`, `T2f`, `T2r`, `kf`, `resnorm`.

### Inversion Recovery T1
Needs IR data: a 4D NIfTI, or a `.mat` containing `IRdata` (+ optional `TI`, `Mask`).
```bash
./target/release/qmrust fit \
  --data ir_data.nii.gz \
  --config prots/irt1_config.yaml \
  --output-dir ./FitResults_t1
```
**T1 outputs**: `T1`, `b`, `a`, `res` (+ `idx` for the magnitude method).

## Configs

The files in `prots/` are **fully explicit** — every protocol, timing, pulse,
and fitting parameter is listed, so a run is self-documenting. Edit them to
match your acquisition (angles/offsets, timing table, bounds, etc.).

Print the fully-resolved config a run will use (defaults applied, validated):
```bash
./target/release/qmrust dump-config --config prots/qmt_config_sledpikerp.yaml
```

## Simulation

`qmrust sim` mirrors qMRLab's `Sim_*` tools: generate signal from ground-truth
parameters, optionally add noise, and fit it back. Parameters, noise, and sweep
ranges live in a `sim:` block in the same YAML.

```bash
# forward signal only
qmrust sim signal       --config prots/qmt_sim_ramani.yaml --output sig.json
# one voxel, N noisy trials, fit back (+ optional SVG)
qmrust sim single-voxel --config prots/qmt_sim_ramani.yaml --output sv.json --plot sv.svg
# sweep one parameter, report bias/std
qmrust sim sensitivity  --config prots/qmt_sim_ramani.yaml --output sens.json --plot sens.svg
# Monte-Carlo over parameter distributions
qmrust sim montecarlo   --config prots/qmt_sim_ramani.yaml --output mc.json
```

`sim:` block fields: `params` (ground truth), `b1`/`b0`/`r1`, `noise`
(`type: none|gaussian|rician`, `snr`), `seed`, `trials`, `sweep`
(sensitivity), `distributions` (montecarlo). Noise is `sigma = max(|signal|)/SNR`;
runs are reproducible for a fixed `seed`.

## Common options

| Flag | Meaning |
|---|---|
| `--config <file>` | protocol/fitting config (required) |
| `--mat-dir <dir>` | auto-load `MTdata/R1map/B1map/B0map/Mask.mat` |
| `--data` / `--mat-data` | single 4D NIfTI / `.mat` input |
| `--mask`, `--r1map`, `--b1map`, `--b0map` | individual maps (`.mat` or NIfTI) |
| `--output-dir <dir>` | where maps are written (default `./FitResults`) |
| `--threads <n>` | worker threads (default: all cores) |

Fitting shows a live progress bar (elapsed, throughput, ETA); it auto-hides when
output is redirected to a file.

## Notes on comparing against qMRLab

- For `.mat` inputs, output maps are written with a `make_nii`-compatible header
  (2D, sform origin at voxel (1,1,1)) so they **overlay and subtract voxel-exactly**
  against qMRLab's `FitResults`.
- Match the sub-model: comparing the wrong one dominates the differences.
- For qMT, `kf`/`T2f`/`T2r` agree tightly (~1–4%); `F`/`kr` are individually
  ill-conditioned and best compared on averages/trends, not per voxel.
- Validate the Sf table (SledPikeRP) against qMRLab's:
  ```bash
  ./target/release/qmrust dump-sf --config prots/qmt_config_sledpikerp.yaml --output sf.bin
  ```
