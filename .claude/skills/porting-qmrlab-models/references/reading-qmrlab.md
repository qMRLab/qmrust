# Reading a qMRLab model class

A qMRLab model is a MATLAB class under `qMRLab/Models/<Category>/<Model>.m`
(one file per model, often with a `<Model>_fun.m` or private helper files
alongside it). Every model subclasses `AbstractModel`, so the members you
need are always in the same places.

## Where the members live

- **`properties` block** — the class's declared state:
  - `xnames` — parameter names, in fitter output order.
  - `st` — start values for the fit, one per `xnames` entry.
  - `lb` / `ub` — lower/upper bounds, one pair per `xnames` entry.
  - `fx` — logical mask of which parameters are held fixed (not fitted).
  - `Prot` — the protocol struct: acquisition parameters the model needs
    (echo times, inversion times, flip angles, …), each with its own units
    and, often, a table of column names.
  - `buttons` — UI-exposed options (fit method, algorithm choice, on/off
    switches). These become the model's own config fields, not `Prot`.
  - `voxelwise` — whether the model fits per-voxel or needs neighborhood/
    whole-volume context; maps to `qmrust`'s `FitStrategy`.
- **`equation(obj, x, Opt)` method** — the forward signal model: given
  parameter values `x` (and sometimes an `Opt` struct), returns the
  simulated signal. This is what `qmrust`'s `forward(params, aux)` must
  reproduce.
- **`fit(obj, data)` method** — the per-voxel (or per-ROI) fit: takes
  measured `data`, returns fitted parameter values. This is what `qmrust`'s
  `fit(measurement, aux)` must reproduce, algorithm included — not just the
  forward equation.
- **Constructor / `UpdateFields`** — options-derived state computed once
  from `buttons`/`Prot` rather than stored directly (e.g. a derived grid,
  a method flag resolved from a button string). Anything computed here that
  the fit or equation depends on must show up in the qmrust config's
  `validate_options`/protocol-ingestion step, not be recomputed ad hoc
  inside the fitter.

Read the `properties` block and both methods before writing anything down —
`xnames` order, `st`/`lb`/`ub`/`fx` order, and the fitter's own output order
do not always match, and a silent transposition is a wrong port that still
compiles.

## The example-data fetch

Every qMRLab model defines how to obtain its demo dataset — record the exact
mechanism, since Phases 4-5 (bidsify, validate) depend on it:

- an `onlineData` property on the class: a URL (typically an OSF link)
  pointing at a zip of sample data, and often a matching `FitResults`
  reference output;
- and/or a demo driver `<Model>_batch.m` in the model's folder, which calls
  `qMRgenBatch(Model)` — this downloads/unzips the `onlineData` archive,
  builds the model's own `Prot`, and runs the fit to produce `FitResults`.

Read `onlineData` (or the batch script if the property is absent) and note:
the URL, what's inside the archive (raw measurement + protocol + optional
`FitResults`), and any qMRLab-side unit or naming convention the archive
uses. This is the same dataset `qmrust bidsify` will later convert.

## Unit traps (qMRLab -> BIDS-native SI)

qMRLab is not BIDS-native; qmrust is. The class's numbers are usually in
whatever unit qMRLab's UI displays, not BIDS units:

- **Time is typically milliseconds** in `Prot`/`st`/`lb`/`ub` (echo time,
  inversion time, repetition time, fitted T1/T2). BIDS-native — and
  `qmrust-core` — uses **seconds**.
- **Angles may be degrees** (flip angle in some `Prot` tables); BIDS-MRI's
  own `FlipAngle` field is degrees too, but any angle qmrust computes with
  internally is radians.
- **Frequency offsets are Hz** in both qMRLab and BIDS — no conversion
  trap there, but confirm the class isn't secretly using rad/s.

