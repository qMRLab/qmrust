#!/usr/bin/env bash
# Build one self-contained BIDS example dataset per model from qMRLab's OSF demo
# data, and validate each through the BIDS fit path.
#
# Each dataset root is independently a complete, valid BIDS dataset with a
# single subject (`sub-01`): its own `dataset_description.json`, its raw
# acquisitions, and its own `derivatives/preprocessed` inputs (brain mask,
# field/parameter maps). That makes each one a single self-sufficient fetch unit
# — zip a root and it carries everything a fit of that model needs, and nothing
# belonging to another model.
#
# Roots are named `ds-<lowercased BIDS suffix>`, derived from the model's
# registry entry so no model→directory mapping is maintained anywhere:
#
#   ds-irt1     inversion_recovery   IRT1      ds-mts      mt_sat      MTS
#   ds-mese     mono_t2              MESE      ds-qmtspgr  qmt_spgr    QMTSPGR
#   ds-mtr      mt_ratio             MTR       ds-vfa      vfa_t1      VFA
#   ds-tb1dam   b1_dam               TB1DAM    ds-tb1afi   b1_afi      TB1AFI
#
# plus `ds-mts-b1`, the MTS + TB1map dataset the B1-correction path consumes.
# It is a synthetic phantom rather than OSF data (see its section below).
#
# The datasets are NOT committed (repo policy keeps large data out of git) —
# this script is the reproducible artifact. `derivatives/qmrust` (the reference
# fit outputs each block writes) is a dev/CI artifact for checking fits, not a
# fitting input, so `--zip` excludes it.
#
# Usage: scripts/make_bids_examples.sh [scratch_dir] [dataset_parent] [--zip]
#   scratch_dir     where OSF archives are downloaded/unpacked (default: osf-data)
#   dataset_parent  where the dataset roots are written (default: ~/Desktop/qmrust-osf)
#   --zip           also write <dataset_parent>/ds-<slug>.zip for each root
set -euo pipefail

ZIP=0
POSITIONAL=()
for arg in "$@"; do
  case "$arg" in
    --zip) ZIP=1 ;;
    *) POSITIONAL+=("$arg") ;;
  esac
done

DATA="${POSITIONAL[0]:-osf-data}"
OUT="${POSITIONAL[1]:-$HOME/Desktop/qmrust-osf}"
BIN="${QMRUST_BIN:-./target/release/qmrust}"
# The synthetic MTS phantom that backs ds-mts-b1; override to point elsewhere.
PHANTOM="${QMRUST_MTS_B1_PHANTOM:-$HOME/Desktop/ds-qmrust-test}"
PHANTOM_SUBJECT="${QMRUST_MTS_B1_SUBJECT:-sub-06}"

mkdir -p "$DATA" "$OUT"

# Where the data comes from and how each archive is bidsified: ci/datasets.sh,
# shared with ci/integration_osf.sh so the datasets a reader fits in the browser
# are byte-for-byte the ones CI validated against qMRLab.
# shellcheck source=ci/datasets.sh
. "$(dirname "$0")/../ci/datasets.sh"
fetch_all

# Assert every named map exists and is non-empty under a dataset's qmrust
# derivatives, i.e. that the BIDS-path fit actually produced it. Each map lands
# in the datatype directory its BIDS suffix implies (anat for tissue
# parameters, fmap for field maps), so search across them rather than assuming.
assert_maps() {
  local root="$1"; shift
  local subj="$root/derivatives/qmrust/sub-01"
  for suffix in "$@"; do
    local f
    f="$(find "$subj" -name "sub-01_${suffix}.nii.gz" -size +0 | head -1)"
    [ -n "$f" ] || { echo "MISSING or empty: $subj/*/sub-01_${suffix}.nii.gz" >&2; exit 1; }
  done
}

# ─── ds-irt1 — inversion_recovery (IRT1) ───────────────────────────────────
# Stacked .mat measurement (9 inversion times) + a separate Mask.mat.

IR_REF="$(find "$DATA/ir" -path '*FitResults/T1.nii.gz' | head -1)"

