#!/usr/bin/env bash
# Download qMRLab's OSF demo datasets and run the qmrust fit pipelines against
# them end-to-end. Mirrors qMRLab's downloadData.m data sources.
set -euo pipefail

DATA="${1:-osf-data}"
BIN="${QMRUST_BIN:-./target/release/qmrust}"
mkdir -p "$DATA"

echo "Downloading qMRLab OSF datasets..."
curl -L --fail -o "$DATA/ir.zip"  "https://osf.io/cmg9z/download?version=3"
curl -L --fail -o "$DATA/qmt.zip" "https://osf.io/pzqyn/download?version=2"
unzip -o -q "$DATA/ir.zip"  -d "$DATA/ir"
unzip -o -q "$DATA/qmt.zip" -d "$DATA/qmt"

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

echo "Running IR fit..."
"$BIN" fit --mat-data "$IR_MAT" --mask "$IR_MASK" \
  --config prots/irt1_config.yaml --output-dir "$DATA/out_ir"

echo "Running qMT Ramani fit..."
"$BIN" fit --mat-dir "$QMT_DIR" \
  --config prots/qmt_config_ramani.yaml --output-dir "$DATA/out_ramani"

echo "Running qMT SledPikeRP fit..."
"$BIN" fit --mat-dir "$QMT_DIR" \
  --config prots/qmt_config_sledpikerp.yaml --output-dir "$DATA/out_srp"

echo "Asserting outputs..."
for f in "$DATA/out_ir/T1.nii.gz" "$DATA/out_ramani/F.nii.gz" "$DATA/out_srp/F.nii.gz"; do
  test -s "$f" || { echo "MISSING or empty: $f"; exit 1; }
done
echo "OSF integration OK"
