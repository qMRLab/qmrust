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
- which BIDS suffix and per-volume entities the inputs map to — settle this against the
  **BIDS qMRI appendix** (https://bids-specification.readthedocs.io/en/stable/appendices/qmri.html),
  the authority for qMRI grouping suffixes (`MTS`, `MTR`, `MP2RAGE`, `VFA`, …) and the
  entities that index them (`mt-on`/`mt-off`, `flip`, `inv`, `echo`). Prefer a canonical
  suffix over inventing one; a genuinely non-official suffix (like `QMTSPGR`) is the
  exception, not the default;
- what tolerance counts as agreement with qMRLab (Tier 3).

A wrong silent assumption here produces maps that look right and are wrong, so when in
doubt, ask — that is cheaper at every phase than discovering it at validation.

**Phase 0 — Setup.** Obtain the qMRLab source: ask for a local qMRLab checkout path
(or read it from a configured location) and confirm the target model name. It reads
`.m` files directly with Read/Grep. Record how this model fetches its example data
(an `onlineData` URL / demo `*_batch.m` / `qMRgenBatch` download) — the port depends
on it for the bidsify, validation, and app-integration gates.

**Phase 1 — Read the class.** Locate `equations`/`fit`/`Prot`/`xnames`/`st,lb,ub,fx`
and options in the `.m` files. Produce a written statement of the signal equation,
parameters, protocol, fit method, and every input (including optional image inputs)
with their units.
→ *Gate: confirm the equation and units.*

**Phase 2 — Translate.** Run `.claude/skills/porting-qmrlab-models/scaffold_model.sh <name> <Suffix>` (from the repo root), then fill the four
`TODO(port)` markers in `config.rs` (config fields), `fit.rs` (signal equation +
fitter), `model.rs` (protocol mapping / `bids()`), and the `default_grouping.yaml`
grouping block. Write the forward→fit round-trip test.

For a `Series` model, **the per-volume protocol params *are* the volume
identity** (`engine::build_volume_ids` builds them from `proto.volumes`), and
`forward` must tag its samples with exactly the same keys. So a quantity that is
constant across the series — a single TR, a fixed TE — belongs in
`Scope::Global`, not `Scope::PerVolume`: as a per-volume param it silently joins
the identity on one side only, and no predicted sample matches its volume. The
fit keeps working, because it queries one key by name; the only symptom is a
missing forward curve in the app. Assert the two agree, don't eyeball it:
```rust
// Compared by volume index: any drift between the protocol-derived identity
// and the sample `forward` emits for that volume fails here, immediately.
let ids = crate::engine::build_volume_ids(m.measurement(), &proto, m.n_volumes()).unwrap();
let sig = m.forward(&params, &Aux::new());
let samples = sig.series();
assert_eq!(samples.len(), ids.len());
for (id, sample) in ids.iter().zip(samples) {
    let crate::core::model::VolumeId::Params(row) = id else {
        panic!("a Series model must yield param-row identities")
    };
    assert_eq!(
        *row, sample.params,
        "volume identity {row:?} has no matching forward sample {:?}",
        sample.params
    );
}
```
`models/vfa_t1/model.rs` carries this as
`forward_samples_carry_the_volume_identities_the_bids_path_builds`; copy it and
swap the model.
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

**Phase 6 — Ship it in the app.** A model nobody can reach in the playground is
half-ported. The playground (`docs/playground/`) and `qmrust-wasm` are entirely
data-driven — no per-model JS, HTML, CSS, or wasm binding exists, and none should
be added. What the app needs is the model's *example dataset and payload*, both
generated:

1. Add the model's block to `scripts/make_bids_examples.sh` — an OSF `fetch`
   line, and a `ds-<lowercased suffix>` section that bidsifies, fits, and
   `assert_maps`-checks its outputs. Roots are named from the registry suffix, so
   there is no model→directory table to edit. Run it with `--zip`.
2. Regenerate the catalog and the playground payload:
   ```bash
   ./target/release/qmrust catalog > catalog.json
   python3 scripts/make_docs_figures.py --bids-dir <dataset parent> \
     --catalog catalog.json --bundle-slice
   ```
   This writes `docs/figures/<model>/*.webp`, `docs/playground/data/<model>.json`
   (+ its `.nii.gz` slices) and re-derives `docs/playground/data/index.json`,
   which is what the model picker reads. A model missing from `index.json` means
   its dataset root wasn't found — fix step 1, don't hand-edit the manifest.
3. Check the generated payload's `files.aux` lists every optional input the model
   declares, and that its `probes` agree with the CLI's own maps. An aux map that
   silently fails to resolve makes the app fit uncorrected while the CLI corrects
   — the maps then differ with nothing to point at.
4. **Rebuild the playground wasm before serving locally.** No wasm *source* edit
   is needed, but `docs/playground/pkg/` is a gitignored build artifact: a stale
   one carries the old registry and the app fails with `Unknown model: '<name>'`
   while still rendering the pre-baked slice, which reads like a data problem and
   is not. CI rebuilds it on deploy, so this bites local testing only.
   ```bash
   cd crates/qmrust-wasm && wasm-pack build --target web --out-dir ../../docs/playground/pkg
   ```
   Then hard-reload — browsers cache the `.wasm`.
5. Upload `ds-<suffix>.zip` to the Zenodo deposition behind
   `docs/playground/data/sources.json` (the fetch path resolves
   `${base}/${archive}${suffix}`). Adding a file mints a **new version id**, so
   afterwards update `record`/`doi`/`base` to that id — the concept id 404s on
   the `/api/` files route. Then confirm every model's archive still resolves
   200 with `Access-Control-Allow-Origin: *`, not just the new one. **This
   publishes data externally — confirm with the user before uploading.**
→ *Gate: the model appears in `docs/playground/data/index.json`, its payload has
the expected aux + probes, and the user has confirmed the archive upload.*

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
  and its output provenance does not duplicate the acquisition axis into `Parameters`;
- the model is reachable in the playground: a `scripts/make_bids_examples.sh` block
  builds its `ds-<suffix>` root, and a regenerated
  `docs/playground/data/{index.json,<model>.json}` (+ slices) carries its aux maps and
  CLI-agreeing probes. No per-model JS/HTML/CSS/wasm edit is ever part of this — if one
  seems necessary, the app has grown a special case that belongs in the data instead.

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

BIDS qMRI grouping suffixes and entities (the authority for the `bids()` /
`bids_volume()` / grouping decisions in Phases 2–4):
https://bids-specification.readthedocs.io/en/stable/appendices/qmri.html
