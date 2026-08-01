#!/usr/bin/env python3
"""Drop an empty editor clause from a rendered citation, on stdin to stdout.

Crossref carries no editor for this record, and citeproc still emits the
clause: MLA renders "edited by , vol. 5" and Harvard "Edited by , 5(53)". The
text is upstream's, not ours, but it is shown to a reader, so it is cleaned
here rather than passed through.

Only the empty form is touched. A citation that genuinely names an editor keeps
it, which is why the patterns require the comma to follow the clause directly.
"""
import re
import sys

# "edited by , " and "Edited by , " with nothing between the clause and comma.
EMPTY_EDITOR = re.compile(r"(?i)\bedited by\s*,\s*")


def strip(text: str) -> str:
    return EMPTY_EDITOR.sub("", text)


if __name__ == "__main__":
    sys.stdout.write(strip(sys.stdin.read()))
