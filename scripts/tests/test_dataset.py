"""Tests for `docsfig.dataset` and the bundle it feeds the playground.

Run: python3 -m unittest discover -s scripts/tests -v
"""
import json
import pathlib
import sys
import tempfile
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))

from docsfig import dataset, nifti  # noqa: E402
import make_docs_figures as figs  # noqa: E402


def named_model(**over):
    m = {
        "name": "mt_ratio_demo", "bids_suffix": "MTR",
        "title": "MTR demo",
        "family": "Magnetization Transfer", "family_icon": "waves-arrow-up",
        "subgroup": "Semi-quantitative MT", "category_order": 3,
        "measurement": {
            "kind": "named",
            "roles": [
                {"role": "MTon", "entities": [{"key": "mt", "value": "on"}]},
                {"role": "MToff", "entities": [{"key": "mt", "value": "off"}]},
            ],
            "rows": [],
        },
        "protocol_schema": [],
        "required_inputs": [],
        "outputs": [{"name": "MTR", "bids_suffix": "MTR", "unit": "%", "diagnostic": False}],
        "params": [{"name": "MTR", "lower": None, "upper": None, "fixed": False}],
        # Every registered model declares one entry per parameter it fits, and the
        # payload carries them so the form can label a ground-truth value with the
        # model's own unit.
        "symbols": [{"name": "MTR", "meaning": "Magnetization transfer ratio", "unit": "%"}],
        "recipes": {"bids": "recipe.yaml", "non_bids": "recipe.yaml", "sim": "recipe.yaml"},
    }
    m.update(over)
    return m