echo "Building $OUT/ds-irt1 ..."
bidsify_inversion_recovery "$OUT/ds-irt1"
# output-dir is the derivatives *root*: run_fit_bids appends qmrust/<subject>/anat/.
"$BIN" fit --bids-dir "$OUT/ds-irt1" \
  --config recipes/bids/irt1_config.yaml --output-dir "$OUT/ds-irt1/derivatives"
assert_maps "$OUT/ds-irt1" T1map

# ─── ds-mese — mono_t2 (MESE) ──────────────────────────────────────────────
# Ships as NIfTI: a 4D SEdata.nii.gz (30 echoes) + Mask.nii.gz. bidsify reads
# the non-BIDS recipe (its echo_times become the sidecars); the BIDS-path fit
# then reads those sidecars, so the BIDS recipe carries no acquisition axis.


echo "Building $OUT/ds-mese ..."
bidsify_mono_t2 "$OUT/ds-mese"
"$BIN" fit --bids-dir "$OUT/ds-mese" \
  --config recipes/bids/mono_t2_config.yaml --output-dir "$OUT/ds-mese/derivatives"
assert_maps "$OUT/ds-mese" T2map

# ─── ds-mtr — mt_ratio (MTR) ───────────────────────────────────────────────
# A named set (one <role>.mat per role: MTon/MToff) + Mask.mat, read from
# --mat-dir. MTR has no acquisition arrays, so the recipes carry only the model
# name (+ the BIDS recipe's mask block).

MTR_DIR="$(dirname "$(locate mtr 'MTon.mat')")"

echo "Building $OUT/ds-mtr ..."
bidsify_mt_ratio "$OUT/ds-mtr"
"$BIN" fit --bids-dir "$OUT/ds-mtr" \
  --config recipes/bids/mt_ratio_config.yaml --output-dir "$OUT/ds-mtr/derivatives"
assert_maps "$OUT/ds-mtr" MTRmap

# ─── ds-mts — mt_sat (MTS) ─────────────────────────────────────────────────
# Per-role NIfTIs (MTw/PDw/T1w) read from --nii-dir; the non-BIDS recipe
# supplies each role's FlipAngle/RepetitionTimeExcitation, written into the MTS
# sidecars. This archive ships no mask and no B1 map, so the dataset has no
# `derivatives/preprocessed` and the fit takes the uncorrected Helms path.

MTSAT_DIR="$(dirname "$(locate mtsat 'MTw.nii.gz')")"

echo "Building $OUT/ds-mts ..."
bidsify_mt_sat "$OUT/ds-mts"
"$BIN" fit --bids-dir "$OUT/ds-mts" \
  --config recipes/bids/mt_sat_config.yaml --output-dir "$OUT/ds-mts/derivatives"
assert_maps "$OUT/ds-mts" MTsat T1map MTRmap

# ─── ds-qmtspgr — qmt_spgr (QMTSPGR) ───────────────────────────────────────
# Stacked MTdata.mat (10 flip/offset combos) read from --mat-dir, which also
# supplies this model's declared aux maps (R1map.mat/B1map.mat/B0map.mat) and
# Mask.mat by filename convention.

QMT_DIR="$(dirname "$(locate qmt 'MTdata.mat')")"

echo "Building $OUT/ds-qmtspgr ..."
bidsify_qmt_spgr "$OUT/ds-qmtspgr"
"$BIN" fit --bids-dir "$OUT/ds-qmtspgr" \
  --config recipes/bids/qmt_config_ramani.yaml --output-dir "$OUT/ds-qmtspgr/derivatives"
assert_maps "$OUT/ds-qmtspgr" Fmap kRmap R1Fmap R1Rmap T2Fmap T2Rmap

# ─── ds-vfa — vfa_t1 (VFA) ─────────────────────────────────────────────────
# A 4D VFAData.nii.gz (2 flip angles) + Mask.nii.gz + B1map.nii.gz. A NIfTI
# source has no directory convention to discover aux from, so the transmit map
# is named explicitly with --aux; it lands in `derivatives/preprocessed` as the
# TB1map the model's optional B1map input resolves to.


