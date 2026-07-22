#!/usr/bin/env bash
# Scaffold a new qmrust model by cloning the living reference model
# (inversion_recovery) into a new model directory, renaming its symbols, and
# wiring it into the registry and BIDS grouping grammar. The result compiles
# and passes tests as a renamed inversion_recovery; the porter then replaces
# the four TODO(port)-marked pieces with the target model's math.
set -euo pipefail

usage() { echo "usage: $0 <snake_name> <BidsSuffix>   e.g. $0 mono_t2 T2map" >&2; exit 2; }
[ $# -eq 2 ] || usage
NAME="$1"; SUFFIX="$2"
[[ "$NAME" =~ ^[a-z][a-z0-9_]*$ ]] || { echo "name must be snake_case: $NAME" >&2; exit 2; }
[[ "$SUFFIX" =~ ^[A-Za-z][A-Za-z0-9]*$ ]] || { echo "suffix must be alnum: $SUFFIX" >&2; exit 2; }

ROOT="$(git rev-parse --show-toplevel)"
CORE="$ROOT/crates/qmrust-core/src"
SRC="$CORE/models/inversion_recovery"
DST="$CORE/models/$NAME"
GROUP="$ROOT/crates/rust-bids/src/default_grouping.yaml"
REG="$CORE/registry.rs"
MODS="$CORE/models/mod.rs"

[ -d "$SRC" ] || { echo "reference model not found: $SRC" >&2; exit 1; }
[ -e "$DST" ] && { echo "target already exists: $DST" >&2; exit 1; }

CAMEL="$(echo "$NAME" | awk -F_ '{for(i=1;i<=NF;i++) printf "%s%s", toupper(substr($i,1,1)), substr($i,2)}')"

# 1. Clone the reference model.
cp -R "$SRC" "$DST"

# 2. Rename symbols in every cloned file (perl for portable \b).
for f in "$DST"/*.rs; do
  CAMEL="$CAMEL" NAME="$NAME" perl -pi -e '
    my $c=$ENV{CAMEL}; my $n=$ENV{NAME};
    s/\bIr([A-Z][A-Za-z0-9]*)/$c$1/g;
    s/\bir_([a-z])/${n}_$1/g;
    s/\binversion_recovery\b/$n/g;
  ' "$f"
done

# 3. Register the module.
grep -q "pub mod ${NAME};" "$MODS" || echo "pub mod ${NAME};" >> "$MODS"

# 4. Insert the registry entry after the array opener `&[`.
NAME="$NAME" SUFFIX="$SUFFIX" perl -0777 -pi -e '
  my $n=$ENV{NAME}; my $s=$ENV{SUFFIX};
  my $e = "        ModelEntry {\n            name: \"$n\",\n            bids_suffix: \"$s\",\n            build: models::${n}::build,\n            describe: models::${n}::describe,\n            dump: models::${n}::dump,\n        },\n";
  s/(\&\[\n)/$1$e/;
' "$REG"

# 5. Append a BIDS grouping block (defaults to IR-style; porter adjusts).
grep -q "^${SUFFIX}:" "$GROUP" || cat >> "$GROUP" <<EOF
${SUFFIX}:
  sequential_set:
    by: [inv]   # TODO(port): set grouping entities for ${SUFFIX}
EOF

# 6. Prepend TODO(port) banners to the files holding the four gaps.
banner() {
  local file="$1" msg="$2" tmp
  tmp="$(mktemp)"
  { printf '// TODO(port): %s\n// qMRLab source: <ModelName>.m — replace the IR logic below.\n' "$msg"; cat "$file"; } > "$tmp"
  mv "$tmp" "$file"
}
banner "$DST/config.rs" "replace IR config fields with ${CAMEL}'s options + protocol"
banner "$DST/fit.rs"    "replace the IR signal equation and fitter with ${CAMEL}'s"
banner "$DST/model.rs"  "replace IR's protocol mapping / bids() / outputs with ${CAMEL}'s"

echo "scaffolded $NAME ($SUFFIX) at crates/qmrust-core/src/models/$NAME"
echo "next: cargo fmt, then fill the TODO(port) markers (see references/translation-patterns.md)"
