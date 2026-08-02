#!/usr/bin/env bash
# Where qMRLab's demo data comes from, and how each model's archive becomes a
# BIDS dataset. Sourced, never run.
#
# Two callers need exactly this and nothing else in common:
#
#   * ci/integration_osf.sh, which bidsifies each archive and compares the fit
#     to qMRLab's own FitResults;
#   * scripts/make_bids_examples.sh, which bidsifies the same archives into the
#     dataset roots the playground ships.
#
# Stated once because the alternative was stating it twice. An archive pinned to
# `?version=` in one script and a different version in the other is not a build
# failure: both stay green, and CI then validates against data the reader never
# sees. Every fact here is of that kind, so this file owns it and each caller
# supplies only its output root and what it does afterwards.
#
# Contract for callers: set DATA (scratch for archives) and BIN (the qmrust
# binary), then `fetch_all`, then `bidsify_<model> <out-root>` for each model
# you want. Locator variables (IR_MAT, VFA_B1, …) are exported by `fetch_all`
# for callers that need the source files directly.

# Download and unpack one archive, unless it is already unpacked.
fetch() {
  local name="$1" url="$2"
  if [ ! -d "$DATA/$name" ]; then
    echo "Downloading qMRLab OSF $name dataset..."
    curl -L --fail -o "$DATA/$name.zip" "$url"
    unzip -o -q "$DATA/$name.zip" -d "$DATA/$name"
  fi
}

# Locate a file within an unpacked archive, robust to its folder layout. A miss
# is fatal: every caller is about to use the path.
locate() {
  local name="$1" pattern="$2" found
  found="$(find "$DATA/$name" -name "$pattern" | head -1)"
  [ -n "$found" ] || { echo "$pattern not found in $name archive" >&2; exit 1; }
  printf '%s' "$found"
}

# Same, for a qMRLab reference map under the archive's FitResults/.
locate_ref() {
  local name="$1" pattern="$2" found
  found="$(find "$DATA/$name" -path '*FitResults*' -name "$pattern" | head -1)"
  [ -n "$found" ] || { echo "FitResults/$pattern not found in $name archive" >&2; exit 1; }
  printf '%s' "$found"
}

# The archives, pinned. These versions are the dataset's identity: the maps CI
# compares against qMRLab and the maps a reader fits in the browser must come
# from the same bytes.
fetch_all() {
  mkdir -p "$DATA"
  fetch ir       "https://osf.io/cmg9z/download?version=3"
  fetch qmt      "https://osf.io/pzqyn/download?version=2"
  fetch mono_t2  "https://osf.io/kujp3/download?version=3"
  fetch mtr      "https://osf.io/erm2s/download?version=2"
  fetch mtsat    "https://osf.io/c5wdb/download?version=4"
  fetch vfa_t1   "https://osf.io/7wcvh/download?version=3"
  fetch b1_dam   "https://osf.io/mw3sq/download?version=3"
  fetch b1_afi   "https://osf.io/csjgx/download?version=9"

  # Each archive's own layout, resolved once. Mask.mat may sit in a different
  # directory than the measurement it belongs to, so it is located separately
  # rather than assumed alongside.
  IR_MAT="$(locate ir 'IRData.mat')";            export IR_MAT
  IR_MASK="$(locate ir 'Mask.mat')";             export IR_MASK
  QMT_MAT="$(locate qmt 'MTdata.mat')";          export QMT_MAT
  QMT_DIR="$(dirname "$QMT_MAT")";               export QMT_DIR
  MONO_SE="$(locate mono_t2 'SEdata.nii.gz')";   export MONO_SE
  MONO_MASK="$(locate mono_t2 'Mask.nii.gz')";   export MONO_MASK
  MTR_DIR="$(dirname "$(locate mtr 'MTon.mat')")"; export MTR_DIR
  MTSAT_DIR="$(dirname "$(locate mtsat 'MTw.nii.gz')")"; export MTSAT_DIR
  VFA_DATA="$(locate vfa_t1 'VFAData.nii.gz')";  export VFA_DATA
  VFA_MASK="$(locate vfa_t1 'Mask.nii.gz')";     export VFA_MASK
  VFA_B1="$(locate vfa_t1 'B1map.nii.gz')";      export VFA_B1
  B1DAM_A="$(locate b1_dam 'SFalpha.nii.gz')";   export B1DAM_A
  B1DAM_2A="$(locate b1_dam 'SF2alpha.nii.gz')"; export B1DAM_2A
  B1AFI_TR1="$(locate b1_afi 'AFIData1.nii.gz')"; export B1AFI_TR1
  B1AFI_TR2="$(locate b1_afi 'AFIData2.nii.gz')"; export B1AFI_TR2
}

