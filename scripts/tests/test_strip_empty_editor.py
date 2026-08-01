"""Crossref emits an empty editor clause for records with no editor.

The text is upstream's, but a reader sees it, so it is cleaned before it lands
in the payload. A citation that genuinely names an editor must keep it.

Run: python3 -m unittest discover -s scripts/tests -v
"""
import pathlib
import sys
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))

from strip_empty_editor import strip  # noqa: E402


class TestStripEmptyEditor(unittest.TestCase):
    def test_removes_the_empty_mla_and_harvard_clauses(self):
        self.assertEqual(
            strip("Journal of Open Source Software, edited by , vol. 5, no. 53"),
            "Journal of Open Source Software, vol. 5, no. 53",
        )
        self.assertEqual(
            strip("Journal of Open Source Software. Edited by , 5(53), p. 2343"),
            "Journal of Open Source Software. 5(53), p. 2343",
        )

    def test_keeps_a_real_editor(self):
        # The clause only goes when there is nothing between it and the comma.
        text = "A Book. Edited by J. Smith, 2020"
        self.assertEqual(strip(text), text)

    def test_leaves_unrelated_text_alone(self):
        text = "Karakuzu, A. et al. (2020) qMRLab, 5(53), p. 2343"
        self.assertEqual(strip(text), text)


if __name__ == "__main__":
    unittest.main()
