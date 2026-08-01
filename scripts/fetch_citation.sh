#!/usr/bin/env bash
# Render the qMRLab paper's citation in every style the playground offers, and
# write them to the payload the app reads.
#
# The formatting is done here, not in the browser: shipping the rendered strings
# means the citation costs no request and works offline, and a published DOI's
# metadata does not change often enough to be worth either a runtime call to
# doi.org or a CSL formatter in the bundle.
#
# Formatting comes from DOI content negotiation (doi.org, backed by Crossref's
# citeproc-js service), so the strings are the registrar's own rendering rather
# than something reformatted here.
#
# Usage: scripts/fetch_citation.sh [doi] [out]
set -euo pipefail

DOI="${1:-10.21105/joss.02343}"
OUT="${2:-docs/playground/data/citation.json}"

# CSL style file names, not display names — `vancouver` is not one of them,
# which is why the list names `bmj` instead.
STYLES=(apa modern-language-association chicago-author-date harvard-cite-them-right ieee nature bmj)
LABELS=(APA MLA Chicago Harvard IEEE Nature BMJ)

fetch() {
  curl -sL --fail -H "Accept: $1" "https://doi.org/$DOI"
}

echo "Fetching citations for $DOI ..." >&2
{
  printf '{\n  "doi": %s,\n  "url": %s,\n' \
    "$(printf '%s' "$DOI" | python3 -c 'import json,sys;print(json.dumps(sys.stdin.read()))')" \
    "$(printf 'https://doi.org/%s' "$DOI" | python3 -c 'import json,sys;print(json.dumps(sys.stdin.read()))')"
  printf '  "styles": [\n'
  # The separator counts what was emitted, not what was attempted: a dropped
  # style at any position would otherwise leave a comma with nothing before it.
  emitted=0
  for i in "${!STYLES[@]}"; do
    text="$(fetch "text/x-bibliography; style=${STYLES[$i]}" | python3 scripts/strip_empty_editor.py)"
    case "$text" in
      *style-not-found*) echo "  ${STYLES[$i]}: style not found, dropping" >&2; continue ;;
    esac
    [ "$emitted" -gt 0 ] && printf ',\n'
    emitted=$((emitted + 1))
    printf '    {"id": %s, "label": %s, "text": %s}' \
      "$(printf '%s' "${STYLES[$i]}" | python3 -c 'import json,sys;print(json.dumps(sys.stdin.read()))')" \
      "$(printf '%s' "${LABELS[$i]}" | python3 -c 'import json,sys;print(json.dumps(sys.stdin.read()))')" \
      "$(printf '%s' "$text" | python3 -c 'import json,sys;print(json.dumps(sys.stdin.read().strip()))')"
  done
  printf '\n  ],\n'
  # doi.org returns BibTeX as one long line; `prettify_bibtex.py` breaks it onto
  # a field per line so it reads and pastes like BibTeX.
  printf '  "bibtex": %s,\n' \
    "$(fetch 'application/x-bibtex' | python3 scripts/prettify_bibtex.py | python3 -c 'import json,sys;print(json.dumps(sys.stdin.read()))')"
  printf '  "ris": %s\n' \
    "$(fetch 'application/x-research-info-systems' | python3 -c 'import json,sys;print(json.dumps(sys.stdin.read().strip()))')"
  printf '}\n'
} > "$OUT"

python3 -c "import json;d=json.load(open('$OUT'));print('wrote', '$OUT', '—', len(d['styles']), 'styles + bibtex + ris')"
