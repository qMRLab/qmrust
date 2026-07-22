# Validating a port

Validation is tiered. Every port clears the first tier; later tiers apply only when the
model's example data (and a qMRLab reference result) actually exist.

## Tier 1 — Round-trip (always required)

The model's own `forward → fit` must recover known truth: pick parameter values, run
`forward` to synthesize a measurement, run `fit` on that measurement, and assert the
recovered parameters match the values you started with (within a fit tolerance). This
test lives in the model's own `#[cfg(test)]` module, next to the model it exercises —
see the inversion-recovery round-trip in
`crates/qmrust-core/src/models/inversion_recovery/model.rs`
(`build_and_roundtrip_via_trait`). It needs no external data and runs on every
`cargo test`.

A round-trip test alone does not prove agreement with qMRLab — it only proves your
`forward` and `fit` are inverses of each other. Tiers 2 and 3 below are what connect the
port back to qMRLab.

## Tier 2 — Bidsify + fit (when the example data is fetchable)

Every qMRLab model documents how to fetch its own example data (an `onlineData` URL or a
demo `*_batch.m` / `qMRgenBatch` download — recorded in Phase 0). When that data is
fetchable:

1. Fetch it and run `qmrust bidsify` to convert the source `.mat`/raw data into a BIDS
   layout.
2. Confirm the produced layout is correct against the model's own declarations:
   - the suffix matches `bids()`;
   - per-volume entities and sidecars match `bids_volume()`;
   - voxel data in the BIDS NIfTI files is byte-identical to the source.
3. Fit the bidsified dataset with `qmrust fit --bids-dir` and confirm it runs to
   completion and produces the expected output maps.

This tier is a structural check — it proves the BIDS conversion and the BIDS-driven fit
path work for this model — not yet a numerical comparison against qMRLab.

## Tier 3 — Match qMRLab (when a reference result exists)

When qMRLab publishes a reference result for the fetched dataset (e.g. a `FitResults`
struct alongside the OSF data), the fitted maps must be compared against it **directly**,
voxelwise, and match within a stated tolerance. There is no universal tolerance: choose a
per-map absolute or relative bound from the map's units and qMRLab's own numerical
precision, and record it in the port's notes. The comparison must be unit-aware — qmrust
is BIDS-native throughout, so a qMRLab quantity in non-BIDS units (e.g. milliseconds vs.
seconds) needs an explicit conversion before comparison; never expect raw numerical
equality across differing units.

This direct `FitResults` comparison is the tier's actual requirement, and no harness
performs it for a new model yet — you must add it. Two existing harnesses are
**prerequisites**, not the comparison itself:

- **`ci/integration_osf.sh`** downloads qMRLab's OSF datasets and runs the fit pipelines
  end-to-end (mirrors qMRLab's `downloadData.m` sources), asserting only that each output
  map is produced and non-empty. It is a smoke check that the fetch → fit pipeline stays
  green against real qMRLab data — it does **not** currently compare any map to
  `FitResults`.
- **The `#[ignore]`d tests in `crates/qmrust-cli/src/commands.rs`**
  (`bids_fit_matches_mat_fit`, `qmtspgr_bids_fit_matches_mat_fit`) assert that fitting a
  dataset through the BIDS path produces maps exactly equal (voxelwise, values and NaN
  footprint) to fitting it through the `.mat` path. This is BIDS-vs-`.mat` **path parity**
  — the precondition for a qMRLab-agreement claim made via one path to hold via the other
  — not agreement with qMRLab. They need a local dataset and no network, so they run under
  `--ignored`:
  ```bash
  QMRUST_IR_MAT=<path>/IRData.mat QMRUST_IR_MASK=<path>/Mask.mat \
    cargo test -p qmrust-cli --release bids_fit_matches_mat_fit -- --ignored --nocapture
  ```

A new model's Tier 3 therefore means: fetch qMRLab's `FitResults` for the model's dataset,
extend `ci/integration_osf.sh` (or an equivalent test) to compare the produced maps to
those `FitResults` voxelwise within the stated tolerance — going beyond its current
non-empty assertion — and add a BIDS-vs-`.mat` parity test alongside the IR and qMT-SPGR
ones in `crates/qmrust-cli/src/commands.rs`.

(BIDS-grouping/resolver correctness — as opposed to model fitting — has its own golden
oracle at `crates/rust-bids/tests/oracle.rs`; it is unrelated to numerical fit agreement
and only relevant if the port also changes BIDS-grouping behavior.)

## Documented gap (no reference result)

If qMRLab does not publish a reference result for the model's example data, Tier 3 does
not apply. The round-trip, build, and (if data is fetchable) bidsify gates are the
validation bar. Say so explicitly in the port's notes: numerical agreement with qMRLab
is unverified for this model. Never imply agreement that hasn't been checked.

## The five completion gates

Every port must pass all five before it is considered done, regardless of which
validation tiers above apply:

```bash
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build -p qmrust-core --target wasm32-unknown-unknown
cargo build -p rust-bids --target wasm32-unknown-unknown
```
