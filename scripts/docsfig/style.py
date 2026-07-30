"""Shared figure style. Deterministic by construction: fixed colormaps, fixed
percentile windows, no randomness, no timestamps — so a regenerated figure
differs only when the data or the code did.
"""
import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt  # noqa: E402
import numpy as np  # noqa: E402
from mpl_toolkits.axes_grid1 import make_axes_locatable  # noqa: E402

BG = "#0d1117"
FG = "#e6edf3"
MUTED = "#9aa7b2"
LO_PCT, HI_PCT = 2.0, 98.0
GRAY = "gray"
# Colormap per quantity, chosen by unit so it never depends on a model name.
CMAP_BY_UNIT = {"s": "magma", "1/s": "viridis", "%": "cividis", "": "inferno"}
WEBP_KWARGS = {"format": "webp", "lossless": False, "quality": 90, "method": 6}


def cmap_for(unit):
    return matplotlib.colormaps[CMAP_BY_UNIT.get(unit, "inferno")].copy()


def window(a):
    """Fixed percentile window over finite values."""
    v = np.asarray(a)[np.isfinite(a)]
    if v.size == 0:
        return 0.0, 1.0
    lo, hi = np.percentile(v, [LO_PCT, HI_PCT])
    return (float(lo), float(hi)) if hi > lo else (float(lo), float(lo) + 1.0)


def show(ax, img, cmap, lo=None, hi=None, title=None):
    cm = matplotlib.colormaps[cmap].copy() if isinstance(cmap, str) else cmap
    cm.set_bad(BG)
    if lo is None:
        lo, hi = window(img)
    im = ax.imshow(np.rot90(img), cmap=cm, vmin=lo, vmax=hi, interpolation="nearest")
    ax.set_facecolor(BG)
    ax.set_xticks([])
    ax.set_yticks([])
    for s in ax.spines.values():
        s.set_visible(False)
    if title:
        ax.set_title(title, color=FG, fontsize=7.5, pad=3)
    return im


def colorbar(fig, ax, im, label=""):
    cax = make_axes_locatable(ax).append_axes("right", size="6%", pad=0.04)
    cb = fig.colorbar(im, cax=cax)
    cb.ax.tick_params(colors=MUTED, labelsize=6, length=2)
    cb.outline.set_edgecolor(MUTED)
    cb.outline.set_linewidth(0.4)
    if label:
        cb.set_label(label, color=MUTED, fontsize=6.5)
    return cb


def grid(n, per_row=5, panel=2.0):
    rows = (n + per_row - 1) // per_row
    cols = min(n, per_row)
    fig, axes = plt.subplots(
        rows, cols, figsize=(panel * cols, panel * rows), facecolor=BG, squeeze=False
    )
    for ax in axes.flat:
        ax.set_facecolor(BG)
        ax.set_xticks([])
        ax.set_yticks([])
        for s in ax.spines.values():
            s.set_visible(False)
    return fig, axes.flat


def save(fig, path):
    """Write WebP via Pillow (matplotlib has no WebP backend)."""
    import io

    from PIL import Image

    path.parent.mkdir(parents=True, exist_ok=True)
    buf = io.BytesIO()
    fig.savefig(buf, format="png", facecolor=BG, dpi=160, bbox_inches="tight")
    plt.close(fig)
    buf.seek(0)
    Image.open(buf).convert("RGB").save(path, **WEBP_KWARGS)