# qMRLab's own fitted maps, for the voxelwise comparisons. Exported separately
# because only the CI caller needs them.
locate_references() {
  MONO_REF_T2="$(locate_ref mono_t2 'T2.nii.gz')";    export MONO_REF_T2
  MTR_REF="$(locate_ref mtr 'MTR.nii.gz')";           export MTR_REF
  MTSAT_REF_SAT="$(locate_ref mtsat 'MTSAT.nii.gz')"; export MTSAT_REF_SAT
  MTSAT_REF_T1="$(locate_ref mtsat 'T1.nii.gz')";     export MTSAT_REF_T1
  MTSAT_REF_MTR="$(locate_ref mtsat 'MTR.nii.gz')";   export MTSAT_REF_MTR
  VFA_REF_T1="$(locate_ref vfa_t1 'T1.nii.gz')";      export VFA_REF_T1
  VFA_REF_M0="$(locate_ref vfa_t1 'M0.nii.gz')";      export VFA_REF_M0
  B1DAM_REF="$(locate_ref b1_dam 'B1map.nii.gz')";    export B1DAM_REF
  # b1_afi ships a filtered map too; the raw one is what this model produces
  # (the smoothing is qMRLab's FilterClass, not part of the fit).
  B1AFI_REF="$(locate_ref b1_afi 'B1map_raw.nii.gz')"; export B1AFI_REF
}

# One bidsify per model, each taking the root to write. The flags are the
# model's own conversion contract: which source shape it reads, and which
# auxiliary inputs it needs named. Every caller wants them identical.
#
# bidsify always reads the *non-BIDS* recipe: that is the one carrying the
# acquisition, which becomes the sidecars. The BIDS recipe is for fitting the
# result and deliberately omits it.

# Stacked .mat measurement (9 inversion times) + a separate Mask.mat.
bidsify_inversion_recovery() {
  "$BIN" bidsify --model inversion_recovery \
    --mat-data "$IR_MAT" --mask "$IR_MASK" \
    --config recipes/non-bids/irt1_config.yaml --subject 01 --out "$1"
}

# A 4D SEdata.nii.gz (30 echoes) + Mask.nii.gz.
bidsify_mono_t2() {
  "$BIN" bidsify --model mono_t2 --nii-data "$MONO_SE" --nii-mask "$MONO_MASK" \
    --config recipes/non-bids/mono_t2_config.yaml --subject 01 --out "$1"
}

# A named set (one <role>.mat per role: MTon/MToff) + Mask.mat, from --mat-dir.
bidsify_mt_ratio() {
  "$BIN" bidsify --model mt_ratio --mat-dir "$MTR_DIR" \
    --config recipes/non-bids/mt_ratio_config.yaml --subject 01 --out "$1"
}

# Per-role NIfTIs (MTw/PDw/T1w) from --nii-dir. This archive ships no mask and
# no B1 map, so the fit takes the uncorrected Helms path.
bidsify_mt_sat() {
  "$BIN" bidsify --model mt_sat --nii-dir "$MTSAT_DIR" \
    --config recipes/non-bids/mt_sat_config.yaml --subject 01 --out "$1"
}

# Stacked MTdata.mat (10 flip/offset combos) from --mat-dir, which also supplies
# this model's declared aux maps (R1map/B1map/B0map.mat) and Mask.mat by name.
bidsify_qmt_spgr() {
  "$BIN" bidsify --model qmt_spgr --mat-dir "$QMT_DIR" \
    --config recipes/non-bids/qmt_config_ramani.yaml --subject 01 --out "$1"
}

# A 4D series + mask + B1 map. A NIfTI source has no directory convention to
# discover aux from, so the transmit map is named explicitly.
bidsify_vfa_t1() {
  "$BIN" bidsify --model vfa_t1 --nii-data "$VFA_DATA" --nii-mask "$VFA_MASK" \
    --aux "B1map=$VFA_B1" \
    --config recipes/non-bids/vfa_t1_config.yaml --subject 01 --out "$1"
}

# Two separate 3D volumes, one per flip angle, so --nii-data is repeated in
# acquisition order. No mask in the archive.
bidsify_b1_dam() {
  "$BIN" bidsify --model b1_dam \
    --nii-data "$B1DAM_A" --nii-data "$B1DAM_2A" \
    --config recipes/non-bids/b1_dam_config.yaml --subject 01 --out "$1"
}

# Two separate 3D volumes, one per interleaved repetition time.
bidsify_b1_afi() {
  "$BIN" bidsify --model b1_afi \
    --nii-data "$B1AFI_TR1" --nii-data "$B1AFI_TR2" \
    --config recipes/non-bids/b1_afi_config.yaml --subject 01 --out "$1"
}
