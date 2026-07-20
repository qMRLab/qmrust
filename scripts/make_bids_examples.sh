#!/usr/bin/env bash
# Produce a local, byte-identical IRT1 BIDS example dataset from qMRLab's OSF
# IR demo data, then validate the BIDS fit path against both the .mat fit
# path and qMRLab's own reference FitResults. The dataset itself is NOT
# committed (repo policy keeps large data out of git) — this script is the
# reproducible artifact; re-run it to regenerate `ds-qmrust/`.
#
# Usage: scripts/make_bids_examples.sh [scratch_dir]
set -euo pipefail

DATA="${1:-osf-data}"
BIN="${QMRUST_BIN:-./target/release/qmrust}"
CONFIG="prots/irt1_config.yaml"
mkdir -p "$DATA"

echo "Downloading qMRLab OSF IR dataset..."
curl -L --fail -o "$DATA/ir.zip" "https://osf.io/cmg9z/download?version=3"
unzip -o -q "$DATA/ir.zip" -d "$DATA/ir"

# Locate the key files by name (robust to archive folder layout).
IR_MAT="$(find "$DATA/ir" -name 'IRData.mat' | head -1)"
IR_MASK="$(find "$DATA/ir" -name 'Mask.mat' | head -1)"
IR_REF="$(find "$DATA/ir" -path '*FitResults/T1.nii.gz' | head -1)"
[ -n "$IR_MAT" ] || { echo "IRData.mat not found in IR archive"; exit 1; }
[ -n "$IR_MASK" ] || { echo "Mask.mat not found in IR archive"; exit 1; }
[ -n "$IR_REF" ] || { echo "FitResults/T1.nii.gz not found in IR archive"; exit 1; }

BIDS_DIR="$DATA/ds-qmrust"
echo "Converting to a BIDS dataset at $BIDS_DIR..."
"$BIN" bidsify --model inversion_recovery \
  --mat-data "$IR_MAT" --mask "$IR_MASK" \
  --config "$CONFIG" --subject 01 --out "$BIDS_DIR"

echo "BIDS dataset tree:"
find "$BIDS_DIR" -type f | sort

echo "Fitting via the BIDS path..."
# output-dir is the derivatives *root*: run_fit_bids appends qmrust/<subject>/anat/
# itself, so the result lands at $BIDS_DIR/derivatives/qmrust/sub-01/anat/.
"$BIN" fit --bids-dir "$BIDS_DIR" --config "$CONFIG" --output-dir "$BIDS_DIR/derivatives"

echo "Fitting via the .mat path (for comparison)..."
"$BIN" fit --mat-data "$IR_MAT" --mask "$IR_MASK" --config "$CONFIG" --output-dir "$DATA/out_mat"

BIDS_T1="$BIDS_DIR/derivatives/qmrust/sub-01/anat/sub-01_T1map.nii.gz"
MAT_T1="$DATA/out_mat/T1.nii.gz"
test -s "$BIDS_T1" || { echo "MISSING or empty: $BIDS_T1"; exit 1; }
test -s "$MAT_T1" || { echo "MISSING or empty: $MAT_T1"; exit 1; }

echo "Produced:"
echo "  BIDS fit:      $BIDS_T1"
echo "  .mat fit:      $MAT_T1"
echo "  qMRLab ref:    $IR_REF"
echo
echo "To validate voxelwise agreement in-process, run the ignored integration test:"
echo "  QMRUST_IR_MAT=$IR_MAT QMRUST_IR_MASK=$IR_MASK \\"
echo "    cargo test -p qmrust-cli --release bids_fit_matches_mat_fit -- --ignored --nocapture"
echo
echo "Upload $BIDS_DIR to OSF to publish it as the qmrust BIDS example."
