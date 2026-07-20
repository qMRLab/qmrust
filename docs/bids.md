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
`[subject, session, run, task]`) and a map of named `sets`, each one of:

- **`plain_set`** — one file per unit, optionally with extra extensions or
  cross-modal includes.
- **`named_set`** — several files distinguished by entity constraints, e.g.
  qMT's MTS set groups `PDw`/`MTw`/`T1w` by `flip`/`mtransfer` entity values,
  with a `required: [...]` list of which named groups must all be present.
- **`sequential_set`** — an ordered series along one or more entities, e.g.
  IRT1's inversion-recovery series ordered `by: [inversion]`.

Today the grouping config is a **bundled default** (`default_config()`,
covering `IRT1` and `MTS`) plus a programmatic `parse_config(yaml: &str)` for
supplying your own. A discoverable, on-disk `rust-bids.yaml` convention is
planned but not yet implemented.

## The I/O seam

All filesystem access goes through the `fs::DatasetFs` trait rather than
`std::fs` directly. That's what lets the same resolver run against a native
filesystem walker in the CLI today, and — unchanged — against a browser-side
(e.g. Tauri/JS) directory listing in the future, alongside `qmrust-wasm`.

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
back to reading its own `--config` as before.

## Units

Sidecar fields resolved through `protocol_schema()` are read as-is — qmrust expects them
in BIDS/SI units, so an `InversionTime` sidecar value of `0.35` means 350 ms, and the
resulting fitted map (e.g. `T1map`) is in seconds too. See the "Units — BIDS-native (SI)"
principle in [`CLAUDE.md`](../CLAUDE.md) for the full rule and how it differs from
qMRLab (milliseconds).
