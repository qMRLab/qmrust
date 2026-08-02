"""Tests for the model-documentation generator's pure functions.

Run: python3 -m unittest discover -s scripts/tests -v
"""
import json
import pathlib
import sys
import tempfile
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))

import gen_model_docs as g

REPO = pathlib.Path(__file__).resolve().parents[2]
# Real committed recipes: `render_usage` embeds recipe files verbatim, so a
# rendering test needs paths that actually resolve.
BIDS_RECIPE = "recipes/bids/irt1_config.yaml"
NON_BIDS_RECIPE = "recipes/non-bids/irt1_config.yaml"


def minimal_model(**over):
    m = {
        "name": "demo", "bids_suffix": "DEMO", "title": "Demo model",
        "category": "t1-relaxometry", "category_title": "T1 Relaxometry",
        "family": "T1 Relaxometry", "family_icon": "spline-pointer",
        "subgroup": None, "category_order": 0,
        "summary": "A summary.", "equation": r"S = M_0",
        "symbols": [{"name": "T1", "meaning": "Relaxation time", "unit": "s"}],
        "citations": ["barral2010"],
        "source_dir": "crates/qmrust-core/src/models/demo",
        "recipes": {"bids": BIDS_RECIPE, "non_bids": NON_BIDS_RECIPE, "sim": None},
        "params": [{"name": "T1", "lower": None, "upper": None, "fixed": False}],
        "outputs": [{"name": "T1", "bids_suffix": "T1map", "unit": "s",
                     "diagnostic": False},
                    {"name": "res", "bids_suffix": None, "unit": None,
                     "diagnostic": True}],
        "measurement": {"kind": "series", "roles": [],
                        "rows": [{"InversionTime": 0.35}, {"InversionTime": 0.5}]},
        "protocol_schema": [{"name": "InversionTime", "source": "field",
                             "key": "InversionTime", "scope": "per_volume"}],
        "required_inputs": [],
        "n_volumes": 2, "strategy": "voxelwise",
        "effective_config": "model: demo\n",
    }
    m.update(over)
    return m


class TestRendering(unittest.TestCase):
    def test_unbounded_parameters_render_as_unbounded(self):
        """A null bound is infinity in the Rust model; never print 'None'."""
        table = g.render_outputs_table(minimal_model())
        self.assertIn("unbounded", table)
        self.assertNotIn("None", table)

    def test_series_rows_are_labeled_by_identity(self):
        rows = g.identity_labels(minimal_model())
        self.assertEqual(rows, ["InversionTime=0.35", "InversionTime=0.5"])

    def test_derived_output_with_no_matching_param_is_not_fitted(self):
        """An output absent from `params` is derived, not fitted: never 'free'."""
        m = minimal_model(outputs=[
            {"name": "T1", "bids_suffix": "T1map", "unit": "s",
             "diagnostic": False},
            {"name": "MTR", "bids_suffix": "MTRmap", "unit": "%",
             "diagnostic": False},
            {"name": "res", "bids_suffix": None, "unit": None,
             "diagnostic": True},
        ])
        table = g.render_outputs_table(m)
        self.assertIn("| `MTR` | `MTRmap` | % | — | — |", table)
        self.assertNotIn("| `MTR` | `MTRmap` | % | — | free |", table)

    def test_named_roles_are_labeled_by_role(self):
        m = minimal_model(measurement={
            "kind": "named",
            "roles": [
                {"role": "MTon", "entities": [{"key": "mt", "value": "on"}]},
                {"role": "MToff", "entities": [{"key": "mt", "value": "off"}]},
            ],
            "rows": [],
        })
        self.assertEqual(g.identity_labels(m), ["MTon", "MToff"])

    def test_diagnostics_are_separated_from_quantitative_maps(self):
        page = g.render_page(minimal_model(), figures={}, repo_root=REPO)
        self.assertIn("T1map", page)
        self.assertIn("dropdown", page)   # diagnostics live in a dropdown

    def test_missing_figures_are_not_referenced(self):
        page = g.render_page(minimal_model(), figures={}, repo_root=REPO)
        self.assertNotIn(".webp", page)


class TestBibliography(unittest.TestCase):
    def test_unknown_citation_key_is_an_error(self):
        with tempfile.TemporaryDirectory() as d:
            bib = pathlib.Path(d) / "references.bib"
            bib.write_text("@article{other,\n  title = {x}\n}\n")
            with self.assertRaises(g.GenError):
                g.validate_citations([minimal_model()], bib)

    def test_known_citation_key_passes(self):
        with tempfile.TemporaryDirectory() as d:
            bib = pathlib.Path(d) / "references.bib"
            bib.write_text("@article{barral2010,\n  title = {x}\n}\n")
            g.validate_citations([minimal_model()], bib)


class TestCheckMode(unittest.TestCase):
    def test_check_detects_drift(self):
        """--check is the CI gate: an edited generated page must fail it."""
        with tempfile.TemporaryDirectory() as d:
            root = pathlib.Path(d)
            (root / "docs").mkdir()
            (root / "docs" / "references.bib").write_text(
                "@article{barral2010,\n  title = {x}\n}\n")
            (root / "docs" / "index.md").write_text(
                f"# x\n\n{g.GALLERY_BEGIN}\n{g.GALLERY_END}\n")
            # The generator embeds recipe files, so they must exist under the
            # root it is pointed at.
            for rel in (BIDS_RECIPE, NON_BIDS_RECIPE):
                p = root / rel
                p.parent.mkdir(parents=True, exist_ok=True)
                p.write_text("model: demo\n")
            models = [minimal_model()]
            g.write_all(models, root, check=False)
            g.write_all(models, root, check=True)          # in sync: no raise
            page = root / "docs" / "models" / "t1-relaxometry" / "demo.md"
            page.write_text(page.read_text() + "\nhand edit\n")
            with self.assertRaises(g.GenError):
                g.write_all(models, root, check=True)


if __name__ == "__main__":
    unittest.main()
