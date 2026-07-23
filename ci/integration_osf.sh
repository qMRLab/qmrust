#!/usr/bin/env bash
# Download qMRLab's OSF demo datasets and run the qmrust fit pipelines against
# them end-to-end. Mirrors qMRLab's downloadData.m data sources.
set -euo pipefail

DATA="${1:-osf-data}"
BIN="${QMRUST_BIN:-./target/release/qmrust}"
mkdir -p "$DATA"

echo "Downloading qMRLab OSF datasets..."
curl -L --fail -o "$DATA/ir.zip"       "https://osf.io/cmg9z/download?version=3"
curl -L --fail -o "$DATA/qmt.zip"      "https://osf.io/pzqyn/download?version=2"
curl -L --fail -o "$DATA/mono_t2.zip"  "https://osf.io/kujp3/download?version=3"
curl -L --fail -o "$DATA/mtr.zip"      "https://osf.io/erm2s/download?version=2"
unzip -o -q "$DATA/ir.zip"      -d "$DATA/ir"
unzip -o -q "$DATA/qmt.zip"     -d "$DATA/qmt"
unzip -o -q "$DATA/mono_t2.zip" -d "$DATA/mono_t2"
unzip -o -q "$DATA/mtr.zip"     -d "$DATA/mtr"

# Locate the datasets by their key files (robust to archive folder layout).
IR_MAT="$(find "$DATA/ir" -name 'IRData.mat' | head -1)"
QMT_MAT="$(find "$DATA/qmt" -name 'MTdata.mat' | head -1)"
[ -n "$IR_MAT" ]  || { echo "IRData.mat not found in IR archive"; exit 1; }
[ -n "$QMT_MAT" ] || { echo "MTdata.mat not found in qMT archive"; exit 1; }
IR_DIR="$(dirname "$IR_MAT")"
QMT_DIR="$(dirname "$QMT_MAT")"

# Mask.mat may live in a different directory than IRData.mat within the
# archive; locate it independently rather than assuming it's alongside.
IR_MASK="$(find "$DATA/ir" -name 'Mask.mat' | head -1)"
[ -n "$IR_MASK" ] || { echo "Mask.mat not found in IR archive"; exit 1; }

# mono_t2 ships as NIfTI (SEdata.nii.gz) with a qMRLab reference (FitResults/).
MONO_SE="$(find "$DATA/mono_t2" -name 'SEdata.nii.gz' | head -1)"
MONO_MASK="$(find "$DATA/mono_t2" -name 'Mask.nii.gz' | head -1)"
MONO_REF_T2="$(find "$DATA/mono_t2" -path '*FitResults*' -name 'T2.nii.gz' | head -1)"
[ -n "$MONO_SE" ]     || { echo "SEdata.nii.gz not found in mono_t2 archive"; exit 1; }
[ -n "$MONO_MASK" ]   || { echo "Mask.nii.gz not found in mono_t2 archive"; exit 1; }
[ -n "$MONO_REF_T2" ] || { echo "FitResults/T2.nii.gz not found in mono_t2 archive"; exit 1; }

# mt_ratio ships as a named set of separate .mat files (MTon.mat/MToff.mat,
# + Mask.mat) with a qMRLab reference (FitResults/MTR.nii.gz).
MTR_ON="$(find "$DATA/mtr" -name 'MTon.mat' | head -1)"
MTR_REF="$(find "$DATA/mtr" -path '*FitResults*' -name 'MTR.nii.gz' | head -1)"
[ -n "$MTR_ON" ]  || { echo "MTon.mat not found in MTR archive"; exit 1; }
[ -n "$MTR_REF" ] || { echo "FitResults/MTR.nii.gz not found in MTR archive"; exit 1; }
MTR_DIR="$(dirname "$MTR_ON")"

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
"$BIN" bidsify --model mono_t2 --nii-data "$MONO_SE" --nii-mask "$MONO_MASK" \
  --config recipes/non-bids/mono_t2_config.yaml --subject 01 --out "$DATA/mono_t2_bids"
"$BIN" fit --bids-dir "$DATA/mono_t2_bids" \
  --config recipes/bids/mono_t2_config.yaml --output-dir "$DATA/out_mono_t2"

echo "Running mt_ratio bidsify + BIDS-path fit..."
# bidsify reads the named set (one <role>.mat per role) from --mat-dir and its
# Mask.mat; MTR has no acquisition arrays, so the recipes carry only the model
# name (+ the BIDS recipe's mask block). The BIDS-path fit reassembles the
# mt-on/mt-off named collection.
"$BIN" bidsify --model mt_ratio --mat-dir "$MTR_DIR" \
  --config recipes/non-bids/mt_ratio_config.yaml --subject 01 --out "$DATA/mtr_bids"
"$BIN" fit --bids-dir "$DATA/mtr_bids" \
  --config recipes/bids/mt_ratio_config.yaml --output-dir "$DATA/out_mtr"

echo "Asserting outputs..."
for f in "$DATA/out_ir/T1.nii.gz" "$DATA/out_ramani/F.nii.gz" "$DATA/out_srp/F.nii.gz" \
         "$DATA/out_mono_t2/qmrust/sub-01/anat/sub-01_T2map.nii.gz" \
         "$DATA/out_mtr/qmrust/sub-01/anat/sub-01_MTRmap.nii.gz"; do
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

echo "OSF integration OK"
