#!/usr/bin/env bash
# Self-test: scaffold a throwaway model, prove it compiles + its renamed
# round-trip test passes + wiring landed, then restore the tree.
set -euo pipefail
ROOT="$(git rev-parse --show-toplevel)"
HERE="$ROOT/.claude/skills/porting-qmrlab-models"
NAME="scaffold_probe"
SUFFIX="ScaffoldProbe"
CORE="$ROOT/crates/qmrust-core/src"

cleanup() {
  rm -rf "$CORE/models/$NAME"
  git -C "$ROOT" checkout -- \
    crates/qmrust-core/src/models/mod.rs \
    crates/qmrust-core/src/registry.rs \
    crates/rust-bids/src/default_grouping.yaml 2>/dev/null || true
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
