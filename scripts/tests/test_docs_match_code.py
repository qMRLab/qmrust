"""`docs/agents/*.md` must not describe a contract the code no longer has.

A stale agent doc is worse than a missing one: a contributor follows it and
reintroduces the very thing it silently mis-describes. These docs are read as
the authority on how a fit is built and how data flows, so the signatures they
quote have to be the real ones.

Scope, deliberately narrow so it never cries wolf:

  * `pub type X = ...;` and `pub trait X: ...` lines quoted in a ```rust block
    are compared verbatim against the crates. Those are exact by nature, and
    they are where the drift that prompted this test happened (an `Effective`
    alias that had gained a `&Protocol` parameter months after the doc froze).
  * `pub fn name` mentions are checked by *name only*, since the docs
    legitimately abbreviate argument lists.

What it cannot check is prose. A paragraph describing the wrong pipeline order
still reads fine to a parser, so changing a flow still means reading the
surrounding text.

Run: python3 -m unittest discover -s scripts/tests -v
"""
import pathlib
import re
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
DOCS = sorted((ROOT / "docs" / "agents").glob("*.md"))


def rust_blocks(text):
    return re.findall(r"```rust\n(.*?)```", text, re.S)


def crate_sources():
    return "\n".join(
        p.read_text(errors="ignore")
        for p in (ROOT / "crates").rglob("*.rs")
        if "target" not in p.parts
    )


class TestDocsMatchCode(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.src = crate_sources()

    def test_docs_exist(self):
        # A silent zero-file glob would make every check below vacuous.
        self.assertTrue(DOCS, "no docs/agents/*.md found")

    def test_quoted_type_and_trait_signatures_are_real(self):
        checked = 0
        for doc in DOCS:
            for block in rust_blocks(doc.read_text()):
                for line in block.splitlines():
                    s = line.strip()
                    if not (s.startswith("pub type ") or s.startswith("pub trait ")):
                        continue
                    # Compare up to the body/terminator: `{` for a trait, `;`
                    # for an alias.
                    sig = s.split("{")[0].rstrip().rstrip(";").rstrip()
                    self.assertIn(
                        sig,
                        self.src,
                        f"{doc.name} quotes a signature the crates do not have:\n    {sig}",
                    )
                    checked += 1
        self.assertGreater(checked, 0, "no type/trait signatures found to check")

    def test_named_functions_exist(self):
        for doc in DOCS:
            for block in rust_blocks(doc.read_text()):
                for name in re.findall(r"\bpub fn (\w+)", block):
                    self.assertIn(
                        f"pub fn {name}",
                        self.src,
                        f"{doc.name} documents `pub fn {name}`, which no crate defines",
                    )


if __name__ == "__main__":
    unittest.main()