The rule: convert at the shell boundary, never inside `qmrust-core` (see
`CLAUDE.md` "Units" and `docs/agents/ADDING-A-MODEL.md`'s invariants list).
Concretely, that means the conversion happens in the CLI/bidsify/sidecar
code that reads a qMRLab `.mat` or constructs BIDS sidecars — the core
model's `forward`/`fit` never multiply or divide by 1000, and validation
against qMRLab's own `FitResults` must account for the unit difference
rather than expecting raw numerical equality.

## Worked read: qMRLab `IR` onto `inversion_recovery`

qMRLab's `Models/T1/IR.m` (Inversion Recovery) class members map onto
`crates/qmrust-core/src/models/inversion_recovery/` as follows:

| qMRLab `IR.m`                                                    | qmrust                                                                                   |
| ----------------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| `xnames = {'T1','b','a'}` (fit output order)                     | `IrFitter::output_names()` in `crates/qmrust-core/src/models/inversion_recovery/fit.rs` — `["T1","b","a",...]`, the per-voxel fit-output order; this is the qMRLab-equivalent name list, **not** `param_names()` |
| (equation's own parameter order — qMRLab passes `x` positionally into `equation`, order implied by `st`/`lb`/`ub`) | `IrFitter::param_names()` in the same file — `["T1","a","b"]`, the `forward()`/equation argument order. Different from `output_names()`'s `a`/`b` order: a silent transposition between the two is exactly the hazard flagged above |
| `Prot.IRData.Mat` (inversion times, ms)                          | `IrConfig.inversion_times: Vec<f64>` (seconds) in `crates/qmrust-core/src/models/inversion_recovery/config.rs`, surfaced via `protocol_schema()` / `IrModel::bids_volume` (`InversionTime` sidecar key) in `crates/qmrust-core/src/models/inversion_recovery/model.rs` |
| `buttons` — fit method ("Magnitude"/"Complex"), T1 search range, zoom factor/points | `IrConfig.method: Option<FitMethod>`, `t1_range: T1Range`, `zoom: ZoomConfig` in `crates/qmrust-core/src/models/inversion_recovery/config.rs` |
| `equation(obj, x)`: `S(TI) = a + b*exp(-TI/T1)`, `abs(...)` for magnitude | `IrFitter::forward` in `crates/qmrust-core/src/models/inversion_recovery/fit.rs` (same equation, seconds-native `TI`/`T1`) |
| `fit(obj, data)`: RD-NLS grid search + zoom refinement (Barral et al. 2010), `rdNls`/`rdNlsPr` | `rd_nls` / `rd_nls_pr` in `crates/qmrust-core/src/models/inversion_recovery/fit.rs`, dispatched by `IrFitter::fit_voxel` on `FitMethod` |
| `st`/`lb`/`ub` for T1 (search range, ms) | `IrConfig.t1_range: T1Range` (`start`/`stop`/`step`, seconds) — RD-NLS is a grid search, not a bounded gradient fit, so this range *is* the search grid rather than a bounds pair |
| `fx` (no qMRLab-exposed fixed parameters for IR) | `IrModel::fixed_mask()` returns `vec![false; 3]` in `crates/qmrust-core/src/models/inversion_recovery/model.rs` |
| `voxelwise = 1`                                                   | `IrModel::strategy()` returns `FitStrategy::Voxelwise` |
| `onlineData` URL (OSF IR dataset + `FitResults/T1.nii.gz`)        | fetched and converted by `qmrust bidsify`, then fit via `qmrust fit --bids-dir`; validated against `FitResults` in the `#[ignore]`d `bids_fit_matches_mat_fit` test (see `CLAUDE.md`) |

Two unit conversions happen only at the shell boundary, never inside
`fit.rs`/`model.rs`: qMRLab's inversion times and T1 search range are
milliseconds; `IrConfig`'s `inversion_times`/`t1_range` are seconds. The
core fit is scale-consistent (`build_nls_struct`'s doc comment in
`crates/qmrust-core/src/models/inversion_recovery/fit.rs` notes this
explicitly) — whatever unit TI and the T1 grid share is the unit the
fitted T1 comes out in, so the conversion is a pure multiply-by-0.001
done once when the config is built from a qMRLab `.mat`/sidecar source,
not a per-voxel operation.
