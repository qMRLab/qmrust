#!/usr/bin/env python3
"""Render the committed example figures for every documented model.

    python3 scripts/make_docs_figures.py --bids-dir ~/Desktop/qmrust-osf \
        --catalog catalog.json

Writes docs/figures/<model>/{inputs,aux,outputs,curve}.webp. A model whose
dataset isn't present is skipped with a warning — the documentation build never
depends on the datasets being present.

The curve panel's forward signal comes from `qmrust sim signal`, so the physics
lives in Rust and is never reimplemented here.
"""
import argparse
import gzip
import json
import math
import pathlib
import subprocess
import sys
import tempfile

import numpy as np
import yaml

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from docsfig import dataset, nifti, style  # noqa: E402


def label_for(identity):
    return ", ".join(f"{k}={v:g}" for k, v in identity.items()) or "volume"


def _title_for(identity, path):
    """A panel title for one volume — wrapped to one field per line once the
    joined label gets long enough to overlap neighboring panels (a function of
    the sidecar key names in this dataset, not of any model identity)."""
    if not identity:
        return path.name.split("_")[-2]
    label = label_for(identity)
    if len(identity) > 1 and len(label) > 28:
        return "\n".join(f"{k}={v:g}" for k, v in identity.items())
    return label


def fig_inputs(coll, model, out_dir):
    n = len(coll.volumes)
    named = model["measurement"]["kind"] == "named"
    if named:
        # `dataset.find` has already ordered `coll.volumes` to the model's
        # canonical roles, so the role name is a truer title than anything
        # derived from the filename.
        roles = model["measurement"]["roles"]
        titles = [r["role"] for r in roles] if len(roles) == n else [
            _title_for(identity, path) for identity, path in coll.volumes
        ]
    else:
        titles = [_title_for(identity, path) for identity, path in coll.volumes]
    panel = 3.2 if any("\n" in t for t in titles) else 2.0
    fig, axes = style.grid(n, panel=panel)
    for ax, (identity, path), title in zip(axes, coll.volumes, titles):
        img = nifti.slice2d(nifti.read_nii(path))
        style.show(ax, img, style.GRAY, title=title)
    for ax in list(axes)[n:]:
        ax.axis("off")
    fig.suptitle(
        f"{model['title']} — {n} acquired volumes ({coll.subject})",
        color=style.FG, fontsize=9,
    )
    style.save(fig, out_dir / "inputs.webp")


def fig_aux(coll, model, out_dir):
    items = []
    required = {i["name"]: i["required"] for i in model["required_inputs"]}
    for name, path in coll.aux.items():
        tag = "required" if required.get(name) else "optional"
        items.append((f"{name} ({tag})", path, "viridis"))
    if coll.mask is not None:
        items.append(("mask", coll.mask, style.GRAY))
    if not items:
        return False
    fig, axes = style.grid(len(items), per_row=4)
    for ax, (title, path, cmap) in zip(axes, items):
        style.show(ax, nifti.slice2d(nifti.read_nii(path)), cmap, title=title)
    for ax in list(axes)[len(items):]:
        ax.axis("off")
    fig.suptitle("Auxiliary inputs", color=style.FG, fontsize=9)
    style.save(fig, out_dir / "aux.webp")
    return True


def fig_outputs(coll, model, out_dir):
    units = {
        o["bids_suffix"]: o["unit"] for o in model["outputs"] if not o["diagnostic"]
    }
    items = [(s, p) for s, p in coll.outputs.items()]
    if not items:
        return False
    mask = nifti.slice2d(nifti.read_nii(coll.mask)) > 0 if coll.mask else None
    fig, axes = style.grid(len(items), per_row=3, panel=2.4)
    for ax, (suffix, path) in zip(axes, items):
        img = nifti.slice2d(nifti.read_nii(path))
        if mask is not None and mask.shape == img.shape:
            img = np.where(mask, img, np.nan)
        u = units.get(suffix, "")
        im = style.show(ax, img, style.cmap_for(u), title=suffix)
        style.colorbar(fig, ax, im, label=u or "a.u.")
    for ax in list(axes)[len(items):]:
        ax.axis("off")
    fig.suptitle(f"Fitted maps ({coll.subject})", color=style.FG, fontsize=9)
    style.save(fig, out_dir / "outputs.webp")
    return True


