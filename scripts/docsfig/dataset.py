"""Locate a model's example data in a BIDS dataset, using only catalog facts.

Every lookup is driven by the model's own declarations — its `bids_suffix`, the
per-volume keys in its `protocol_schema`, its `required_inputs` suffixes, and
its `bids_outputs` suffixes — so this module never names a model.
"""
import dataclasses
import json
import pathlib


@dataclasses.dataclass
class Collection:
    subject: str
    volumes: list          # [(identity: dict[str, float], path: pathlib.Path)]
    aux: dict              # input name -> path
    mask: object           # pathlib.Path | None
    outputs: dict          # BIDS suffix -> path


def _sidecar(nii_path):
    j = pathlib.Path(str(nii_path).replace(".nii.gz", ".json").replace(".nii", ".json"))
    return json.loads(j.read_text()) if j.exists() else {}


def find(bids_dir, model):
    """The collection for `model`, or None when the dataset has no such data."""
    bids_dir = pathlib.Path(bids_dir)
    suffix = model["bids_suffix"]
    hits = sorted(bids_dir.glob(f"sub-*/anat/*_{suffix}.nii*"))
    if not hits:
        return None
    subject = hits[0].parts[-3]
    hits = [h for h in hits if h.parts[-3] == subject]

    per_volume = [
        p["key"] for p in model["protocol_schema"]
        if p["scope"] == "per_volume" and p["key"]
    ]
    volumes = []
    for path in hits:
        meta = _sidecar(path)
        identity = {k: float(meta[k]) for k in per_volume if k in meta}
        volumes.append((identity, path))

    if model["measurement"]["kind"] == "series" and per_volume:
        # Order to the model's own canonical rows: identity, never position.
        rows = model["measurement"]["rows"]
        ordered = []
        pool = list(volumes)
        for row in rows:
            match = next(
                (v for v in pool
                 if all(abs(v[0].get(k, float("nan")) - val) < 1e-9
                        for k, val in row.items())),
                None,
            )
            if match:
                pool.remove(match)
                ordered.append(match)
        if len(ordered) == len(rows):
            volumes = ordered

    deriv = bids_dir / "derivatives"
    aux = {}
    for spec in model["required_inputs"]:
        if not spec["bids_suffix"]:
            continue
        found = sorted(deriv.glob(f"*/{subject}/anat/*_{spec['bids_suffix']}.nii*"))
        if found:
            aux[spec["name"]] = found[0]

    masks = sorted(deriv.glob(f"*/{subject}/anat/*_mask.nii*"))
    outputs = {}
    for o in model["outputs"]:
        if o["diagnostic"]:
            continue
        found = sorted(deriv.glob(f"qmrust/{subject}/anat/*_{o['bids_suffix']}.nii*"))
        if found:
            outputs[o["bids_suffix"]] = found[0]

    return Collection(
        subject=subject,
        volumes=volumes,
        aux=aux,
        mask=masks[0] if masks else None,
        outputs=outputs,
    )
