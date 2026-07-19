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

## From `Collection` to `Protocol`

`protocol::protocol_for` turns a resolved `Collection` into a
`qmrust_core::Protocol` (the same acquisition-metadata shape a model reads its
protocol from), so grouped BIDS volumes can feed directly into the existing
fitting shell described in [Architecture](architecture.md).
