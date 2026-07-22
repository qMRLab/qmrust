---
name: porting-qmrlab-models
description: Use when porting or translating a qMRI model from qMRLab (MATLAB) into qmrust — guides reading the MATLAB class, translating the math into a pure Rust fitter, wiring it into qmrust, and validating against qMRLab's own example data. Phase-gated.
---

A qMRLab model is a MATLAB class; a qmrust model is a `Model` trait impl. Porting is
translating one subclass into another.

## The correspondence

| qMRLab (MATLAB class)                    | qmrust                                                        |
| ---------------------------------------- | ------------------------------------------------------------- |
| `AbstractModel` (base class)             | the `Model` trait **and** the `ModelConfig` trait             |
| `UpdateFields` / shared model machinery  | the `build_model::<C>` pipeline (the "template method")       |
| class property `xnames`                  | `param_names()` / `output_names()`                            |
| `st`, `lb`/`ub`, `fx`                    | fitter start values, `param_bounds()`, `fixed_mask()`         |
| `Prot` (protocol struct)                 | `protocol_schema()` + config arrays + `ingest_protocol`       |
| `buttons` / options                      | the model's own `Config` struct (`ModelConfig`)               |
| `equation(obj, x)` method                | `forward(params, aux)`                                        |
| `fit(obj, data)` method                  | `fit(measurement, aux)`                                        |
| optional data inputs (B1map, R1, …)      | `required_inputs()` (used-if-present) / `sim_required_aux()`  |
| example-data fetch (`onlineData` URL / demo `*_batch.m`) | the dataset `qmrust bidsify` converts, then fits |

Locate each class member in the `.m` files, place it into its qmrust counterpart, then
prove equivalence.

## Phases and gates

The skill is phase-gated: stop at each boundary for explicit human sign-off before
proceeding. This catches wrong-math-that-runs while it is still cheap.

**Phase 0 — Setup.** Obtain the qMRLab source: ask for a local qMRLab checkout path
(or read it from a configured location) and confirm the target model name. Record
how this model fetches its example data (an `onlineData` URL / demo `*_batch.m` /
`qMRgenBatch` download) — the port depends on it for the bidsify and validation gates.

**Phase 1 — Read the class.** Locate `equations`/`fit`/`Prot`/`xnames`/`st,lb,ub,fx`
and options in the `.m` files. Produce a written statement of the signal equation,
parameters, protocol, fit method, and every input (including optional image inputs)
with their units.
→ *Gate: confirm the equation and units.*

**Phase 2 — Translate.** Run `./scaffold_model.sh <name> <Suffix>`, then fill the four
`TODO(port)` markers (config fields, signal equation, fitter, protocol mapping). Write
the forward→fit round-trip test.
→ *Gate: confirm the translation; round-trip test passes.*

**Phase 3 — Wire.**
→ *Gate: `cargo test --workspace`, `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, and both
`cargo build --target wasm32-unknown-unknown` (`qmrust-core`, `rust-bids`) commands are
green.*

**Phase 4 — Fetch and bidsify example data.**
→ *Gate: bidsify succeeds and the BIDS layout is correct.*

**Phase 5 — Validate against qMRLab.**
→ *Gate: human reviews the delta and signs off.*

## Definition of done

Always required:

- forward→fit round-trip recovers known truth;
- `cargo test --workspace`, `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets -- -D warnings` pass;
- `cargo build -p qmrust-core --target wasm32-unknown-unknown` and
  `cargo build -p rust-bids --target wasm32-unknown-unknown` pass.

Required whenever the model's example data is fetchable (the normal case, since every
qMRLab model defines how to fetch it):

- `qmrust bidsify` converts the fetched example data into a correct BIDS layout —
  suffix, per-volume entities, and sidecars match `bids()`/`bids_volume()`, voxel data
  byte-identical to source;
- the model fits that bidsified data via `qmrust fit --bids-dir`.

Required when a qMRLab reference result exists (e.g. `FitResults` on OSF):

- the fitted maps match qMRLab's `FitResults` within a stated tolerance, accounting
  for unit differences (not raw numerical equality);
- the comparison is wired into the `oracle` test and/or `ci/integration_osf.sh`.

When no reference result exists:

- the round-trip, build, and bidsify gates are the bar, and the port records a
  documented validation gap (no silent claim of qMRLab numerical agreement).

## Pointers

Mechanical file checklist: `docs/agents/ADDING-A-MODEL.md`. Deep dives:
`references/reading-qmrlab.md`, `references/translation-patterns.md`,
`references/optional-inputs.md`, `references/validation.md`.
