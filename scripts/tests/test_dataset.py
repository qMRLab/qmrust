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
        "outputs": [],
        "params": [{"name": "MTR", "lower": None, "upper": None, "fixed": False}],
        "recipes": {"bids": "recipe.yaml", "non_bids": "recipe.yaml", "sim": None},
    }
    m.update(over)
    return m


class TestNamedVolumeMatching(unittest.TestCase):
    """Filesystem glob order alphabetizes `mt-off` before `mt-on`, the exact
    opposite of `ROLES = ["MTon", "MToff"]`. `dataset.find` must re-order named
    volumes to the model's roles by entity token, not glob order."""

    def _dataset(self, tmp):
        anat = pathlib.Path(tmp) / "sub-01" / "anat"
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
            hits = sorted(pathlib.Path(tmp).glob("sub-*/anat/*_MTR.nii*"))
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


if __name__ == "__main__":
    unittest.main()
