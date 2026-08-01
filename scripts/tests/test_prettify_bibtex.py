"""`doi.org` returns BibTeX on one line; the playground shows it as a block.

Run: python3 -m unittest discover -s scripts/tests -v
"""
import pathlib
import sys
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))

from prettify_bibtex import prettify  # noqa: E402


class TestPrettifyBibtex(unittest.TestCase):
    def test_a_comma_inside_braces_is_not_a_field_break(self):
        # The real qMRLab title carries a comma. Splitting on every comma would
        # tear it in half and produce BibTeX that no longer parses.
        raw = (
            "@article{Karakuzu_2020, title={qMRLab: Quantitative MRI analysis, under one "
            "umbrella}, volume={5}, year={2020} }"
        )
        out = prettify(raw)
        self.assertIn("title={qMRLab: Quantitative MRI analysis, under one umbrella},", out)
        self.assertEqual(out.count("\n"), 4, out)  # head + 3 fields, closing brace on the last

    def test_every_field_lands_on_its_own_line_and_the_entry_closes(self):
        out = prettify("@article{k, a={1}, b={2} }")
        self.assertEqual(out, "@article{k,\n  a={1},\n  b={2}\n}")

    def test_an_unbraced_value_survives(self):
        # `month=Sept` comes back without braces from the real service.
        self.assertIn("  month=Sept", prettify("@article{k, month=Sept, pages={1} }"))


if __name__ == "__main__":
    unittest.main()
