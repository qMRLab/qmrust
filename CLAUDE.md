This document contains repository-wide constraints and engineering principles. This is NOT an architecture document.

- For system design and extension points see `docs/agents/ARCHITECTURE.md`
- For BIDS layout, metadata resolution, model input pipeline see `docs/agents/DATA-PIPELINE.md`

---

# About this codebase

qmrust is a native Rust quantitative MRI fitting workspace built on a **functional core /
imperative shell** architecture. The numerical core is deterministic, side-effect free, and
compiles unchanged for native and WebAssembly.

---

# The one rule that must never break

**`qmrust-core` stays pure.** No filesystem, CLI, browser, JS-binding, or BIDS-traversal
dependencies. If functionality needs I/O or the outside world, it lives outside the core.

Verify:

```bash
cargo build -p qmrust-core --target wasm32-unknown-unknown
```

---

# Repository invariants

Part of the architecture. Do not violate.

- **Core purity** — enforced by the wasm build above.
- **`Model` stays object-safe** — enforced at compile time.
- **Behaviour-preserving refactors do not change fitting results.** Verify by fitting a
  fixed dataset before and after the change and diffing the output maps voxelwise — they
  must be identical (values and NaN footprint). The real pipelines are exercised by
  `ci/integration_osf.sh` (CI, against qMRLab's OSF datasets) and the `#[ignore]`d
  round-trip tests `bids_fit_matches_mat_fit` / `qmtspgr_bids_fit_matches_mat_fit`, which
  assert the BIDS-path maps equal the `.mat`-path maps exactly:
  ```bash
  QMRUST_IR_MAT=<path>/IRData.mat QMRUST_IR_MASK=<path>/Mask.mat \
    cargo test -p qmrust-cli --release bids_fit_matches_mat_fit -- --ignored --nocapture
  ```
  Any diff in fitting output is a regression regardless of intent.
- **Each model owns its own configuration.**
- **Threaded WebAssembly is an optional feature** — must not affect default native or default
  wasm builds.

---

# Working principles

**One authoritative representation.** Every piece of knowledge — a rule, a constant, a
taxonomy, a contract — has a single, unambiguous, authoritative home, and everything else
derives from it. A fact stated twice is a fact that will disagree with itself. The two
copies do not fail loudly when they drift; the stale one just keeps being read.

Worked examples in this repo, each the single home of one fact:

- `registry::Category` — a model's family, subgroup, reading order, icon and docs
  directory. The gallery and the playground picker both derive the whole tree from it;
  neither restates a model's membership or the order families appear in.
- `rust_bids::datatype_for_suffix` — whether a BIDS suffix lives in `anat/` or `fmap/`.
  One rule, three writers (raw acquisitions, preprocessed aux, derivative outputs).
- `core::model::SeriesAxis` — how a single-axis series is identified: its rows, its
  tagged samples, the signal `fit` assembles back, the axis `ingest_protocol` reads.
- `tests/properties.rs` — the contract every model owes. A shared property covers models
  that do not exist yet; a per-model copy covers only the models someone remembered.

**But duplication is cheaper than the wrong abstraction.** A premature abstraction has to
be *un*-abstracted before the real shape can be seen, and that is more expensive than the
duplication it replaced. Prefer waiting.

**Rule of Three** is the heuristic: generalize on the third occurrence, when the shape has
had two chances to disagree with itself. It is a heuristic, not a law:

- Abstract immediately when the duplication is trivially obvious and cannot be wrong — a
  constant, a well-understood formula, a spec fact.
- Wait longer than three when the logic is complex or the domain is still uncertain. Two
  models sharing an equation may be one insight away from needing different ones.

The test is whether the candidates are the *same knowledge*, not whether they are the same
characters. Code that looks alike for unrelated reasons is coincidence, and merging it
couples things that should move independently.

**Extend, don't special-case.** Use existing abstractions and extension points (see
ARCHITECTURE.md) rather than model-specific branches, ad hoc dispatch, or duplicated logic. If
an abstraction can't support a feature, improve the abstraction.

A guard that cannot apply to every case is a signal: derive it from what the subject
*declares*, never from its name. `qmt_spgr` is excluded from the single-axis properties
because its `measurement()` has two axes, not because a test names it.

**Delete, don't accumulate.** Never leave dead code, commented-out code, speculative
scaffolding, obsolete compat layers, duplicated sources of truth, or stale terminology. When a
concept is renamed or superseded, update every reference and remove the old one.

**Comments and docs are timeless.** Explain the current contract: invariants, assumptions,
safety requirements, domain knowledge, non-obvious reasoning. Never explain history, recent
changes, rejected alternatives, review context, or task references. If a comment only makes
sense to someone who watched it being written, delete it. Same rule for `///` and `//!`.

**Docs describe the current system.** Keep architecture docs synchronized with code.
Progressive disclosure: essentials first, details second.

---

# Units

BIDS-native units throughout. The core performs no unit conversion; convert non-BIDS formats
only at the shell boundary before data enters the core. Validation against external
implementations must account for unit differences, not expect raw numerical equality.

---

# Before considering work complete

```bash
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

If changes touch purity boundaries, also:

```bash
cargo build -p qmrust-core --target wasm32-unknown-unknown
cargo build -p rust-bids --target wasm32-unknown-unknown
```