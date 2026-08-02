# qmrust

qmrust is a native-Rust toolkit for quantitative MRI. Point it at a BIDS
dataset and it fits qMRI models — inversion-recovery T1, multi-echo T2, MTR,
MTsat, qMT-SPGR — as a fast command-line tool or directly in your browser via
WebAssembly, with identical numbers either way.

```bash
qmrust fit --bids-dir ds-mydata --config recipes/bids/irt1_config.yaml \
  --output-dir out
```

**New here?** [Getting started](getting-started.md) builds the CLI and runs
your first fit. **Curious first?** The [playground](playground.md) fits real
data in this page, in your browser.

## Methods

<!-- BEGIN generated: model gallery -->
### T1 Relaxometry

::::{grid} 1 1 2 2
:::{card}
:header: Inversion Recovery
:link: models/t1-relaxometry/inversion_recovery.md

Fits the longitudinal relaxation time T1 from a series of inversion-recovery images acquired at different inversion times.

`IRT1` · `T1map`
:::
:::{card}
:header: Variable Flip Angle
:link: models/t1-relaxometry/vfa_t1.md

Fits the longitudinal relaxation time T1 from spoiled gradient-echo images acquired at two or more excitation flip angles and a single repetition time.

`VFA` · `T1map`, `M0map`
:::
::::

### T2 Relaxometry

::::{grid} 1 1 2 2
:::{card}
:header: Monoexp T2
:link: models/t2-relaxometry/mono_t2.md

Fits the transverse relaxation time T2 from a multi-echo spin-echo series as a mono-exponential decay with a free amplitude.

`MESE` · `T2map`, `M0map`
:::
::::

### Field Mapping

::::{grid} 1 1 2 2
:::{card}
:header: Actual Flip Angle B1+
:link: models/field-mapping/b1_afi.md

Maps the transmit (B1+) field from a single spoiled gradient-echo sequence that interleaves two excitation repetition times at one nominal flip angle.

`TB1AFI` · `TB1map`
:::
:::{card}
:header: Double Angle B1+
:link: models/field-mapping/b1_dam.md

Maps the transmit (B1+) field from two spoiled gradient-echo volumes acquired at flip angles alpha and twice alpha.

`TB1DAM` · `TB1map`
:::
::::

### Magnetization Transfer

#### Semi-quantitative MT

::::{grid} 1 1 2 2
:::{card}
:header: MT Ratio
:link: models/magnetization-transfer/mt_ratio.md

Computes the magnetization transfer ratio from two images: one acquired with an off-resonance saturation pulse and one without.

`MTR` · `MTRmap`
:::
:::{card}
:header: MT Saturation
:link: models/magnetization-transfer/mt_sat.md

Derives the MT saturation parameter from three spoiled gradient-echo volumes — MT-weighted, PD-weighted and T1-weighted.

`MTS` · `MTsat`, `T1map`, `MTRmap`
:::
::::

#### Quantitative MT

::::{grid} 1 1 2 2
:::{card}
:header: qMT-SPGR
:link: models/magnetization-transfer/qmt_spgr.md

Two-pool quantitative magnetization transfer from a spoiled gradient-echo sequence with off-resonance saturation sampled across a grid of saturation flip angles and frequency offsets.

`QMTSPGR` · `Fmap`, `kRmap`, `R1Fmap`, `R1Rmap`, `T2Fmap`, `T2Rmap`
:::
::::
<!-- END generated: model gallery -->

## Guides

- [Getting started](getting-started.md) — build the CLI, run a fit, read the outputs.
- [BIDS](guide/bids.md) — how collections, sidecars, auxiliary maps and masks are resolved.
- [Fitting without BIDS](guide/non-bids.md) — 4D NIfTI and qMRLab `.mat` inputs.
- [Simulation](guide/simulation.md) — forward signals, sim→fit round-trips, sensitivity.
- [Browser & wasm](guide/browser.md) — the same core, compiled to WebAssembly.

## Contributing

qmrust is a functional core with an imperative shell: the numerical models are
pure Rust and run unchanged natively and in wasm. See
[Architecture](dev/architecture.md) and [Adding a model](dev/adding-a-model.md).
