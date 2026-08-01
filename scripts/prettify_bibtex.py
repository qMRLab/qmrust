#!/usr/bin/env python3
"""Break a one-line BibTeX entry onto a field per line, on stdin → stdout.

`doi.org` returns BibTeX as a single long line. That is valid, but it neither
reads nor pastes like BibTeX, and in a fixed-width box it wraps like prose.

Splitting only on commas at brace depth zero is the whole trick: a title or an
author list containing a comma sits inside braces, so it survives intact.
"""
import sys


def prettify(raw: str) -> str:
    raw = raw.strip()
    head, _, rest = raw.partition(",")
    fields, depth, buf = [], 0, ""
    for ch in rest:
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
        if ch == "," and depth == 0:
            fields.append(buf.strip())
            buf = ""
        else:
            buf += ch
    tail = buf.strip().rstrip("}").strip()
    if tail:
        fields.append(tail)
    body = ",\n".join("  " + f for f in fields if f)
    return f"{head.strip()},\n{body}\n}}"


if __name__ == "__main__":
    sys.stdout.write(prettify(sys.stdin.read()))
