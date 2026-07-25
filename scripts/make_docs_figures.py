#!/usr/bin/env python3
"""Render the committed example figures for every documented model.

    python3 scripts/make_docs_figures.py --bids-dir ~/Desktop/ds-qmrust-test \
        --catalog catalog.json

Writes docs/figures/<model>/{inputs,aux,outputs,curve}.webp. A model with no
matching data in the dataset is skipped with a warning — the documentation
build never depends on the dataset being present.

The curve panel's forward signal comes from `qmrust sim signal`, so the physics
lives in Rust and is never reimplemented here.
"""
import argparse
import json
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
        r for r in model["measurement"]["roles"]
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
        # live on unrelated absolute scales; normalize each to its own max so
        # the bars compare shape/ratio, the only thing that is meaningful.
        measured_n = np.array(measured) / max(np.max(np.abs(measured)), 1e-12)
        predicted_n = np.array(predicted) / max(np.max(np.abs(predicted)), 1e-12)
        x = np.arange(len(measured))
        ax.bar(x - 0.18, measured_n, width=0.34, label="measured", color="#58a6ff")
        ax.bar(x + 0.18, predicted_n, width=0.34, label="forward model", color="#f0883e")
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
    ax.set_ylabel("normalized signal" if named else "signal", color=style.MUTED, fontsize=8)
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


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--bids-dir", required=True, type=pathlib.Path)
    ap.add_argument("--catalog", required=True, type=pathlib.Path)
    ap.add_argument("--repo-root", default=".", type=pathlib.Path)
    ap.add_argument("--out", default=None, type=pathlib.Path,
                    help="default: <repo-root>/docs/figures")
    ap.add_argument("--qmrust", default="./target/release/qmrust")
    args = ap.parse_args(argv)

    out_root = args.out or (args.repo_root / "docs" / "figures")
    models = json.loads(args.catalog.read_text())["models"]
    for model in models:
        print(f"{model['name']}:")
        coll = dataset.find(args.bids_dir, model)
        if coll is None:
            print(
                f"  skip: no {model['bids_suffix']} data in {args.bids_dir}",
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
    return 0


if __name__ == "__main__":
    sys.exit(main())