echo "Building $OUT/ds-vfa ..."
bidsify_vfa_t1 "$OUT/ds-vfa"
"$BIN" fit --bids-dir "$OUT/ds-vfa" \
  --config recipes/bids/vfa_t1_config.yaml --output-dir "$OUT/ds-vfa/derivatives"
assert_maps "$OUT/ds-vfa" T1map M0map

# ─── ds-tb1dam — b1_dam (TB1DAM) ───────────────────────────────────────────
# Two separate 3D NIfTIs, one per flip angle (alpha and 2*alpha), so --nii-data
# is repeated once per volume in acquisition order. The archive ships no mask,
# so the dataset has no `derivatives/preprocessed` and the whole image is fit.


echo "Building $OUT/ds-tb1dam ..."
bidsify_b1_dam "$OUT/ds-tb1dam"
"$BIN" fit --bids-dir "$OUT/ds-tb1dam" \
  --config recipes/bids/b1_dam_config.yaml --output-dir "$OUT/ds-tb1dam/derivatives"
assert_maps "$OUT/ds-tb1dam" TB1map

# ─── ds-tb1afi — b1_afi (TB1AFI) ───────────────────────────────────────────
# Two separate 3D NIfTIs, one per interleaved repetition time, so --nii-data is
# repeated once per volume in acquisition order. The archive ships no mask, so
# the dataset has no `derivatives/preprocessed` and the whole image is fit.


echo "Building $OUT/ds-tb1afi ..."
bidsify_b1_afi "$OUT/ds-tb1afi"
"$BIN" fit --bids-dir "$OUT/ds-tb1afi" \
  --config recipes/bids/b1_afi_config.yaml --output-dir "$OUT/ds-tb1afi/derivatives"
assert_maps "$OUT/ds-tb1afi" TB1map

# ─── ds-mts-b1 — mt_sat with a B1 map, for the B1-correction path ──────────
# The only dataset here that is not OSF-derived: a synthetic concentric phantom
# (MTS roles + a TB1map spanning a range of transmit efficiencies) whose known
# ground truth is what makes the B1 correction checkable. Its generator lives
# outside this repo, so this block re-lays the existing tree rather than
# rebuilding it from source — point QMRUST_MTS_B1_PHANTOM at a dataset root
# holding the phantom under QMRUST_MTS_B1_SUBJECT.

PHANTOM_ANAT="$PHANTOM/$PHANTOM_SUBJECT/anat"
PHANTOM_B1="$PHANTOM/derivatives/preprocessed/$PHANTOM_SUBJECT/fmap/${PHANTOM_SUBJECT}_TB1map.nii.gz"