def pick_voxel(coll):
    """The in-mask voxel nearest the mask centroid — a fixed, reproducible rule."""
    first = nifti.slice2d(nifti.read_nii(coll.volumes[0][1]))
    if coll.mask is None:
        return tuple(s // 2 for s in first.shape)
    mask = nifti.slice2d(nifti.read_nii(coll.mask)) > 0
    idx = np.argwhere(mask)
    if idx.size == 0:
        return tuple(s // 2 for s in first.shape)
    centroid = idx.mean(axis=0)
    return tuple(idx[np.argmin(((idx - centroid) ** 2).sum(axis=1))])


def fit_one_voxel(qmrust, recipe, coll, voxel):
    """Fit a single voxel by handing the CLI a 1x1x1xN volume.

    Returns {output_name: value}. Using the real CLI keeps every fitted value
    consistent with the committed maps and avoids duplicating any model code.
    """
    values = [
        float(nifti.slice2d(nifti.read_nii(p))[voxel]) for _, p in coll.volumes
    ]
    with tempfile.TemporaryDirectory() as d:
        d = pathlib.Path(d)
        nifti.write_nii(d / "voxel.nii.gz", np.array(values).reshape(1, 1, 1, -1))
        cmd = [qmrust, "fit", "--data", str(d / "voxel.nii.gz"),
               "--config", recipe, "--output-dir", str(d / "out")]
        for name, path in coll.aux.items():
            flag = {"B1map": "--b1map", "B0map": "--b0map", "R1map": "--r1map"}.get(name)
            if not flag:
                continue
            aux_val = float(nifti.slice2d(nifti.read_nii(path))[voxel])
            nifti.write_nii(d / f"{name}.nii.gz", np.array([[[aux_val]]]))
            cmd += [flag, str(d / f"{name}.nii.gz")]
        r = subprocess.run(cmd, capture_output=True, text=True)
        if r.returncode != 0:
            raise RuntimeError(r.stderr.strip() or "fit failed")
        out = {}
        for p in sorted((d / "out").rglob("*.nii*")):
            name = p.name.split(".")[0].split("_")[-1]
            out[name] = float(np.atleast_1d(nifti.read_nii(p)).ravel()[0])
        return values, out


def forward_curve(qmrust, recipe_path, params, coll, voxel):
    """`qmrust sim signal` at `params` — the Rust forward model, verbatim."""
    cfg = yaml.safe_load(pathlib.Path(recipe_path).read_text())
    sim = {"params": params}
    for name, key in (("B1map", "b1"), ("B0map", "b0"), ("R1map", "r1")):
        if name in coll.aux:
            sim[key] = float(nifti.slice2d(nifti.read_nii(coll.aux[name]))[voxel])
    cfg["sim"] = sim
    cfg.pop("mask", None)
    with tempfile.TemporaryDirectory() as d:
        d = pathlib.Path(d)
        (d / "sim.yaml").write_text(yaml.safe_dump(cfg, sort_keys=False))
        r = subprocess.run(
            [qmrust, "sim", "signal", "--config", str(d / "sim.yaml"),
             "--output", str(d / "sig.json")],
            capture_output=True, text=True,
        )
        if r.returncode != 0:
            raise RuntimeError(r.stderr.strip() or "sim signal failed")
        return json.loads((d / "sig.json").read_text())["signal"]


def fig_curve(coll, model, out_dir, qmrust, repo_root):
    recipe = str(repo_root / model["recipes"]["non_bids"])
    n_recipe = model["n_volumes"]
    if len(coll.volumes) != n_recipe:
        print(
            f"  skip curve: recipe declares {n_recipe} volumes, dataset has "
            f"{len(coll.volumes)} — the axis would be wrong",
            file=sys.stderr,
        )
        return False
    voxel = pick_voxel(coll)
    try:
        measured, fitted = fit_one_voxel(qmrust, recipe, coll, voxel)
    except RuntimeError as e:
        print(f"  skip curve: single-voxel fit failed ({e})", file=sys.stderr)
        return False

    param_names = [p["name"] for p in model["params"]]
    missing = [p for p in param_names if p not in fitted]
    if missing:
        print(f"  skip curve: fit did not report {missing}", file=sys.stderr)
        return False
    params = {p: fitted[p] for p in param_names}

    named = model["measurement"]["kind"] == "named"
    labels = [label_for(i) or p.name for i, p in coll.volumes] if not named else [
        r["role"] for r in model["measurement"]["roles"]
    ]
    try:
        predicted = forward_curve(qmrust, recipe, params, coll, voxel)
    except RuntimeError as e:
        print(f"  skip curve: sim signal failed ({e})", file=sys.stderr)
        return False

    fig, ax = style.plt.subplots(figsize=(6.4, 3.4), facecolor=style.BG)
    ax.set_facecolor(style.BG)
    if named:
        # MTR-style models carry no amplitude term, so measured and predicted
        # live on unrelated absolute scales. Normalizing each bar pair to its
        # own max would hide a role mismatch by construction (both bars would
        # always reach 1.0). Instead plot the measured values as-is and scale
        # the forward curve by a single common factor (least-squares, not
        # per-bar), so a real discrepancy between measured and predicted
        # remains visible.
        measured_a = np.array(measured)
        predicted_a = np.array(predicted)
        denom = np.sum(predicted_a**2)
        scale = np.sum(measured_a * predicted_a) / denom if denom > 0 else 1.0
        x = np.arange(len(measured))
        ax.bar(x - 0.18, measured_a, width=0.34, label="measured", color="#58a6ff")
        ax.bar(x + 0.18, predicted_a * scale, width=0.34,
               label="forward model (scaled)", color="#f0883e")
        ax.set_xticks(x)
        ax.set_xticklabels(labels, color=style.MUTED, fontsize=7)
    else:
        keys = sorted({k for i, _ in coll.volumes for k in i})
        varying = [k for k in keys if len({i.get(k) for i, _ in coll.volumes}) > 1]
        if len(varying) > 1:
            # More than one identity key varies: no single key is a
            # meaningful axis, so plot in the model's canonical acquisition
            # order instead of implying a trend along an arbitrary key.
            x = np.arange(len(measured))
            order = x
            ax.set_xticks(x)
            ax.set_xticklabels(
                [label_for(i) for i, _ in coll.volumes],
                color=style.MUTED, fontsize=6, rotation=60, ha="right",
            )
            ax.set_xlabel("acquisition", color=style.MUTED, fontsize=8)
        else:
            xkey = varying[0] if varying else (keys[0] if keys else None)
            x = (
                np.array([i[xkey] for i, _ in coll.volumes]) if xkey
                else np.arange(len(measured))
            )
            order = np.argsort(x)
            ax.set_xlabel(xkey or "volume", color=style.MUTED, fontsize=8)
        ax.plot(np.array(x)[order], np.array(predicted)[order], "-",
                color="#f0883e", lw=1.6, label="forward model")
        ax.plot(np.array(x)[order], np.array(measured)[order], "o",
                color="#58a6ff", ms=4, label="measured")
    ax.set_ylabel("signal", color=style.MUTED, fontsize=8)
    ax.tick_params(colors=style.MUTED, labelsize=7)
    for s in ax.spines.values():
        s.set_color(style.MUTED)
        s.set_linewidth(0.4)
    fitted_txt = ", ".join(f"{k}={v:.3g}" for k, v in params.items())
    ax.set_title(
        f"voxel {tuple(int(v) for v in voxel)} — {fitted_txt}",
        color=style.FG, fontsize=8,
    )
    leg = ax.legend(fontsize=7, facecolor=style.BG, edgecolor=style.MUTED)
    for t in leg.get_texts():
        t.set_color(style.FG)
    style.save(fig, out_dir / "curve.webp")
    return True


_AUX_FLAG = {"B1map": "--b1map", "B0map": "--b0map", "R1map": "--r1map"}


def _compressed_size(arr, dtype):
    return len(gzip.compress(nifti.encode(arr, dtype)))


def _pick_probe_voxels(mask, shape):
    """Deterministic candidate voxels, as `(x, y)` — indices along `dims[0]`
    (the array's first/`nx` axis) and `dims[1]` (second/`ny` axis)
    respectively, NIfTI's own on-disk voxel-index order: in-mask nearest the
    mask centroid, then the mask-centroid rule generalized to a handful of
    fixed offsets around it (or the slice center, when there is no mask) —
    always the same voxels for the same slice, so probes are reproducible
    across runs."""
    nx, ny = shape
    if mask is not None and mask.any():
        idx = np.argwhere(mask)  # rows are (axis0, axis1) = (x, y)
        centroid = idx.mean(axis=0)
        order = np.argsort(((idx - centroid) ** 2).sum(axis=1))
        return [tuple(int(v) for v in idx[i]) for i in order[:8]]
    cx, cy = nx // 2, ny // 2
    offsets = [(0, 0), (-3, -3), (-3, 3), (3, -3), (3, 3), (0, 6), (6, 0), (-6, -6)]
    out = []
    for dx, dy in offsets:
        x, y = cx + dx, cy + dy
        if 0 <= x < nx and 0 <= y < ny:
            out.append((x, y))
    return out


def _verify_probes(data_path, probes, sub):
    """Guard the `(x, y)` convention: the bundle's own on-disk data at each
    probe's `(x, y)` must equal the sample the probe was derived from — a
    transposed convention is caught here, not only by a downstream consumer
    reading the contract literally."""
    check = nifti.read_nii(data_path)
    for p in probes:
        x, y = p["x"], p["y"]
        for t, v in enumerate(sub):
            expected = float(np.nan_to_num(v[x, y]))
            got = float(check[x, y, t]) if check.ndim == 3 else float(check[x, y])
            if not math.isclose(expected, got, rel_tol=1e-5, abs_tol=1e-6):
                raise AssertionError(
                    f"{data_path}: probe (x={x}, y={y}) t={t} reads {got} from "
                    f"the bundle but was derived from {expected} — x/y convention "
                    "mismatch between the probe and the written NIfTI"
                )


def _fit_bundle(qmrust, recipe, data_path, aux_paths):
    """Fit the exact bundle NIfTI with the CLI; returns {output_name: 2D array}."""
    with tempfile.TemporaryDirectory() as d:
        d = pathlib.Path(d)
        cmd = [qmrust, "fit", "--data", str(data_path),
               "--config", recipe, "--output-dir", str(d / "out")]
        for name, path in aux_paths.items():
            flag = _AUX_FLAG.get(name)
            if flag:
                cmd += [flag, str(path)]
        r = subprocess.run(cmd, capture_output=True, text=True)
        if r.returncode != 0:
            raise RuntimeError(r.stderr.strip() or "fit failed")
        out = {}
        for p in sorted((d / "out").rglob("*.nii*")):
            name = p.name.split(".")[0].split("_")[-1]
            out[name] = np.atleast_2d(nifti.read_nii(p))
        return out


def bundle_slice(coll, model, out_dir, max_bytes, repo_root, qmrust):
    """Write the playground payload for one model as real NIfTI:

    `<model>.nii.gz` (float32, shape (nx, ny, 1, nt)), `<model>_mask.nii.gz`
    when the collection has a mask, `<model>_<auxname>.nii.gz` per aux input
    the collection resolved (so the fit that produced `probes` is
    reproducible from the shipped payload alone), and `<model>.json` with
    metadata plus a handful of CLI-fitted `probes` voxels the browser's own
    fit must match.

    A probe's `x`/`y` are indices along `dims[0]`/`dims[1]` — the array's
    first and second axes, NIfTI's own on-disk voxel-index order (`data[x,
    y, 0, t]`), not row/column image-display order. `_verify_probes` checks
    this against the just-written file before returning.

    Downsamples by the smallest integer factor whose *compressed* payload
    (data + mask) fits `max_bytes` — gzip changes the size-vs-resolution
    arithmetic, so the budget must be measured on-disk, not on raw floats.
    """
    vols = [nifti.slice2d(nifti.read_nii(p)) for _, p in coll.volumes]
    nt = len(vols)
    mask = (
        nifti.slice2d(nifti.read_nii(coll.mask)) > 0
        if coll.mask is not None and nifti.slice2d(nifti.read_nii(coll.mask)).shape
        == vols[0].shape
        else None
    )

    def build(factor):
        sub = [v[::factor, ::factor] for v in vols]
        h, w = sub[0].shape
        flat = np.zeros((h, w, 1, nt), dtype=np.float32)
        for t, v in enumerate(sub):
            flat[:, :, 0, t] = np.nan_to_num(v)
        msub = mask[::factor, ::factor].astype(np.float32) if mask is not None else None
        return h, w, flat, msub

    factor = 1
    while True:
        h, w, flat, msub = build(factor)
        size = _compressed_size(flat, "f4")
        if msub is not None:
            size += _compressed_size(msub.reshape(h, w, 1), "f4")
        if size <= max_bytes or (h <= 1 and w <= 1):
            break
        factor += 1

    out_dir.mkdir(parents=True, exist_ok=True)
    data_path = out_dir / f"{model['name']}.nii.gz"
    nifti.write_nii(data_path, flat, "f4")
    mask_path = None
    if msub is not None:
        mask_path = out_dir / f"{model['name']}_mask.nii.gz"
        nifti.write_nii(mask_path, msub.reshape(h, w, 1), "f4")

    named = model["measurement"]["kind"] == "named"
    volume_ids = (
        [r["role"] for r in model["measurement"]["roles"]] if named
        else [identity for identity, _ in coll.volumes]
    )
    labels = (
        [r["role"] for r in model["measurement"]["roles"]] if named
        else [label_for(i) for i, _ in coll.volumes]
    )
    # `display_range` is the model's declared window for this quantity, so a map
    # is legible before anyone touches a slider; `None` leaves it to the data.
    outputs = [
        {"name": o["name"], "unit": o["unit"], "display_range": o.get("display_range")}
        for o in model["outputs"] if not o["diagnostic"]
    ]
    enums = model.get("enums", [])

    # Downsampled aux inputs, written into the bundle at the same factor the
    # data was built at, so a fit of this exact bundle — CLI or browser —
    # uses matching aux resolution and reproduces the same fit.
    recipe = str(repo_root / model["recipes"]["non_bids"])
    aux_paths = {}
    aux_files = {}
    for name, path in coll.aux.items():
        if name not in _AUX_FLAG:
            continue
        a = nifti.slice2d(nifti.read_nii(path))[::factor, ::factor]
        a_path = out_dir / f"{model['name']}_{name}.nii.gz"
        nifti.write_nii(a_path, a.reshape(a.shape + (1,)), "f4")
        aux_paths[name] = a_path
        aux_files[name] = a_path.name
    try:
        maps = _fit_bundle(qmrust, recipe, data_path, aux_paths)
    except (RuntimeError, OSError) as e:
        print(f"  probes: skipped, bundle fit failed ({e})", file=sys.stderr)
        maps = {}

    probes = []
    if maps:
        for x, y in _pick_probe_voxels(msub > 0 if msub is not None else None, (h, w)):
            expected = {}
            ok = True
            for o in outputs:
                arr = maps.get(o["name"])
                if arr is None or x >= arr.shape[0] or y >= arr.shape[1]:
                    ok = False
                    break
                v = float(arr[x, y])
                if not math.isfinite(v):
                    ok = False
                    break
                expected[o["name"]] = v
            if ok and expected:
                probes.append({"x": x, "y": y, "expected": expected})
        _verify_probes(data_path, probes, [v[::factor, ::factor] for v in vols])

    meta = {
        "model": model["name"],
        "title": model["title"],
        "bids_suffix": model["bids_suffix"],
        # The picker groups by these; they come from the registry's own
        # taxonomy so the tree it draws is never restated in the app.
        "family": model["family"],
        "family_icon": model["family_icon"],
        "subgroup": model["subgroup"],
        "category_order": model["category_order"],
        "dims": [h, w, 1, nt],
        "factor": factor,
        "volume_ids": volume_ids,
        "labels": labels,
        "params": [p["name"] for p in model["params"]],
        "outputs": outputs,
        "enums": enums,
        # Two recipes, because a recipe is protocol + options and where the
        # protocol comes from differs by input. The pre-baked slice has no
        # sidecars, so its recipe carries the acquisition (`non_bids`). A fetched
        # BIDS dataset resolves its own acquisition from its sidecars, so its
        # recipe carries options only (`bids`) and `resolve_bids` supplies the
        # protocol. Both paths come from the registry's declared recipe paths —
        # never a filename guessed here.
        "config": (repo_root / model["recipes"]["non_bids"]).read_text(),
        "config_bids": (repo_root / model["recipes"]["bids"]).read_text(),
        # Three recipes, because a recipe is protocol + options and where the
        # protocol comes from differs: the sim recipe carries the acquisition
        # and a sim: block of ground-truth parameters, and reads no image data.
        "config_sim": (repo_root / model["recipes"]["sim"]).read_text(),
        # The dataset archive this model's data lives in, by the same
        # `ds-<lowercased BIDS suffix>` rule `make_bids_examples.sh` builds and
        # zips (resolved against `sources.json`'s host).
        "archive": f"ds-{model['bids_suffix'].lower()}.zip",
        "files": {
            "data": data_path.name,
            "mask": mask_path.name if mask_path else None,
            "aux": aux_files,
        },
        "probes": probes,
    }
    (out_dir / f"{model['name']}.json").write_text(json.dumps(meta, indent=1))
    return factor, h, w, len(probes)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--bids-dir", required=True, type=pathlib.Path,
                    help="parent directory holding one ds-<suffix>/ BIDS "
                    "dataset root per model (see scripts/make_bids_examples.sh)")
    ap.add_argument("--catalog", required=True, type=pathlib.Path)
    ap.add_argument("--repo-root", default=".", type=pathlib.Path)
    ap.add_argument("--out", default=None, type=pathlib.Path,
                    help="default: <repo-root>/docs/figures")
    ap.add_argument("--qmrust", default="./target/release/qmrust")
    ap.add_argument("--bundle-slice", action="store_true",
                    help="also write playground data slices")
    ap.add_argument("--max-bytes", type=int, default=600_000,
                    help="per-model playground payload budget, measured on the "
                    "gzip-compressed .nii.gz (not raw float32) — generous enough "
                    "that every model ships at full native resolution and only "
                    "downsamples if it genuinely doesn't fit")
    args = ap.parse_args(argv)

    out_root = args.out or (args.repo_root / "docs" / "figures")
    catalog = json.loads(args.catalog.read_text())
    models = catalog["models"]
    for model in models:
        print(f"{model['name']}:")
        coll = dataset.find(args.bids_dir, model)
        if coll is None:
            print(
                f"  skip: no {model['bids_suffix']} data in "
                f"{dataset.root_for(args.bids_dir, model)}",
                file=sys.stderr,
            )
            continue
        out_dir = out_root / model["name"]
        fig_inputs(coll, model, out_dir)
        print(f"  inputs.webp ({len(coll.volumes)} volumes, {coll.subject})")
        if fig_aux(coll, model, out_dir):
            print("  aux.webp")
        if fig_outputs(coll, model, out_dir):
            print("  outputs.webp")
        if fig_curve(coll, model, out_dir, args.qmrust, args.repo_root):
            print("  curve.webp")
        if args.bundle_slice:
            factor, h, w, n_probes = bundle_slice(
                coll, model, args.repo_root / "docs" / "playground" / "data",
                args.max_bytes, args.repo_root, args.qmrust,
            )
            print(f"  bundle: {h}x{w} (downsampled {factor}x, {n_probes} probes)")

    if args.bundle_slice:
        # The index lists catalog models with a payload present, driven by the
        # catalog's own model names rather than by globbing every `.json` in
        # the directory, which also holds non-payload files (`sources.json`).
        data_dir = args.repo_root / "docs" / "playground" / "data"
        names = sorted(
            m["name"] for m in models if (data_dir / f"{m['name']}.json").exists()
        )
        # The accepted sim.noise.type values are a global fact from NoiseKind,
        # not a per-model one, so they ride the index rather than each payload.
        (data_dir / "index.json").write_text(
            json.dumps(
                {"models": names, "noise_kinds": catalog["noise_kinds"]}, indent=1
            )
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
