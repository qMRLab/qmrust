#!/usr/bin/env bash
# Vendor a minimal slice of bids2nf's test data for qmrust-bids' oracle tests.
# Copies the input dataset trees (as zero-byte .nii placeholders) and the golden
# *_unified.json for the qMRI collections qmrust fits. Run from repo root.
set -euo pipefail
DEST="crates/qmrust-bids/tests/fixtures"
BASE="https://raw.githubusercontent.com/agahkarakuzu/bids2nf/main/tests"
mkdir -p "$DEST"

for ds in qmri_irt1 qmri_mtsat; do
  # Golden output(s)
  mkdir -p "$DEST/expected/$ds"
  for f in $(curl -sfL "https://api.github.com/repos/agahkarakuzu/bids2nf/contents/tests/expected_outputs/$ds" \
    | grep '"path"' | sed 's/.*": "//;s/",.*//'); do
    curl -sfL "$BASE/../$f" -o "$DEST/expected/$ds/$(basename "$f")"
  done
done
echo "Vendored golden outputs into $DEST/expected."
echo "NOTE: input file *listings* are reconstructed inside oracle.rs from the golden"
echo "      JSON paths (nii/json), so no large binaries are committed."
