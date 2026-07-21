# Data pipeline: dataset → protocol → fit

From dataset to fit — BIDS layout, sidecar metadata, and the model input
contract.

This is the deep-dive on how qmrust turns a directory of images (and their
JSON metadata) into the two things a `Model` actually consumes: an
identity-keyed `Measurement` and a resolved `Protocol`. See
[`ARCHITECTURE.md`](ARCHITECTURE.md) for the system map; this doc covers one
subsystem in full.

---

## The big picture

A dataset is data files plus metadata files. Getting from there to a fit
means turning that pile into an **ordered, identity-keyed input** that a
model can consume — without the model ever knowing whether the data came
from a BIDS directory, a `.mat` file, or (soon) a browser file picker.

The contract that makes this possible lives entirely in the `Model` trait
(`qmrust_core::core::model`):

- a model **declares** what shape of measurement it reads (`measurement()`)
  and where its acquisition parameters come from (`protocol_schema()`);
  auxiliary maps it needs are declared too (`required_inputs()`).
- the shell (CLI today, browser/Tauri later) **fulfills** those declarations:
  it locates files, reads metadata, resolves values, and hands the model an
  identity-keyed `Measurement` plus a scalar `Aux` bundle.

Everything below — `rust-bids`'s layout resolver, its sidecar reader, and
`resolve_protocol` — exists to fulfill that contract for BIDS datasets. A
model never sees a file path or a JSON blob; it only ever sees ordered
`f64`s, a `Measurement`, and an `Aux`.

---

## 1. Layout resolution — `rust-bids`

`rust-bids` is a wasm-clean, standalone crate (kept separate from
`qmrust-core` because it is generalizable beyond this workspace) that turns a
raw file listing into `Collection`s — groups of volumes that belong to one
fit.

- **`fs::DatasetFs`** is the I/O seam: a trait with `list`/`read` that stands
  in for `std::fs`. The native CLI implements it with `StdFs` (a real
  filesystem walk); a browser/Tauri frontend can implement it over a JS
  directory listing with no change to the resolver. This is what keeps
  `rust-bids` wasm-clean.
- **`scan_dataset(fs, cfg)`** walks the registry (`qmrust_core::registry::all()`)
  and, for every registered model's BIDS suffix, calls `collections_for` —
  registry-driven, so a new model needs no change here. It's the
  multi-model "what can I fit in this dataset" entry point (e.g. for a future
  dataset browser).