if [ -d "$PHANTOM_ANAT" ] && [ -s "$PHANTOM_B1" ]; then
  echo "Building $OUT/ds-mts-b1 from the $PHANTOM_SUBJECT phantom in $PHANTOM ..."
  # bidsify reads a named model's roles as <role>.nii.gz, so stage the
  # phantom's BIDS-named volumes under their role names first. The role→entity
  # mapping is the model's own (mt-on = MTw; flip-1/flip-2 mt-off = PDw/T1w).
  STAGE="$DATA/mts-b1-roles"
  rm -rf "$STAGE"; mkdir -p "$STAGE"
  cp "$PHANTOM_ANAT/${PHANTOM_SUBJECT}_flip-1_mt-on_MTS.nii.gz"  "$STAGE/MTw.nii.gz"
  cp "$PHANTOM_ANAT/${PHANTOM_SUBJECT}_flip-1_mt-off_MTS.nii.gz" "$STAGE/PDw.nii.gz"
  cp "$PHANTOM_ANAT/${PHANTOM_SUBJECT}_flip-2_mt-off_MTS.nii.gz" "$STAGE/T1w.nii.gz"

  "$BIN" bidsify --model mt_sat --nii-dir "$STAGE" \
    --aux "B1map=$PHANTOM_B1" \
    --config recipes/non-bids/mt_sat_config.yaml --subject 01 --out "$OUT/ds-mts-b1"

  # The uncorrected fit first: it proves the dataset resolves and fits through
  # the plain MTS path, independent of the correction.
  "$BIN" fit --bids-dir "$OUT/ds-mts-b1" \
    --config recipes/bids/mt_sat_config.yaml \
    --output-dir "$OUT/ds-mts-b1/derivatives"
  assert_maps "$OUT/ds-mts-b1" MTsat T1map MTRmap

  # Then the B1-corrected fit, which is what this dataset exists for. The
  # correction needs a sequence-simulation artifact self-calibrated on the
  # dataset itself; `recipes/bids/mt_sat_b1corr_config.yaml` names it by the
  # relative path `fitvalues.yaml`, resolved from the working directory, so it
  # is produced there and moved into `derivatives/preprocessed` — an input the
  # corrected fit consumes, not one of its output maps, so it belongs in the
  # zipped archive rather than the excluded `derivatives/qmrust` tree.
  "$BIN" mtsat-b1 --seq recipes/mtsat_b1_seq.yaml \
    --bids-dir "$OUT/ds-mts-b1" --out fitvalues.yaml
  "$BIN" fit --bids-dir "$OUT/ds-mts-b1" \
    --config recipes/bids/mt_sat_b1corr_config.yaml \
    --output-dir "$OUT/ds-mts-b1/derivatives"
  assert_maps "$OUT/ds-mts-b1" MTsat T1map MTRmap
  FITVALUES="$OUT/ds-mts-b1/derivatives/preprocessed/sub-01/sub-01_fitvalues.yaml"
  mkdir -p "$(dirname "$FITVALUES")"
  mv fitvalues.yaml "$FITVALUES"
  test -s "$FITVALUES" || { echo "MISSING or empty: $FITVALUES" >&2; exit 1; }
else
  echo "Skipping ds-mts-b1: no phantom at $PHANTOM_ANAT (+ $PHANTOM_B1)."
  echo "  Set QMRUST_MTS_B1_PHANTOM/QMRUST_MTS_B1_SUBJECT to build it."
fi

# ─── Zip each root as a single fetchable unit ──────────────────────────────
# `derivatives/qmrust` is excluded: it holds reference fit *outputs*, which are
# how a fit gets checked, not an input a fit needs.
#
# Filesystem cruft is excluded too. A published archive is read by the
# playground's own BIDS resolver, which reports every file it cannot account
# for, so a stray `.DS_Store` shows up as "not a recognized BIDS file" in front
# of the reader. These are also pruned from the tree first, so a re-zip of an
# untouched root cannot smuggle one back in.

if [ "$ZIP" = 1 ]; then
  find "$OUT" -name '.DS_Store' -delete
  for root in "$OUT"/ds-*; do
    [ -d "$root" ] || continue
    slug="$(basename "$root")"
    echo "Zipping $slug ..."
    (cd "$OUT" && rm -f "$slug.zip" \
      && zip -r -q -X "$slug.zip" "$slug" \
        -x "$slug/derivatives/qmrust/*" "*.DS_Store" "*__MACOSX*")
  done
  ls -lh "$OUT"/ds-*.zip
fi

echo
echo "Dataset roots under $OUT:"
for root in "$OUT"/ds-*; do
  [ -d "$root" ] && printf '  %-14s %s\n' "$(basename "$root")" "$(du -sh "$root" | cut -f1)"
done
echo
echo "Voxelwise agreement with qMRLab's own FitResults is checked by"
echo "ci/integration_osf.sh; the .mat-vs-BIDS round-trips by the #[ignore]d tests:"
echo "  QMRUST_IR_MAT=$IR_MAT QMRUST_IR_MASK=$IR_MASK \\"
echo "    cargo test -p qmrust-cli --release bids_fit_matches_mat_fit -- --ignored --nocapture"
echo "  QMRUST_QMT_MAT=$(locate qmt 'MTdata.mat') \\"
echo "    cargo test -p qmrust-cli --release qmtspgr_bids_fit_matches_mat_fit -- --ignored --nocapture"
[ -n "$IR_REF" ] && echo "  qMRLab IR reference: $IR_REF"
