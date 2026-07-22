# Adding a model

qmrust fits several qMRI models (inversion-recovery T1, qMT-SPGR, ...) and is
designed so that adding another one touches exactly two places: a new module,
and one line in a registry. Nothing else — not the CLI, not the config
parser, not the fitting engine, not the simulator — needs to know a new model
exists.

## Why: two seams, not scattered branches

- **The `Model` trait** (`qmrust_core::core::model::Model`) is the whole
  contributor surface. It is object-safe (used as `Box<dyn Model>`), so no
  generics or associated types — just methods: `param_names`, `output_names`,
  `param_bounds`, `fixed_mask`, `required_inputs`, `measurement`, `forward`,
  `fit`, and optionally `strategy`/`bids`/`protocol_schema`/`bids_outputs`.
  The core only ever sees ordered `f64`s, an identity-keyed `Measurement`,
  and a scalar `Aux` bundle (`aux.get("B1map")`) — never files, JSON, or a
  config format. That's what keeps it portable to wasm.
- **The registry** (`registry::all()` in `registry.rs`) is the whole dispatch
  point: a static list of `ModelEntry { name, bids_suffix, build, describe,
  dump }`. The CLI, the simulator, and the wasm bindings all resolve a model by
  calling `registry::by_name` — there is no `match cfg.model { ... }` anywhere
  else in the codebase.

## Checklist: add a model

1. New directory `crates/qmrust-core/src/models/<name>/`: a `config.rs` (a
   `serde` struct implementing `ModelConfig`), the pure math, and a `model.rs`
   (`impl Model` + the one-line `build`/`describe`/`dump` entry points that
   delegate to the shared pipeline).
2. Register the module in `models/mod.rs`.
3. Add **one** `ModelEntry` to `registry::all()` in `registry.rs` (name + BIDS
   suffix + `build` + `describe` + `dump`).
4. Add tests: a forward → fit round-trip, and config parse/validate.

That's it. Use `models/inversion_recovery/` as the minimal reference model
(its `protocol_schema()` maps `InversionTime` off the BIDS sidecar);
`models/qmt_spgr/` shows a nested-config model that also declares auxiliary
inputs (B1/B0/R1 maps) and a `Series` measurement keyed by `(Angle, Offset)`.

## Inputs are BIDS

qmrust fits BIDS or BIDS-like layouts only — `qmrust fit --bids-dir <dir>`,
or `--mat-dir`/`--mat-data` for qMRLab `.mat` data converted to BIDS via
`qmrust bidsify`. The `rust-bids` crate parses the whole dataset — the raw
tree *and* every `derivatives/<pipeline>/` — into one flat table
(`parse_to_table`), then groups matching files into `Collection`s per a
declarative grouping config (`BidsConfig`). Everything downstream (grouping,
auxiliary-input resolution, mask resolution) is just a query over that table
via `table_filter(rows, &[(column, value)])` — nothing model- or
dataset-specific. See
[`docs/agents/DATA-PIPELINE.md`](agents/DATA-PIPELINE.md) for the full
walkthrough.

## Declaring your model's BIDS contract

A model doesn't read files or JSON; it declares what it needs, and the shell
(`rust-bids` + the CLI) fulfills the declaration:

- **`measurement() -> MeasurementKind`** — the shape of data the model reads.
  `Named { roles }` is a fixed set of role-labeled volumes (e.g. an MTS-style
  model's `["PDw", "MTw", "T1w"]`); `Series { rows }` is a variable-length
  series whose canonical per-volume identity rows the model itself owns, in
  its own order (e.g. IR's one `{"InversionTime": ti}` row per TI). Both are
  identity-keyed: the shell tags each data volume with a `VolumeId`, and the
  engine matches supplied volumes to the model's declared identities *by
  value*, never by array position — reordering the acquisition list yields
  the identical fit.
- **`protocol_schema() -> Vec<ProtoParam>`** (default `vec![]`) — how each
  acquisition parameter is resolved from a BIDS sidecar (or `--config`):
  `Source::Field(key)` reads a value straight off the sidecar;
  `Source::Derived(fn(&dyn Meta) -> Result<f64>)` computes one from several
  sidecar fields (a pure, image-scoped fn, not a closure, so `Model` stays
  object-safe); `Source::Option(key)` is the non-BIDS fallback, read from
  `--config` instead. `Scope::PerVolume` resolves once per volume,
  `Scope::Global` once for the whole collection. IR's is minimal:

  ```rust
  fn protocol_schema(&self) -> Vec<ProtoParam> {
      vec![ProtoParam {
          name: "InversionTime",
          source: Source::Field("InversionTime"),
          scope: Scope::PerVolume,
      }]
  }
  ```

  A model that skips `protocol_schema()` resolves to an empty `Protocol` and
  falls back to reading its own `--config` as before — this is opt-in, not a
  breaking requirement.
- **`required_inputs() -> Vec<InputSpec>`** — auxiliary scalar maps (B1/B0/R1,
  ...) the model reads via `aux.get(name)`. Each `InputSpec { name, required,
  bids }` optionally carries a `BidsMap { suffix, entity }` locating it in a
  BIDS dataset. qMT declares:

  ```rust
  fn required_inputs(&self) -> Vec<InputSpec> {
      vec![
          InputSpec { name: "R1map", required: false, bids: Some(BidsMap { suffix: "R1map",  entity: None }) },
          InputSpec { name: "B1map", required: false, bids: Some(BidsMap { suffix: "TB1map", entity: None }) },
          InputSpec { name: "B0map", required: false, bids: Some(BidsMap { suffix: "B0map",  entity: None }) },
      ]
  }
  ```
- **`bids_outputs() -> Vec<(&'static str, &'static str, &'static str)>`**
  (default `vec![]`) — which `output_names()` entries are genuine
  quantitative maps worth exporting as BIDS derivatives: a 3-tuple of
  `(output_name, BIDS-derivatives suffix, unit)`, e.g. IR's
  `[("T1", "T1map", "s")]` or qMT's `[("F", "Fmap", ""), ("kr", "kRmap",
  "1/s"), ...]`. `""` marks a unitless quantity. Diagnostics (residuals,
  scenario indices, ...) are omitted — only real maps are listed. This is
  what `qmrust fit --bids-dir` uses to write `derivatives/qmrust/...`.
- **`bids() -> Option<BidsSpec>`** — the model's own BIDS grouping identity:
  its suffix and the entities that index its acquisition axis.

## Customizing the layout for non-official BIDS

Most qMRI protocols (qMT-SPGR, MTsat, ...) aren't in the official BIDS-MRI
suffix list. Rather than hardcoding exceptions, non-official layout facts are
*declared* in the grouping config (`BidsConfig`, `rust-bids/src/config.rs`):

- `custom_suffixes: [QMTSPGR]` — a non-official suffix that should still be
  discovered (and stays exempt from `.bidsignore`, so general-purpose BIDS
  validators can be kept quiet with a `.bidsignore` entry without qmrust
  losing the files).
- `custom_entities: [{ key: cest, name: cestPool }]` — a non-official entity
  key, mapped to the full name the file table stores it under.

A registered model's own `bids_suffix` (e.g. `QMTSPGR`) is already known at
compile time via `qmrust_core::registry::all()` — `Vocabulary::bids()` builds
that in with no config; `Vocabulary::from_config(cfg)` layers a dataset's
declared customs on top. An unrecognized suffix is still included in the
table (never dropped), just warned about — permissive but loud.

Grouping itself is one of three declarative shapes under `BidsConfig`:

```yaml
loop_over: [sub, ses, run, task]

custom_entities:
  - key: cest
    name: cestPool
custom_suffixes: [QMTSPGR]

IRT1:
  sequential_set:
    by: [inv]

QMTSPGR:
  sequential_set:
    by: [mt, flip]

MTS:
  named_set:
    PDw: { flip: "1", mt: "off" }
    MTw: { flip: "1", mt: "on" }
    T1w: { flip: "2", mt: "off" }
    required: [PDw, MTw, T1w]
```

- `loop_over` — the entities that define one fittable "unit" (a collection).
- `sequential_set: { by: [...] }` — an ordered series along one or more
  entities (IR's inversion series; qMT's flip/offset series).
- `named_set: { <role>: { <entity>: <value>, ... }, required: [...] }` —
  fixed, role-labeled volumes matched by entity constraints, with a
  `required` list of which roles must all be present.

See [`docs/agents/DATA-PIPELINE.md`](agents/DATA-PIPELINE.md) for the full
`Vocabulary`/grouping mechanics.

## Auxiliary maps and the mask

A fit resolves each of a model's `required_inputs()` entries from the
dataset's flat table by the collection's full identity (subject/session/run/
...) plus the declared BIDS suffix (and entity, if any) — found in the raw
tree or in *any* `derivatives/<pipeline>/`. A missing `required: true` input
is a hard build-time error; a missing optional one just leaves the model to
its own default.

The brain mask works the same way but is declared separately, in `--config`
under a `mask:` block, because a dataset can carry more than one mask (brain,
tissue, lesion, ...) and auto-picking one would be a silent guess:

```yaml
mask:
  desc: brain
```

`suffix` defaults to `mask`; every other key is an entity constraint (short
keys like `desc` are normalized to their full BIDS name). An under-specified
`mask:` that still matches several files is a hard error, never a silent
pick; an absent `mask:` block means no masking at all.

## Units

qmrust works natively in BIDS/SI units, end to end: time (`RepetitionTime`,
`EchoTime`, `InversionTime`, and any fitted time constant such as T1/T2) in
**seconds**, frequency in **Hz**, field in **tesla**, angle in **radians**
except BIDS-MRI's `FlipAngle`, which is **degrees**. There is no internal
ms↔s round-tripping — a non-BIDS source (a qMRLab `.mat` in milliseconds) is
converted once, at the shell boundary (`bidsify` / `.mat` load), so
`qmrust-core` only ever sees BIDS units. Fitted maps therefore differ from
qMRLab's own `FitResults` by the unit factor (qMRLab T1 in ms = qmrust T1 in
s × 1000) — validation against qMRLab references must reconcile that factor,
never expect raw equality. `bids_outputs()` records each map's SI unit
explicitly.

## Going deeper

For the full trait definition, the supporting value types (`Aux`,
`InputSpec`, `FitStrategy`, `Protocol`, `MeasurementKind`, `BidsSpec`), and a
worked line-by-line example, see
[`docs/agents/ARCHITECTURE.md`](agents/ARCHITECTURE.md#the-model-trait--the-single-contributor-surface).
For the BIDS layout/sidecar/protocol machinery in depth, see
[`docs/agents/DATA-PIPELINE.md`](agents/DATA-PIPELINE.md).
