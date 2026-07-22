#!/usr/bin/env bash
# Self-test: scaffold a throwaway model, prove it compiles + its renamed
# round-trip test passes + wiring landed, then restore the tree.
set -euo pipefail
ROOT="$(git rev-parse --show-toplevel)"
HERE="$ROOT/.claude/skills/porting-qmrlab-models"
NAME="scaffold_probe"
SUFFIX="ScaffoldProbe"
CORE="$ROOT/crates/qmrust-core/src"
DST="$CORE/models/$NAME"
MODS="$CORE/models/mod.rs"
REG="$CORE/registry.rs"
GROUP="$ROOT/crates/rust-bids/src/default_grouping.yaml"

# Refuse to run if the probe model already exists — never clobber it. Checked
# before the trap is armed, so cleanup can't delete a pre-existing directory.
[ -e "$DST" ] && { echo "refusing to run: $DST already exists" >&2; exit 1; }

# Snapshot the files the scaffold mutates and restore them verbatim on exit.
# (git checkout would discard any unrelated uncommitted edits to these files.)
SNAP="$(mktemp -d)"
cp "$MODS" "$SNAP/mod.rs"
cp "$REG" "$SNAP/registry.rs"
cp "$GROUP" "$SNAP/default_grouping.yaml"
cleanup() {
  rm -rf "$DST"
  cp "$SNAP/mod.rs" "$MODS"
  cp "$SNAP/registry.rs" "$REG"
  cp "$SNAP/default_grouping.yaml" "$GROUP"
  rm -rf "$SNAP"
}
trap cleanup EXIT

"$HERE/scaffold_model.sh" "$NAME" "$SUFFIX"

# Structural assertions.
test -f "$CORE/models/$NAME/model.rs"
grep -q "pub mod $NAME;" "$CORE/models/mod.rs"
grep -q "name: \"$NAME\"" "$CORE/registry.rs"
grep -q "^$SUFFIX:" "$ROOT/crates/rust-bids/src/default_grouping.yaml"
grep -Rq "TODO(port)" "$CORE/models/$NAME/"
# No leftover IR identifiers.
! grep -Rq "\bIr[A-Z]" "$CORE/models/$NAME/" || { echo "leftover Ir* symbol"; exit 1; }

# It compiles and the renamed round-trip test passes.
( cd "$ROOT" && cargo test -p qmrust-core "models::${NAME}::" -- --nocapture )

echo "SCAFFOLD SELF-TEST PASSED"