- **`collections_for(fs, cfg, suffix)`** (used by `run_fit_bids` for a single,
  already-chosen model) parses the dataset into flat rows (`table::parse_to_table`)
  and groups them (`resolve::resolve_set`) per a declarative grouping grammar,
  `BidsConfig`: `Sequential` sets (ordered by an entity, e.g. IRT1's `inv-`
  index, or qmt_spgr's custom `QMTSPGR` suffix ordered `by: [mt, flip]`),
  `Named` sets (fixed named slots matched by entity constraints, e.g. MTS's
  PDw/MTw/T1w), and `Plain` sets (parse-only, not yet grouped). A registered
  model's suffix is discovered even if `.bidsignore`'d (`QMTSPGR` is a
  custom, non-official suffix, so its example dataset ships a `.bidsignore`
  entry for general-purpose BIDS validators — `rust-bids`'s own discovery
  ignores it). Grouping
  is permissive-but-loud: a missing required member drops the collection, a
  missing non-required one attaches a `Warning` to the `Collection` rather
  than panicking.
- **`Collection` / `GroupedData`** — the resolved output: subject/session/
  run/task identity plus `GroupedData::Sequential(Vec<VolumeRef>)` or
  `GroupedData::Named(BTreeMap<String, VolumeRef>)`, each `VolumeRef` pairing
  a `.nii` path with its (optional) co-located sidecar path. A collection also
  carries `entities` — its **full** grouping identity (every `loop_over`
  entity present, as bare values), which is what auxiliary-input resolution
  matches against so any entity a dataset groups by participates.

### The flat table is the universal substrate

`table::parse_to_table` is Layer 1 for **everything**, not just grouping. It
walks the entire dataset — raw tree and every `derivatives/<pipeline>/` alike
(the `derivatives` column records the pipeline, `None` for raw) — into
`BidsRow`s whose columns are the parsed entities plus the structural
`suffix`/`datatype`/`derivatives`/`extension`/`path`. This mirrors
libBIDS.sh's model: one table spanning the whole dataset, queried by column.

- **`table_filter(rows, &[(column, value)])`** (over the `row_column`
  accessor) is the one generic query: select rows matching an arbitrary set of
  column constraints. Both collection grouping (`resolve_named`) and
  auxiliary-input resolution are expressed through it — nothing model- or
  dataset-specific.
- **Input resolution** (`qmrust-cli`, `resolve_aux_and_mask`) is a pure
  function of *(table, the collection's full `entities` identity, the model's
  declared inputs)*: for each `required_inputs()` entry it filters the table by
  the collection identity + the declared BIDS suffix (+ any `BidsMap.entity`),
  loads the single match, errors on ambiguity, and skips an absent optional
  input. A B1/B0/R1 map or mask is thus found wherever it lives — raw or any
  derivatives pipeline — and matched on whatever entities identify the
  collection (subject/session/run/…), never a hard-coded pair.
- Because resolution takes a *table + identity*, discovery is an optional top
  layer: a caller that iterates (`scan_dataset`) and a caller that is handed
  one subject's files (e.g. a Nextflow channel) share the same resolution path.

---

## 2. Metadata capture — `Sidecar`

Layout resolution tells you which files belong together; it says nothing
about their acquisition metadata. That's `sidecar::sidecar_for`'s job: build
one image's **full**, inheritance-merged JSON view.

- BIDS inheritance is resolved as a directory-chain merge: dataset root →
  `sub-` → `[ses-]` → datatype directory. At each level, `.json` files whose
  parsed entities are a subset of the image's own entities and whose suffix
  matches are merged in, least-specific first; the image's own co-located
  sidecar is re-applied last so it always wins ties regardless of
  directory-listing order.
- **`Sidecar`** exposes typed accessors — `f64`/`str`/`array`/`contains`/`get`
  — over the merged JSON object. A missing sidecar is an empty `Sidecar`, not
  an error; a malformed one (present but unparsable, or not a JSON object)
  is.
- This is a deliberate simplification: a directory-chain merge, not full
  entity-powerset matching against every ancestor directory. It's sufficient
  for the shipped qMRI suffixes, which never need sideways matching (e.g. a
  `sub-02` file influencing `sub-01`'s metadata).

---

## 3. The model input contract (the heart)

A model declares what it needs; nothing downstream hardcodes per-model
knowledge. Three declarations, all in `qmrust_core::core::model`:

- **`measurement() -> MeasurementKind`** — the shape of data the model reads,
  and the identities it reads by:
  - `Named { roles }`: a fixed set of role-labeled volumes (e.g. MTS's
    `["PDw", "MTw", "T1w"]`).
  - `Series { rows }`: a variable-length series whose canonical per-volume
    identity rows (`BTreeMap<String, f64>`, e.g. one `{"InversionTime": ti}`
    per TI) the model itself owns, in its canonical order.

  Both are **order-free and identity-keyed**: the shell tags each data volume
  with a `VolumeId` (`Role` or `Params`), and the engine matches supplied
  volumes to the model's declared identities by *value*, not position — a
  fit is invariant to how the acquisition list was ordered. An identity with
  no match fails loudly (a panic for that voxel) instead of silently
  assembling the wrong signal.

- **`protocol_schema() -> Vec<ProtoParam>`** — a declarative mapping from
  BIDS metadata (or `--config`) onto one acquisition parameter, per
  `ProtoParam { name, source, scope }`:
  - `Source::Field(key)` — read straight off the sidecar.
  - `Source::Derived(fn(&dyn Meta) -> Result<f64>)` — computed from several
    sidecar fields; a plain `fn` pointer (not a closure) so evaluation is a
    pure, image-scoped computation and `Model` stays object-safe.
  - `Source::Option(key)` — the non-BIDS fallback, read from `--config`
    options instead of any sidecar.
  - `Scope::PerVolume` resolves the param once per volume; `Scope::Global`
    resolves it once for the whole collection (against the first volume's
    sidecar). `protocol_schema()` defaults to `vec![]` — additive, no
    behaviour change for a model that hasn't declared one.

- **`required_inputs() -> Vec<InputSpec>`** — auxiliary scalar inputs (B1/B0/
  R1, …), each naming what the compute layer reads via `aux.get(name)` and,
  optionally, a `BidsMap { suffix, entity }` locating it in a BIDS dataset.
  There is no hardcoded aux-input list anywhere else; the shell loads exactly
  what each model declares.

- **`Meta`** — the read-only view (`f64`/`str`/`array` by key) a
  `Source::Derived` fn reads through. It lives in core, not `rust-bids`, so a
  `Derived` schema can be written without core depending on `rust-bids` (the
  dependency arrow only ever points into core). `rust-bids`'s `Sidecar`
  implements it.

---

## 4. Resolution + drive

Putting the pieces together, per collection:

1. `rust_bids::protocol::resolve_protocol(fs, collection, schema, options)`
   builds each volume's `Sidecar` (`sidecar_for`) in collection order and
   evaluates the model's `protocol_schema()` against it — `PerVolume` params
   per volume, `Global` params once — producing a `qmrust_core::Protocol`
   (`{ volumes: Vec<BTreeMap<String, f64>>, global: BTreeMap<String, f64> }`).
   A param that can't be resolved is a hard error naming the param (and, for
   `PerVolume`, the offending volume) — a silently missing value would
   otherwise only surface later as a per-voxel fit failure.
2. `build_volume_ids(model.measurement(), protocol, n_volumes)` (in
   `qmrust-cli/src/commands.rs`) turns the resolved `Protocol` (or, for
   `Named`, the model's own role list) into a `Vec<VolumeId>` — one identity
   per data volume — which `engine::run` uses to assemble each voxel's
   identity-keyed `Measurement` before calling `model.fit`.
3. `validate_against_protocol(kind, proto)` fails loudly **at model-build
   time** — not per-voxel — if a non-empty resolved `Protocol` is
   inconsistent with the model's declared `measurement()` (wrong volume
   count, missing key, or an identity the model's canonical rows don't
   contain). An empty `Protocol` (the model reads its own config) is always
   consistent.

This is the same path regardless of source: a BIDS collection is just one
way to arrive at a `Protocol` and an ordered volume set; a `.mat` file
(`ProtocolSource::Mat` in `qmrust-core/src/protocol.rs`) is another.

---

## 5. The two feeders

- **CLI, `qmrust fit --bids-dir <dir>`** — today's feeder. Builds a native
  `StdFs`, groups the dataset via `rust_bids::collections_for` keyed on the
  chosen model's registry `bids_suffix`, and for each collection calls
  `load_collection` (reads the NIfTI volumes + calls `resolve_protocol`) then
  `fit_and_write` (`build_volume_ids` → `engine::run` → NIfTI output). v1
  scope: `Sequential` collections and models with no *required* aux input —
  a model that declares required aux is rejected up front with a clear
  error, not a silent skip; `Named` collections are logged and skipped for
  now (`commands.rs::run_fit_bids`/`load_collection`).
- **Browser/Tauri (future)** — same `DatasetFs` seam, a different
  implementation backed by JS directory listings instead of `std::fs`; no
  change to `rust-bids`'s resolution or protocol logic.
- **Non-BIDS `--data`/`--mat`** — doesn't go through `rust-bids` at all.
  Acquisition parameters come from the model's own `--config` YAML
  (`ProtocolSource::Yaml`, an empty `Protocol`) or a `.mat` override
  (`ProtocolSource::Mat`); a model's `Source::Option` schema entries are the
  hook for reading a non-BIDS parameter out of `--config` under the same
  `protocol_schema()` contract.

---

## Units

`qmrust-core` is BIDS-native (SI): sidecar/config timing fields (`InversionTime`,
`RepetitionTime`, …) and fitted time-constant maps are in seconds, offsets in Hz, field in
tesla, `FlipAngle` in degrees per BIDS-MRI. A non-BIDS source (`.mat` via
`ProtocolSource::Mat`) converts to these units at the shell boundary before reaching a
model; qMRLab-reference validation reconciles the resulting ×1000 (s vs. ms) factor rather
than expecting raw equality. See the "Units — BIDS-native (SI)" principle in
[`CLAUDE.md`](../../CLAUDE.md) for the full rule.

---

## Deferred

- Fitting `Named` collections, and mapping qMT/MP2RAGE-style protocols onto
  them (today only `Sequential` collections drive a real fit).
- Resolving BIDS-side auxiliary maps (B1/B0/R1) so aux-dependent models can
  fit straight from a BIDS dataset (`run_fit_bids` currently rejects any
  model with a required aux input, and doesn't yet resolve a BIDS mask
  either — it fits every nonzero voxel rather than a `derivatives/`-supplied
  mask region).
- A real multi-field `Source::Derived` model (e.g. MP2RAGE) — the mechanism
  is proven today only by IR's single-field `InversionTime` schema and a
  stub `Derived` test.

---

## 6. The output side — `bids_outputs()` and the derivatives layout

Resolution (above) is how a dataset becomes a fit; this section is how a fit
becomes a dataset again, in BIDS-derivatives form.

- **`Model::bids_outputs() -> Vec<(&'static str, &'static str, &'static str)>`**
  declares which of a model's `output_names()` are genuine quantitative maps
  worth exporting in qMRLab's BIDS-derivatives naming, what suffix each gets,
  and each map's physical unit as a BIDS/SI string (`""` for a unitless
  quantity). Diagnostics (`res`, `idx`, `resnorm`, …) are omitted — only real
  maps are listed. IR declares `[("T1", "T1map", "s")]` (its `a`/`b` fit
  coefficients aren't standalone qMRLab maps, so they're left out;
  `R1map`/`M0map` would need the model to produce them directly, which it
  doesn't yet). qMT declares `[("F","Fmap",""), ("kr","kRmap","1/s"),
  ("R1f","R1Fmap","1/s"), ("R1r","R1Rmap","1/s"), ("T2f","T2Fmap","s"),
  ("T2r","T2Rmap","s")]`. Default is `vec![]` — additive, no behaviour
  change for a model that hasn't declared one.
- **`write_derivatives`** (`qmrust-cli/src/commands.rs`), used by
  `run_fit_bids`, writes each declared `(output, suffix, units)` triple
  present in a fit's `FitResults` to
  `deriv_root/qmrust/<subject>[/<session>]/anat/<subject>[_<session>]_<suffix>.nii.gz`,
  plus a full provenance JSON sidecar (`crate::provenance::FitProvenance`,
  `qmrust-cli/src/provenance.rs`) carrying: software + build environment
  (version, commit, rustc, target, build profile, OS/arch), the exact input
  volumes fit from (`Sources`, as dataset-relative `bids::<path>` URIs), the
  model name, the full resolved config (`Parameters`), the resolved
  `Protocol` actually used (per-volume params grouped into arrays, plus any
  global scalars), that map's `Units` (from `bids_outputs()`), a UTC
  ISO-8601 `DateExecuted`, and `FitDurationSeconds`. It also ensures one
  `deriv_root/qmrust/dataset_description.json` (`DatasetType: derivative`),
  whose own `GeneratedBy` carries the same software/commit identity. It
  reuses the same NIfTI writer flat, non-BIDS output uses
  (`write_map_nifti` for `.mat`-sourced data, `write_3d_nifti` otherwise),
  so map values are identical between the flat and derivatives layouts —
  only the path, file naming, and sidecar content differ. Plain
  `qmrust fit --output-dir` (no `--bids-dir`) keeps its existing flat
  `output_dir/<map>.nii.gz` layout, with no sidecar, unchanged.

## 7. `qmrust bidsify` — `.mat` → byte-identical BIDS (input provenance)

The BIDS pipeline needs example datasets to fit; `bidsify`
(`qmrust-cli/src/bidsify.rs`) is how one is produced from qMRLab's own OSF
`.mat` test data, so that fitting the BIDS version reproduces the `.mat` fit
exactly.

- **Byte-identical is the guarantee**: each volume is sliced straight out of
  the `.mat` `Array4<f64>` and written as `f64`/datatype-64 NIfTI — no
  rescale, no dtype narrowing. `bidsify --model inversion_recovery` writes
  `sub-<subject>/anat/sub-<subject>_inv-<i>_IRT1.nii.gz` (1-based `<i>`,
  matching the `.mat`'s own TI order — never re-sorted) + a
  `{InversionTime}` JSON sidecar per volume, `dataset_description.json`,
  `participants.tsv`, and (if a mask is given) a
  `derivatives/qmrust/sub-<subject>/anat/sub-<subject>_desc-brain_mask.nii.gz`.
- `bidsify --model qmt_spgr` writes the custom `QMTSPGR` suffix instead:
  `sub-<subject>/anat/sub-<subject>_flip-<f>_mt-<m>_QMTSPGR.nii.gz`, where
  `flip-<f>`/`mt-<m>` are 1-based, first-seen-order indices over the
  protocol's unique Angle/Offset values (cosmetic — the fit reads identity
  from the sidecar, not the filename), plus an `{Angle, Offset,
  RepetitionTime, MTPulseDuration}` sidecar per volume and a root
  `.bidsignore` (`*QMTSPGR*`, deduplicated across repeat runs). Any aux maps
  present (`R1map`/`B1map`/`B0map`/`Mask`) are written byte-identical under
  `derivatives/qmrust/sub-<subject>/anat/` as `_R1map`/`_TB1map`/`_B0map`/
  `_desc-brain_mask`.
- **How it's validated**: a unit test round-trips an in-memory `Array4`
  through each model's volume writer and asserts every voxel reads back `==`
  the source (not approximate) — this is what proves no rescale/precision
  loss; a separate structure test pins qmt_spgr's flip/mt filename derivation
  and sidecar fields. End to end, `scripts/make_bids_examples.sh` fetches
  qMRLab's OSF IR and qMT datasets, runs `bidsify` for both models into the
  same example dataset (sub-01 IRT1, sub-02 QMTSPGR), and fits each via
  `qmrust fit --bids-dir`. Two `#[ignore]`d integration tests
  (`bids_fit_matches_mat_fit`, `qmtspgr_bids_fit_matches_mat_fit` in
  `commands.rs`) assert each BIDS-path fit is voxel-**equal** to fitting the
  same `.mat` directly (inside the `.mat`'s mask for IRT1 — outside it, the
  BIDS path currently fits extra nonzero voxels the mat path leaves `NaN`,
  per the aux/mask-resolution gap above; the qMT comparison is both no-aux)
  and, for IRT1, within the OSF integration job's existing tolerance of
  qMRLab's own `FitResults/T1.nii.gz`.
- `bidsify` supports exactly `inversion_recovery` and `qmt_spgr`
  (`bail!`s otherwise).
