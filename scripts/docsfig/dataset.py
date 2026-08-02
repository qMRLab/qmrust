"""Locate a model's example data in a BIDS dataset, using only catalog facts.

Every lookup is driven by the model's own declarations — its `bids_suffix`, the
per-volume keys in its `protocol_schema`, its `required_inputs` suffixes, and
its `bids_outputs` suffixes — so this module never names a model.

Each model's example data is its own single-subject BIDS dataset, rooted at
`ds-<lowercased bids_suffix>/` under a common parent (see
`scripts/make_bids_examples.sh`). The directory name is derived from the model's
own suffix, so there is no model→directory table to maintain.
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


def root_for(parent, model):
    """`model`'s own dataset root under `parent`."""
    return pathlib.Path(parent) / f"ds-{model['bids_suffix'].lower()}"


def _entity_tokens(path):
    """The underscore-delimited tokens of a BIDS filename, extension removed.

    Matching entities as substrings would let `flip-1` claim a `flip-10` volume —
    the declared roles here are single-digit today, so this is about the ordering
    staying correct for a dataset that is not.
    """
    name = path.name
    for ext in (".nii.gz", ".nii"):
        if name.endswith(ext):
            name = name[: -len(ext)]
            break
    return set(name.split("_"))


def _require_complete(model, what, ordered, declared, leftover):
    """Every declaration matched exactly one volume, and nothing was left over.

    A duplicate match is impossible by construction — each match is removed from
    the pool — so the two ways to be wrong are a declaration that matched nothing
    and a volume no declaration claimed. Both mean this dataset is not the one the
    model describes.
    """
    if len(ordered) == len(declared) and not leftover:
        return
    raise ValueError(
        f"{model['name']}: matched {len(ordered)} of {len(declared)} {what} in "
        f"{root_for('.', model).name}"
        + (f", leaving {[p.name for _, p in leftover]} unclaimed" if leftover else "")
        + " — the dataset does not match what the model declares"
    )


def find(parent, model):
    """The collection for `model`, or None when its dataset isn't there.

    `parent` holds one dataset root per model (see `root_for`).
    """
    suffix = model["bids_suffix"]
    bids_dir = root_for(parent, model)
    # Any datatype directory: an acquisition is filed by its BIDS suffix, so a
    # transmit-field series lands in `fmap/` and a weighted series in `anat/`.
    hits = sorted(bids_dir.glob(f"sub-*/*/*_{suffix}.nii*"))
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

    # Resolution is all-or-nothing. A partial match used to leave `volumes` in
    # glob order, which is the one outcome with no symptom: every figure still
    # renders, and the volumes behind it are simply attributed to the wrong
    # acquisitions. A dataset this model cannot be matched against is a broken
    # dataset, so it stops the build instead.
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
        _require_complete(model, "series rows", ordered, rows, pool)
        volumes = ordered
    elif model["measurement"]["kind"] == "named":
        # Order to the model's own canonical roles: BIDS filename entity
        # tokens (e.g. "mt-on"), never glob order.
        roles = model["measurement"]["roles"]
        ordered = []
        pool = list(volumes)
        for role in roles:
            tokens = {f"{e['key']}-{e['value']}" for e in role["entities"]}
            match = next(
                (v for v in pool if tokens <= _entity_tokens(v[1])),
                None,
            )
            if match:
                pool.remove(match)
                ordered.append(match)
        _require_complete(model, "named roles", ordered, roles, pool)
        volumes = ordered

    deriv = bids_dir / "derivatives"
    aux = {}
    for spec in model["required_inputs"]:
        if not spec["bids_suffix"]:
            continue
        # Any datatype directory: bidsify files a map by BIDS convention, so a
        # transmit/off-resonance map lands in `fmap/` and a parameter map in
        # `anat/`. Globbing only one silently drops the other, and the fit then
        # runs uncorrected while the CLI applies it.
        found = sorted(deriv.glob(f"*/{subject}/*/*_{spec['bids_suffix']}.nii*"))
        if len(found) > 1:
            # Several derivatives claim the same input. Picking one silently
            # would fit against a map nobody chose, and the figures and the
            # playground payload would disagree with a CLI run that picked
            # differently.
            raise ValueError(
                f"{model['name']}: {len(found)} candidates for aux input "
                f"'{spec['name']}' ({spec['bids_suffix']}): "
                f"{[str(p) for p in found]}. Leave exactly one in the dataset."
            )
        if found:
            aux[spec["name"]] = found[0]

    masks = sorted(deriv.glob(f"*/{subject}/anat/*_mask.nii*"))
    outputs = {}
    for o in model["outputs"]:
        if o["diagnostic"]:
            continue
        # Any datatype directory, for the same reason as the aux glob above:
        # an output map is filed by its own BIDS suffix, so a fitted transmit
        # map lands in `fmap/`.
        found = sorted(deriv.glob(f"qmrust/{subject}/*/*_{o['bids_suffix']}.nii*"))
        if found:
            outputs[o["bids_suffix"]] = found[0]

    return Collection(
        subject=subject,
        volumes=volumes,
        aux=aux,
        mask=masks[0] if masks else None,
        outputs=outputs,
    )
