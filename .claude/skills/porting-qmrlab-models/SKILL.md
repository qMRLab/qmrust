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
| class property `xnames` (fit-output order) | `output_names()` (`param_names()` is the `forward`-arg order) |
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

**Ask before you assume.** A gate is not a rubber stamp. At each one, surface every
unresolved assumption as an explicit question and get an answer before proceeding —
never silently guess when a choice would change the math or the output maps and the
qMRLab source does not settle it. The questions that most often decide a port:
- which model variant / config is intended (many qMRLab models have several);
- whether a quantity is in the units you assumed (ms vs s, degrees vs radians, Hz);
- which parameters are fitted vs fixed, and their bounds/start values;
- whether the model actually fits the voxelwise `forward`/`fit` paradigm, or needs a
  different shape — if the abstraction does not fit, improve the abstraction, do not
  special-case;
- what tolerance counts as agreement with qMRLab (Tier 3).

A wrong silent assumption here produces maps that look right and are wrong, so when in
doubt, ask — that is cheaper at every phase than discovering it at validation.

**Phase 0 — Setup.** Obtain the qMRLab source: ask for a local qMRLab checkout path
(or read it from a configured location) and confirm the target model name. It reads
`.m` files directly with Read/Grep. Record how this model fetches its example data
(an `onlineData` URL / demo `*_batch.m` / `qMRgenBatch` download) — the port depends
on it for the bidsify and validation gates.

**Phase 1 — Read the class.** Locate `equations`/`fit`/`Prot`/`xnames`/`st,lb,ub,fx`
and options in the `.m` files. Produce a written statement of the signal equation,
parameters, protocol, fit method, and every input (including optional image inputs)
with their units.
→ *Gate: confirm the equation and units.*

**Phase 2 — Translate.** Run `.claude/skills/porting-qmrlab-models/scaffold_model.sh <name> <Suffix>` (from the repo root), then fill the four
`TODO(port)` markers in `config.rs` (config fields), `fit.rs` (signal equation +
fitter), `model.rs` (protocol mapping / `bids()`), and the `default_grouping.yaml`
grouping block. Write the forward→fit round-trip test.
→ *Gate: confirm the translation; round-trip test passes.*

**Phase 3 — Wire.** Confirm the registry line and grouping block the scaffold added;
decide optional-input wiring per `references/optional-inputs.md`.
→ *Gate: `cargo test --workspace`, `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, and both
`cargo build --target wasm32-unknown-unknown` (`qmrust-core`, `rust-bids`) commands are
green.*

**Phase 4 — Fetch and bidsify example data.** Using the fetch mechanism from Phase 0,
download the model's example dataset and run `qmrust bidsify`; this exercises `bids()`,
`bids_volume()`, `required_inputs()`, and the `default_grouping.yaml` block, and
produces the dataset Phase 5 validates. Write **both** `--config` recipes now (see
`recipes/README.md`): `recipes/non-bids/<name>_config.yaml` **carries** the acquisition
arrays (bidsify reads them as the protocol fallback to write the sidecars; the non-BIDS
fit path reads them directly) and takes its mask via the `--mask` flag, while
`recipes/bids/<name>_config.yaml` **omits** them (the BIDS fit resolves the acquisition
from sidecars) and selects its mask with a `mask:` block. The distinction is not
cosmetic: the output provenance's `Parameters` block echoes the raw recipe verbatim, so
a BIDS fit run with a recipe that still lists the acquisition arrays duplicates the
per-volume axis that `Protocol` already records from the sidecars.
→ *Gate: bidsify succeeds, the BIDS layout is correct, and both recipes exist.*

**Phase 5 — Validate against qMRLab.** Fit the bidsified data via `qmrust fit
--bids-dir --config recipes/bids/<name>_config.yaml` and compare the maps to qMRLab's
`FitResults` for the same dataset.
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

- both `recipes/non-bids/<name>_config.yaml` (acquisition arrays present, `--mask` flag)
  and `recipes/bids/<name>_config.yaml` (acquisition arrays omitted, `mask:` block) exist;
- `qmrust bidsify` converts the fetched example data into a correct BIDS layout —
  suffix, per-volume entities, and sidecars match `bids()`/`bids_volume()`, voxel data
  byte-identical to source;
- the model fits that bidsified data via `qmrust fit --bids-dir` with the BIDS recipe,
  and its output provenance does not duplicate the acquisition axis into `Parameters`.

Required when a qMRLab reference result exists (e.g. `FitResults` on OSF):

- the fitted maps are compared voxelwise to qMRLab's `FitResults` within a stated
  tolerance, accounting for unit differences (not raw numerical equality);
- that comparison is wired into `ci/integration_osf.sh` (beyond its current non-empty
  check) — see `references/validation.md` for what counts and what does not.

When no reference result exists:

- the round-trip, build, and bidsify gates are the bar, and the port records a
  documented validation gap (no silent claim of qMRLab numerical agreement).

## Pointers

Mechanical file checklist: `docs/agents/ADDING-A-MODEL.md`. Deep dives:
`references/reading-qmrlab.md`, `references/translation-patterns.md`,
`references/optional-inputs.md`, `references/validation.md`.
