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
### Magnetization transfer

::::{grid} 1 1 2 2
:::{card}
:header: Magnetization transfer ratio
:link: models/magnetization-transfer/mt_ratio.md

Computes the magnetization transfer ratio from two images: one acquired with an off-resonance saturation pulse and one without.

`MTR` · `MTRmap`
:::
:::{card}
:header: MT saturation
:link: models/magnetization-transfer/mt_sat.md

Derives the MT saturation parameter from three spoiled gradient-echo volumes — MT-weighted, PD-weighted and T1-weighted.

`MTS` · `MTsat`, `T1map`, `MTRmap`
:::
:::{card}
:header: qMT-SPGR
:link: models/magnetization-transfer/qmt_spgr.md

Two-pool quantitative magnetization transfer from a spoiled gradient-echo sequence with off-resonance saturation sampled across a grid of saturation flip angles and frequency offsets.

`QMTSPGR` · `Fmap`, `kRmap`, `R1Fmap`, `R1Rmap`, `T2Fmap`, `T2Rmap`
:::
::::

### Relaxometry

::::{grid} 1 1 2 2
:::{card}
:header: Mono-exponential T2
:link: models/relaxometry/mono_t2.md

Fits the transverse relaxation time T2 from a multi-echo spin-echo series as a mono-exponential decay with a free amplitude.

`MESE` · `T2map`, `M0map`
:::
:::{card}
:header: Inversion recovery T1
:link: models/relaxometry/inversion_recovery.md

Fits the longitudinal relaxation time T1 from a series of inversion-recovery images acquired at different inversion times.

`IRT1` · `T1map`
:::
:::{card}
:header: Variable flip angle T1
:link: models/relaxometry/vfa_t1.md

Fits the longitudinal relaxation time T1 from spoiled gradient-echo images acquired at two or more excitation flip angles and a single repetition time.

`VFA` · `T1map`, `M0map`
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
