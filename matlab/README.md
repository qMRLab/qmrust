# mtsat_b1 MATLAB reference harness

`mtsat_b1_reference.m` runs TardifLab's own MATLAB Bloch simulation
(`BlochSimFlashSequence_v2`) over a fixed parameter grid and writes a CSV. That
CSV is the ground truth qmrust's Rust engine (`crates/qmrust-core/src/mtsat_b1/`)
is checked against, so the numbers it produces need to come from an unmodified
run of the upstream MATLAB toolboxes, not from qmrust.

## 1. Get the three required toolboxes

None of these ship with qmrust — clone them somewhere on your machine (any
directory; paths below use `~/matlab-deps` as an example):

```bash
mkdir -p ~/matlab-deps && cd ~/matlab-deps
git clone https://github.com/TardifLab/OptimizeIHMTimaging.git
git clone https://github.com/qMRLab/qMRLab.git
git clone https://github.com/TardifLab/NeuroImagingMatlab.git
```

## 2. Add them to your MATLAB path

In MATLAB:

```matlab
addpath(genpath('~/matlab-deps/OptimizeIHMTimaging'))
addpath(genpath('~/matlab-deps/qMRLab'))
addpath(genpath('~/matlab-deps/NeuroImagingMatlab'))
```

(`OptimizeIHMTimaging` ships its own `setupIHMTsimPaths.m` — running that
instead is fine too, as long as all three toolboxes end up on the path. The
script only calls `BlochSimFlashSequence_v2` and `getPulseB1rms` directly, but
both pull in helpers from all three toolboxes transitively, so all three must
be on path even though you won't call most of them yourself.)

No other setup, data, or license is needed — everything the script needs is a
pure simulation, there are no images or subjects to download.

## 3. Run it

```matlab
cd /path/to/qmrust/matlab
mtsat_b1_reference
```

It prints one progress line per grid point (with a running elapsed-time
readout) and a total run time at the end. Expect a fairly minor CPU-bound
simulation: 80 grid points x 3 acquisitions (MTw, PDw, T1w) each running a
~6-second-of-steady-state Bloch simulation, so plan for it to take at least a
few minutes; the printed timings tell you exactly how long it took on your
machine.

## 4. What it generates

`matlab/mtsat_b1_reference.csv` — one row per `(M0b, b1, R1)` grid point (80
rows total: 5 M0b values x 4 b1 values x 4 R1 values), columns:

```
M0b,b1,R1,satFlipAngle,sig_MTw,sig_PDw,sig_T1w
```

- `M0b`, `b1` (µT rms), `R1` (1/s) — the grid point.
- `satFlipAngle` — the saturation-pulse nominal flip angle (degrees) the
  script solved so the pulse's RMS B1 equals `b1`.
- `sig_MTw`, `sig_PDw`, `sig_T1w` — the simulated FLASH signal magnitudes for
  the MT-weighted acquisition and the two VFA acquisitions (PDw low-flip,
  T1w high-flip) used for the Helms apparent-R1/A0 calculation.

## 5. What to send back

Just the one file: **`matlab/mtsat_b1_reference.csv`**. Do not commit it —
send it directly (e.g. attach it in Slack/email, or drop it in a shared
folder). A separate Rust test on qmrust's side loads that CSV and compares it
voxel-by-voxel against qmrust's own simulated signals for the same grid; any
mismatch beyond a small numeric tolerance flags a porting regression.

If anything in the script's output looks obviously wrong (NaNs, a MATLAB error
about `TD < 0`, a crash inside `BlochSimFlashSequence_v2`), send the full
console output too — that's more useful for debugging than the CSV alone.

## Notes on fidelity (for the curious / for debugging mismatches)

The script's header comments document two known, non-obvious timing
subtleties in how `BlochSimFlashSequence_v2` accounts for real elapsed time
per TR versus how the Rust engine's `tr_fill` formula does — read those
comments (and the `TODO(collaborator)` markers) if the comparison test shows
a small but consistent offset rather than random noise.
