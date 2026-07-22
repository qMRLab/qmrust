# BIDS datasets (`rust-bids`)

If your qMRI data is already organized as a BIDS dataset, `rust-bids` is what
turns a directory of loose files into the grouped volumes a model needs to
fit — e.g. the three flip/mtransfer combinations an MTS model requires, or
the ordered inversion-time series an IR model requires. It's a separate,
wasm-clean crate (not part of `qmrust-core`, but a consumer of it) because the
layout-resolution problem is generalizable beyond this workspace.

## Two layers

1. **Layer 1 — `parse_to_table`** walks a dataset and produces a flat table
   of rows (`BidsRow`): one row per file, with its BIDS filename entities
   (subject/session/suffix/…) and any sidecar JSON fields already parsed out.
   No grouping logic yet — just "what files exist and what do their names/
   sidecars say".
2. **Layer 2 — `resolve_set` / `collections_for`** groups those rows into
   `Collection`s according to a declarative grouping config (`BidsConfig`).
   Mismatches (a missing required file, an entity that doesn't parse) produce
   a `Warning` attached to the `Collection` rather than a panic — resolution
   is permissive-but-loud.

## The grouping grammar

A `BidsConfig` has a `loop_over` (which entities define one "unit", e.g.
`[sub, ses, run, task]`) and a map of named `sets`, each one of:

- **`plain_set`** — one file per unit, optionally with extra extensions or
  cross-modal includes.
- **`named_set`** — several files distinguished by entity constraints, e.g.
  qMT's MTS set groups `PDw`/`MTw`/`T1w` by `flip`/`mtransfer` entity values,
  with a `required: [...]` list of which named groups must all be present.
- **`sequential_set`** — an ordered series along one or more entities, e.g.
  IRT1's inversion-recovery series ordered `by: [inv]`.

The grouping config is a **bundled default** (`default_config()`, covering
`IRT1`, `MTS`, and `QMTSPGR`); supply your own on-disk manifest with
`fit --grouping <file>`, or `parse_config(yaml: &str)` programmatically.

`QMTSPGR` (qMT-SPGR) is a **custom, non-official** BIDS suffix — it isn't
part of the BIDS-MRI spec, so a `QMTSPGR` dataset ships a root `.bidsignore`
containing `*QMTSPGR*` to keep general-purpose BIDS validators quiet. Layout
resolution discovers it anyway: a path whose suffix matches a *registered*
model (`qmrust_core::registry::by_bids_suffix`) is never dropped by
`.bidsignore`, only genuinely unrelated ignored paths are. It's grouped as a
`sequential_set` ordered `by: [mt, flip]` — the 2 flip angles × 5 offsets
qMRLab's `qmt_spgr_batch` convention produces
(`sub-<subject>_flip-<f>_mt-<m>_QMTSPGR.nii.gz`) — but that ordering is
cosmetic: `qmt_spgr` reads each volume's identity from its sidecar's
`Angle`/`Offset` fields (see below), so a fit is correct regardless of file
order.

## Non-official entities and suffixes

The reader parses filenames against a **vocabulary** of the canonical BIDS
entities, suffixes, and datatypes (`rust_bids::Vocabulary`), extended with every
registered model's own suffix (so `QMTSPGR` is known with no configuration). A
dataset that uses non-standard terms declares them in its grouping config rather
than relying on anything hardcoded:

```yaml
custom_suffixes: [QMTSPGR]              # discoverable + .bidsignore-exempt
custom_entities:
  - { key: cest, name: cestPool }       # short key -> full column name
```

An unrecognized suffix is still read into the table (nothing is silently
dropped) but flagged with a warning. Every input a fit consumes — B1/B0/R1 maps,
and the brain **mask** — is located from this table by the collection's identity
plus a declared suffix (and, for the mask, entity constraints given in the
`--config` `mask:` block, e.g. `mask: { desc: brain }`), found wherever it lives
in the raw tree or any `derivatives/<pipeline>/`. See
[Adding a model](models.md) for the full contributor-facing customization.

## The I/O seam

All filesystem access goes through the `fs::DatasetFs` trait rather than
`std::fs` directly. That's what lets the same resolver run against a native
filesystem walker in the CLI, and the same code runs unchanged against a
browser-side (e.g. Tauri/JS) directory listing alongside `qmrust-wasm`.

## From sidecar metadata to `Protocol`

Each image's full metadata is captured as a `Sidecar` (`sidecar::sidecar_for`): the
co-located JSON merged with any inherited parent-level JSON along the BIDS directory
chain (dataset root → `sub-` → `[ses-]` → datatype directory), with the co-located file
winning ties. A model declares which sidecar fields feed its protocol via
`protocol_schema() -> Vec<ProtoParam>` — each parameter is either a direct sidecar
`Field(key)`, a value `Derived` from several sidecar fields by a pure, image-scoped
function, or a non-BIDS `Option(key)` read from `--config` instead. `protocol::
resolve_protocol` evaluates that schema against each volume's `Sidecar` (and the
`--config` options, for any `Option` fallback) to build a `qmrust_core::Protocol` — the
same acquisition-metadata shape a model reads its protocol from — so grouped BIDS volumes
feed directly into the existing fitting shell described in [Architecture](architecture.md).
A model with no declared `protocol_schema()` resolves to an empty `Protocol`, falling
back to reading its own `--config` as before. `qmt_spgr` declares `Angle` and `Offset`
(both `PerVolume`, read straight off each `QMTSPGR` volume's sidecar), mirroring IR's
`InversionTime` — the fit matches samples to the model's canonical rows by these values,
not by file order.

## Units

Sidecar fields resolved through `protocol_schema()` are read as-is — qmrust expects them
in BIDS/SI units, so an `InversionTime` sidecar value of `0.35` means 350 ms, and the
resulting fitted map (e.g. `T1map`) is in seconds too. See the "Units — BIDS-native (SI)"
principle in [`CLAUDE.md`](../CLAUDE.md) for the full rule and how it differs from
qMRLab (milliseconds).
