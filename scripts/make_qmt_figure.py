#!/usr/bin/env python3
"""qMT-SPGR (SledPikeRP) map comparison: qmrust vs qMRLab vs difference.

Usage:
    python make_qmt_figure.py [DATA_DIR]

DATA_DIR must contain:
    FitResults/*.nii.gz        qMRLab reference maps
    FitResults_rust/*.nii.gz   qmrust maps
    Mask.mat                    brain mask (variable `Mask`)
Writes DATA_DIR/qmt_sledpikerp_comparison.png.

Dependencies: numpy, scipy, matplotlib.
"""
import gzip, struct, os, sys
import numpy as np
import scipy.io as sio
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
from mpl_toolkits.axes_grid1 import make_axes_locatable
from matplotlib.colors import TwoSlopeNorm

BASE = sys.argv[1] if len(sys.argv) > 1 else os.path.expanduser("~/Desktop/qmrust_test/qmt_spgr")
QMRLAB = f"{BASE}/FitResults"
QMRUST = f"{BASE}/FitResults_rust"
OUT = f"{BASE}/qmt_sledpikerp_comparison.png"

def read_nii(path):
    with gzip.open(path, "rb") as f:
        b = f.read()
    dim = struct.unpack("<8h", b[40:56])
    nd = dim[0]
    shape = tuple(dim[1:1+nd])
    n = int(np.prod(shape))
    vox = struct.unpack("<f", b[108:112])[0]
    off = int(vox) if vox > 0 else 352
    data = np.frombuffer(b[off:off+8*n], dtype="<f8").reshape(shape, order="F")
    return np.squeeze(data).astype(float)

mask = np.asarray(sio.loadmat(f"{BASE}/Mask.mat")["Mask"]).astype(float) > 0

# (file key, display title, unit, value cmap)
MAPS = [
    ("F",       "F",        "—",   "viridis"),
    ("kf",      "k_f",      "s⁻¹", "magma"),
    ("kr",      "k_r",      "s⁻¹", "inferno"),
    ("R1f",     "R1_f",     "s⁻¹", "cividis"),
    ("R1r",     "R1_r",     "s⁻¹", "bone"),
    ("T2f",     "T2_f",     "s",   "plasma"),
    ("T2r",     "T2_r",     "s",   "cubehelix"),
    ("resnorm", "resnorm",  "—",   "hot"),
]
DIFF_CMAP = "RdBu_r"
BG = "#0d1117"; FG = "#e6edf3"; MUTED = "#9aa7b2"
disp = lambda a: np.rot90(a)

ncol = len(MAPS)
fig, axes = plt.subplots(3, ncol, figsize=(2.7*ncol, 9.4), facecolor=BG)
fig.subplots_adjust(left=0.035, right=0.965, top=0.88, bottom=0.02, wspace=0.5, hspace=0.08)

def style_bad(name):
    cm = matplotlib.colormaps[name].copy(); cm.set_bad(BG); return cm

def add_cbar(ax, im, lo, hi):
    cax = make_axes_locatable(ax).append_axes("right", size="6%", pad=0.04)
    cb = fig.colorbar(im, cax=cax, ticks=[lo, (lo+hi)/2, hi])
    cb.ax.tick_params(colors=MUTED, labelsize=6.5, length=2)
    cb.outline.set_edgecolor(MUTED); cb.outline.set_linewidth(0.4)
    cb.ax.set_yticklabels([f"{lo:.3g}", f"{(lo+hi)/2:.3g}", f"{hi:.3g}"])

for c, (key, tex, unit, vcmap) in enumerate(MAPS):
    rust = read_nii(f"{QMRUST}/{key}.nii.gz")
    lab  = read_nii(f"{QMRLAB}/{key}.nii.gz")
    m = mask & np.isfinite(rust) & np.isfinite(lab) & (lab != 0)
    rust_m = np.where(m, rust, np.nan)
    lab_m  = np.where(m, lab,  np.nan)
    diff   = np.where(m, lab - rust, np.nan)

    vals = np.concatenate([rust[m], lab[m]])
    lo, hi = np.percentile(vals, [2, 98])
    if hi - lo < 1e-12:
        lo, hi = float(np.nanmin(vals)) - 1e-6, float(np.nanmax(vals)) + 1e-6
    dlim = np.percentile(np.abs(diff[m]), 98)
    dlim = dlim if dlim > 1e-12 else 1e-6

    cmap = style_bad(vcmap)
    im0 = axes[0, c].imshow(disp(rust_m), cmap=cmap, vmin=lo, vmax=hi)
    im1 = axes[1, c].imshow(disp(lab_m),  cmap=cmap, vmin=lo, vmax=hi)
    im2 = axes[2, c].imshow(disp(diff), cmap=style_bad(DIFF_CMAP),
                            norm=TwoSlopeNorm(0.0, -dlim, dlim))
    add_cbar(axes[0, c], im0, lo, hi)
    add_cbar(axes[1, c], im1, lo, hi)
    add_cbar(axes[2, c], im2, -dlim, dlim)

    ttl = f"$\\mathbf{{{tex}}}$" + (f"  [{unit}]" if unit != "—" else "")
    axes[0, c].set_title(ttl, color=FG, fontsize=12, pad=8)

for ax in axes.ravel():
    ax.set_xticks([]); ax.set_yticks([])
    for s in ax.spines.values(): s.set_visible(False)
    ax.set_facecolor(BG)

for r, (lbl, col) in enumerate(zip(
        ["qmrust", "qMRLab", "Δ  (qMRLab − qmrust)"], [FG, FG, "#ff7b72"])):
    axes[r, 0].text(-0.13, 0.5, lbl, transform=axes[r, 0].transAxes,
                    rotation=90, va="center", ha="center",
                    color=col, fontsize=13, fontweight="bold")

fig.suptitle("qMT-SPGR · SledPikeRP  —  qmrust vs qMRLab parameter maps",
             color=FG, fontsize=17, fontweight="bold", x=0.5, y=0.965)
fig.text(0.5, 0.925,
         "rows share color scale per map; difference is diverging about 0",
         color=MUTED, fontsize=9.5, ha="center")

fig.savefig(OUT, dpi=160, facecolor=BG, bbox_inches="tight", pad_inches=0.25)
print("wrote", OUT)