class TestNamedVolumeMatching(unittest.TestCase):
    """Filesystem glob order alphabetizes `mt-off` before `mt-on`, the exact
    opposite of `ROLES = ["MTon", "MToff"]`. `dataset.find` must re-order named
    volumes to the model's roles by entity token, not glob order."""

    def _dataset(self, tmp):
        # Each model's data is its own dataset root under a common parent, so
        # the fixture nests one — located the same way `dataset.find` does,
        # rather than by a hardcoded directory name.
        anat = dataset.root_for(tmp, named_model()) / "sub-01" / "anat"
        anat.mkdir(parents=True)
        # A saturated (MT-on) image is darker than the reference (MT-off).
        nifti.write_nii(anat / "sub-01_mt-off_MTR.nii.gz",
                        [[100.0, 100.0], [100.0, 100.0]])
        (anat / "sub-01_mt-off_MTR.json").write_text("{}")
        nifti.write_nii(anat / "sub-01_mt-on_MTR.nii.gz",
                        [[50.0, 50.0], [50.0, 50.0]])
        (anat / "sub-01_mt-on_MTR.json").write_text("{}")
        return tmp

    def test_glob_order_is_not_role_order(self):
        with tempfile.TemporaryDirectory() as tmp:
            self._dataset(tmp)
            hits = sorted(dataset.root_for(tmp, named_model()).glob("sub-*/anat/*_MTR.nii*"))
            self.assertEqual([h.name for h in hits],
                              ["sub-01_mt-off_MTR.nii.gz", "sub-01_mt-on_MTR.nii.gz"])

    def test_find_orders_volumes_to_roles(self):
        with tempfile.TemporaryDirectory() as tmp:
            self._dataset(tmp)
            coll = dataset.find(tmp, named_model())
            names = [path.name for _, path in coll.volumes]
            self.assertEqual(names, [
                "sub-01_mt-on_MTR.nii.gz",   # role 0: MTon
                "sub-01_mt-off_MTR.nii.gz",  # role 1: MToff
            ])

    def test_bundle_plane_order_matches_declared_volume_ids(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = pathlib.Path(tmp)
            self._dataset(tmp)
            (tmp / "recipe.yaml").write_text("model: mt_ratio\n")
            model = named_model()
            coll = dataset.find(tmp, model)
            out_dir = tmp / "bundle"
            factor, h, w, _ = figs.bundle_slice(
                coll, model, out_dir, max_bytes=10_000, repo_root=tmp,
                qmrust="./target/release/qmrust",
            )
            meta = json.loads((out_dir / f"{model['name']}.json").read_text())
            self.assertEqual(meta["volume_ids"], ["MTon", "MToff"])
            self.assertEqual(meta["files"]["data"], f"{model['name']}.nii.gz")
            dims = meta["dims"]
            self.assertEqual(dims[:3], [h, w, 1])
            nt = dims[3]
            planes = nifti.read_nii(out_dir / meta["files"]["data"])
            plane_means = [planes[:, :, t].mean() for t in range(nt)]
            # volume_ids[0] == "MTon" must be the darker (saturated) plane.
            self.assertLess(plane_means[0], plane_means[1])


class TestProbeConvention(unittest.TestCase):
    """`probes[].x`/`.y` must index the bundle exactly like `dims[0]`/`dims[1]`
    do — NIfTI's own on-disk voxel order — not row/column image-display
    order. An asymmetric per-voxel value pattern makes a transposed
    convention fail immediately, unlike the constant-valued fixture above."""

    def _dataset(self, tmp):
        # Each model's data is its own dataset root under a common parent, so
        # the fixture nests one — located the same way `dataset.find` does,
        # rather than by a hardcoded directory name.
        anat = dataset.root_for(tmp, named_model()) / "sub-01" / "anat"
        anat.mkdir(parents=True)
        # 4 (x) by 5 (y): value = 10*x + y, so every off-diagonal voxel
        # changes under an x/y swap.
        grid = [[10 * x + y for y in range(5)] for x in range(4)]
        nifti.write_nii(anat / "sub-01_mt-off_MTR.nii.gz", grid)
        (anat / "sub-01_mt-off_MTR.json").write_text("{}")
        nifti.write_nii(anat / "sub-01_mt-on_MTR.nii.gz",
                        [[v / 2 for v in row] for row in grid])
        (anat / "sub-01_mt-on_MTR.json").write_text("{}")
        return tmp

    def _bundle(self, tmp):
        (tmp / "recipe.yaml").write_text("model: mt_ratio\n")
        model = named_model()
        coll = dataset.find(tmp, model)
        out_dir = tmp / "bundle"
        figs.bundle_slice(
            coll, model, out_dir, max_bytes=10_000, repo_root=tmp,
            qmrust="./target/release/qmrust",
        )
        return coll, out_dir, model

    def test_probe_xy_matches_bundle_storage_order(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = pathlib.Path(tmp)
            self._dataset(tmp)
            _, out_dir, model = self._bundle(tmp)
            meta = json.loads((out_dir / f"{model['name']}.json").read_text())
            data = nifti.read_nii(out_dir / meta["files"]["data"])
            self.assertTrue(meta["probes"], "no probes recorded")
            for p in meta["probes"]:
                x, y = p["x"], p["y"]
                # plane 0 is MTon = grid/2 at (x, y); a transposed convention
                # would read (10*y + x)/2 here instead.
                self.assertAlmostEqual(float(data[x, y, 0]), (10 * x + y) / 2, places=4)

    def test_verify_probes_rejects_a_transposed_convention(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = pathlib.Path(tmp)
            self._dataset(tmp)
            coll, out_dir, model = self._bundle(tmp)
            data_path = out_dir / f"{model['name']}.nii.gz"
            sub = [nifti.slice2d(nifti.read_nii(p)) for _, p in coll.volumes]
            probes = [{"x": 1, "y": 3}]
            figs._verify_probes(data_path, probes, sub)  # matches: no raise
            # A source transposed relative to what was written (the bug this
            # guards against) must be caught, not silently accepted.
            transposed = [v.T for v in sub]
            with self.assertRaises(AssertionError):
                figs._verify_probes(data_path, probes, transposed)


class TestAuxDiscovery(unittest.TestCase):
    """A declared optional input must be found wherever `bidsify` files it.

    Maps are placed by BIDS datatype convention — a transmit/off-resonance map
    under `fmap/`, a parameter map under `anat/` — so a discovery that searches
    only one datatype silently drops the other. The symptom is the worst kind:
    the app fits uncorrected while the CLI corrects, and both produce plausible
    maps that simply disagree.
    """

    def _dataset(self, tmp, datatype):
        model = named_model(required_inputs=[
            {"name": "B1map", "bids_suffix": "TB1map", "required": False},
        ])
        root = dataset.root_for(tmp, model)
        anat = root / "sub-01" / "anat"
        anat.mkdir(parents=True)
        for mt in ("on", "off"):
            nifti.write_nii(anat / f"sub-01_mt-{mt}_MTR.nii.gz", [[1.0, 1.0], [1.0, 1.0]])
            (anat / f"sub-01_mt-{mt}_MTR.json").write_text("{}")
        aux_dir = root / "derivatives" / "preprocessed" / "sub-01" / datatype
        aux_dir.mkdir(parents=True)
        nifti.write_nii(aux_dir / "sub-01_TB1map.nii.gz", [[1.0, 1.0], [1.0, 1.0]])
        return model

    def test_finds_aux_under_fmap(self):
        with tempfile.TemporaryDirectory() as tmp:
            model = self._dataset(tmp, "fmap")
            coll = dataset.find(tmp, model)
            self.assertIn("B1map", coll.aux)
            self.assertEqual(coll.aux["B1map"].name, "sub-01_TB1map.nii.gz")

    def test_two_candidates_for_one_aux_input_is_an_error(self):
        # The glob spans every pipeline and datatype, so two derivatives can
        # claim the same input. Taking the first silently fits against a map
        # nobody chose, and a CLI run that picked the other would disagree.
        with tempfile.TemporaryDirectory() as tmp:
            model = self._dataset(tmp, "fmap")
            second = (
                dataset.root_for(tmp, model)
                / "derivatives" / "other" / "sub-01" / "anat"
            )
            second.mkdir(parents=True)
            nifti.write_nii(second / "sub-01_TB1map.nii.gz", [[2.0, 2.0], [2.0, 2.0]])
            with self.assertRaises(ValueError) as caught:
                dataset.find(tmp, model)
            self.assertIn("TB1map", str(caught.exception))

    def test_finds_aux_under_anat(self):
        with tempfile.TemporaryDirectory() as tmp:
            model = self._dataset(tmp, "anat")
            coll = dataset.find(tmp, model)
            self.assertIn("B1map", coll.aux)


class TestCommittedBundlesMatchTheirRecipes(unittest.TestCase):
    """A committed payload's embedded recipes must still be the recipes on disk.

    Each payload carries its model's three recipes as text, and the playground
    seeds its editor from them, but the payloads are regenerated only by a
    figure run that needs the BIDS datasets. So editing a recipe without that run
    leaves the browser serving a recipe no test ever validated, while the CLI and
    every Rust property test go on reading the current file. Matching against the
    recipe directories rather than a name map keeps this independent of how a
    model's recipe is named.
    """

    RECIPE_DIRS = {
        "config": "recipes/non-bids",
        "config_bids": "recipes/bids",
        "config_sim": "recipes/sim",
    }

    def test_every_embedded_recipe_is_a_current_recipe_file(self):
        root = pathlib.Path(__file__).resolve().parents[2]
        payloads = [
            p for p in sorted((root / "docs/playground/data").glob("*.json"))
            if "model" in json.loads(p.read_text())
        ]
        self.assertTrue(payloads, "no model payloads found, so this proves nothing")
        for payload in payloads:
            meta = json.loads(payload.read_text())
            for key, recipe_dir in self.RECIPE_DIRS.items():
                on_disk = {f.read_text() for f in (root / recipe_dir).glob("*.yaml")}
                # `assertTrue`, not `assertIn`: the latter reports the failure by
                # dumping every recipe in the directory beside the payload's copy,
                # which buries the one line that says what to do about it.
                self.assertTrue(
                    meta[key] in on_disk,
                    f"{payload.name}: {key} matches no file in {recipe_dir}/ — "
                    "a recipe changed without the payloads being regenerated "
                    "(scripts/make_docs_figures.py --bids-dir ...)",
                )


if __name__ == "__main__":
    unittest.main()
