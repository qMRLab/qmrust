#!/usr/bin/env bash
# Download qMRLab's OSF demo datasets and run the qmrust fit pipelines against
# them end-to-end. Mirrors qMRLab's downloadData.m data sources.
set -euo pipefail

DATA="${1:-osf-data}"
BIN="${QMRUST_BIN:-./target/release/qmrust}"
mkdir -p "$DATA"

# `realpath` is not on every runner image, and the paths here are built from
# `$DATA`, which may itself be absolute or relative.
abs() {
  case "$1" in
    /*) printf '%s' "$1" ;;
    *) printf '%s/%s' "$PWD" "$1" ;;
  esac
}

# Where the data comes from and how each archive is bidsified: ci/datasets.sh,
# shared with scripts/make_bids_examples.sh so an archive cannot be pinned to
# one version here and another there.
# shellcheck source=ci/datasets.sh
. "$(dirname "$0")/datasets.sh"
fetch_all
locate_references

echo "Running IR fit..."
"$BIN" fit --mat-data "$IR_MAT" --mask "$IR_MASK" \
  --config recipes/non-bids/irt1_config.yaml --output-dir "$DATA/out_ir"

echo "Running qMT Ramani fit..."
"$BIN" fit --mat-dir "$QMT_DIR" \
  --config recipes/non-bids/qmt_config_ramani.yaml --output-dir "$DATA/out_ramani"

echo "Running qMT SledPikeRP fit..."
"$BIN" fit --mat-dir "$QMT_DIR" \
  --config recipes/non-bids/qmt_config_sledpikerp.yaml --output-dir "$DATA/out_srp"

echo "Running mono_t2 bidsify + BIDS-path fit..."
# bidsify uses the non-BIDS recipe (its echo_times are the protocol fallback,
# written into the sidecars); the BIDS-path fit then reads those sidecars and
# uses the BIDS recipe (no echo_times — avoids duplicating the acquisition axis
# into the output provenance's Parameters block).
bidsify_mono_t2 "$DATA/mono_t2_bids"
"$BIN" fit --bids-dir "$DATA/mono_t2_bids" \
  --config recipes/bids/mono_t2_config.yaml --output-dir "$DATA/out_mono_t2"

echo "Running mt_ratio bidsify + BIDS-path fit..."
# bidsify reads the named set (one <role>.mat per role) from --mat-dir and its
# Mask.mat; MTR has no acquisition arrays, so the recipes carry only the model
# name (+ the BIDS recipe's mask block). The BIDS-path fit reassembles the
# mt-on/mt-off named collection.
bidsify_mt_ratio "$DATA/mtr_bids"
"$BIN" fit --bids-dir "$DATA/mtr_bids" \
  --config recipes/bids/mt_ratio_config.yaml --output-dir "$DATA/out_mtr"

echo "Running mt_sat bidsify + BIDS-path fit..."
# bidsify reads the per-role NIfTIs (MTw/PDw/T1w) from --nii-dir; the non-BIDS
# recipe supplies each role's FlipAngle/RepetitionTimeExcitation, written into
# the MTS sidecars. The BIDS-path fit folds those per-role sidecars back in.
bidsify_mt_sat "$DATA/mtsat_bids"
"$BIN" fit --bids-dir "$DATA/mtsat_bids" \
  --config recipes/bids/mt_sat_config.yaml --output-dir "$DATA/out_mtsat"

echo "Running vfa_t1 bidsify + BIDS-path fit..."
# bidsify reads the 4D series from --nii-data and the transmit map via --aux;
# the non-BIDS recipe supplies the flip angles and TR, written into the VFA
# sidecars. The BIDS-path fit folds those sidecars back in and resolves the
# TB1map derivative as the model's optional B1map input.
bidsify_vfa_t1 "$DATA/vfa_t1_bids"
"$BIN" fit --bids-dir "$DATA/vfa_t1_bids" \
  --config recipes/bids/vfa_t1_config.yaml --output-dir "$DATA/out_vfa_t1"

echo "Running b1_dam bidsify + BIDS-path fit..."
# The two flip-angle volumes ship as separate 3D files, so --nii-data is
# repeated once per volume in acquisition order; the non-BIDS recipe supplies
# the flip angles, written into the TB1DAM sidecars. The BIDS-path fit folds
# those sidecars back in.
bidsify_b1_dam "$DATA/b1_dam_bids"
"$BIN" fit --bids-dir "$DATA/b1_dam_bids" \
  --config recipes/bids/b1_dam_config.yaml --output-dir "$DATA/out_b1_dam"

echo "Running b1_afi bidsify + BIDS-path fit..."
# The two interleaved repetition times ship as separate 3D files, so
# --nii-data is repeated once per volume in acquisition order; the non-BIDS
# recipe supplies the repetition times and the nominal flip angle, written
# into the TB1AFI sidecars. The BIDS-path fit folds those sidecars back in and
# reassembles the acq-tr1/acq-tr2 collection.
bidsify_b1_afi "$DATA/b1_afi_bids"
"$BIN" fit --bids-dir "$DATA/b1_afi_bids" \
  --config recipes/bids/b1_afi_config.yaml --output-dir "$DATA/out_b1_afi"

echo "Asserting outputs..."
for f in "$DATA/out_ir/T1.nii.gz" "$DATA/out_ramani/F.nii.gz" "$DATA/out_srp/F.nii.gz" \
         "$DATA/out_mono_t2/qmrust/sub-01/anat/sub-01_T2map.nii.gz" \
         "$DATA/out_mtr/qmrust/sub-01/anat/sub-01_MTRmap.nii.gz" \
         "$DATA/out_mtsat/qmrust/sub-01/anat/sub-01_MTsat.nii.gz" \
         "$DATA/out_mtsat/qmrust/sub-01/anat/sub-01_T1map.nii.gz" \
         "$DATA/out_vfa_t1/qmrust/sub-01/anat/sub-01_T1map.nii.gz" \
         "$DATA/out_vfa_t1/qmrust/sub-01/anat/sub-01_M0map.nii.gz" \
         "$DATA/out_b1_dam/qmrust/sub-01/fmap/sub-01_TB1map.nii.gz" \
         "$DATA/out_b1_afi/qmrust/sub-01/fmap/sub-01_TB1map.nii.gz"; do
  test -s "$f" || { echo "MISSING or empty: $f"; exit 1; }
done

# Voxelwise agreement with qMRLab's own FitResults. qmrust reports T2 in
# seconds, qMRLab in ms (--scale 1000). A long-T2 tail (T2 approaching the
# 300 ms bound while the longest echo is 384 ms) is under-determined, so the
# LM and qMRLab's trust-region-reflective diverge there; the bulk agrees to
# machine precision. The threshold accepts that tail without masking a real
# regression in the well-determined majority.
echo "Comparing mono_t2 T2 map to qMRLab FitResults..."
python3 ci/compare_maps.py \
  "$DATA/out_mono_t2/qmrust/sub-01/anat/sub-01_T2map.nii.gz" "$MONO_REF_T2" \
  --scale 1000 --rel-tol 0.01 --min-frac 0.90 --min-corr 0.95 --label mono_t2-T2

# MTR is a closed-form ratio (no fit, no unit conversion — both in percent), so
# agreement with qMRLab is exact to float rounding across the whole mask.
echo "Comparing mt_ratio MTR map to qMRLab FitResults..."
python3 ci/compare_maps.py \
  "$DATA/out_mtr/qmrust/sub-01/anat/sub-01_MTRmap.nii.gz" "$MTR_REF" \
  --rel-tol 0.001 --min-frac 0.99 --min-corr 0.999 --label mt_ratio-MTR

# mt_sat is closed-form (no B1 map in this dataset -> the uncorrected Helms
# path), so all three maps agree with qMRLab to double-precision round-off.
# MTSAT's near-zero (background-adjacent) voxels make a *relative* tolerance
# meaningless there, so its min-frac has margin while the correlation stays 1.
echo "Comparing mt_sat maps to qMRLab FitResults..."
python3 ci/compare_maps.py \
  "$DATA/out_mtsat/qmrust/sub-01/anat/sub-01_MTsat.nii.gz" "$MTSAT_REF_SAT" \
  --rel-tol 0.01 --min-frac 0.98 --min-corr 0.999 --label mt_sat-MTSAT
python3 ci/compare_maps.py \
  "$DATA/out_mtsat/qmrust/sub-01/anat/sub-01_T1map.nii.gz" "$MTSAT_REF_T1" \
  --rel-tol 0.01 --min-frac 0.99 --min-corr 0.999 --label mt_sat-T1
python3 ci/compare_maps.py \
  "$DATA/out_mtsat/qmrust/sub-01/anat/sub-01_MTRmap.nii.gz" "$MTSAT_REF_MTR" \
  --rel-tol 0.001 --min-frac 0.99 --min-corr 0.999 --label mt_sat-MTR

# vfa_t1 is a closed-form linearized least-squares solve with the same B1 map
# qMRLab used, and both report T1 in seconds, so every masked voxel agrees to
# the float32 precision the maps are stored in.
echo "Comparing vfa_t1 maps to qMRLab FitResults..."
python3 ci/compare_maps.py \
  "$DATA/out_vfa_t1/qmrust/sub-01/anat/sub-01_T1map.nii.gz" "$VFA_REF_T1" \
  --rel-tol 0.001 --min-frac 0.999 --min-corr 0.999 --label vfa_t1-T1
python3 ci/compare_maps.py \
  "$DATA/out_vfa_t1/qmrust/sub-01/anat/sub-01_M0map.nii.gz" "$VFA_REF_M0" \
  --rel-tol 0.001 --min-frac 0.999 --min-corr 0.999 --label vfa_t1-M0

# b1_dam is a closed-form arc-cosine of a signal ratio, dimensionless in both
# implementations, so every voxel agrees to double-precision round-off — this
# dataset ships no mask, so that holds over the whole image including the
# out-of-domain voxels where the ratio exceeds 1 and both take the magnitude of
# the complex principal value.
echo "Comparing b1_dam B1 map to qMRLab FitResults..."
python3 ci/compare_maps.py \
  "$DATA/out_b1_dam/qmrust/sub-01/fmap/sub-01_TB1map.nii.gz" "$B1DAM_REF" \
  --rel-tol 0.001 --min-frac 0.999 --min-corr 0.999 --label b1_dam-B1

# b1_afi is likewise closed form and dimensionless in both implementations. Its
# estimator pins an unphysical signal ratio (r > 1) to B1 = 0 while leaving a
# non-finite ratio NaN, and reproducing that arithmetic exactly is what keeps
# the zero and NaN footprints equal to qMRLab's rather than merely the
# well-behaved voxels agreeing.
echo "Comparing b1_afi B1 map to qMRLab FitResults..."
python3 ci/compare_maps.py \
  "$DATA/out_b1_afi/qmrust/sub-01/fmap/sub-01_TB1map.nii.gz" "$B1AFI_REF" \
  --rel-tol 0.001 --min-frac 0.999 --min-corr 0.999 --label b1_afi-B1

# The BIDS path and the .mat path must produce identical maps from the same
# input. CLAUDE.md names this as the definition of a behaviour-preserving
# change, and the two tests that assert it are `#[ignore]`d because they need
# real `.mat` files rather than because they are optional. This script has just
# unpacked exactly those files, so it is the one place they can run
# automatically, and running them here is what makes the invariant enforced
# rather than merely written down.
#
# One invocation for both: a cargo test filter is a substring match, and
# `bids_fit_matches_mat_fit` is a substring of `qmtspgr_bids_fit_matches_mat_fit`,
# so a per-test command would run the qMT case with no `QMRUST_QMT_MAT` set.
#
# `--include-ignored` rather than `--ignored`: the latter runs ONLY ignored
# tests, so if these ever stop being ignored this line keeps running them
# instead of silently running nothing.
# Absolute paths, because `cargo test` runs a test binary with the PACKAGE
# directory as its working directory, not the workspace root. Every other
# command here invokes the CLI from the root, so these are the only paths in
# the script that cannot be relative.
echo "Checking the BIDS and .mat pipelines agree..."
QMRUST_IR_MAT="$(abs "$IR_MAT")" \
QMRUST_IR_MASK="$(abs "$IR_MASK")" \
QMRUST_QMT_MAT="$(abs "$QMT_MAT")" \
  cargo test -p qmrust-cli --release fit_matches_mat_fit \
  -- --include-ignored --nocapture --test-threads=1

echo "OSF integration OK"
