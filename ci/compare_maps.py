#!/usr/bin/env python3
"""Voxelwise comparison of two NIfTI maps, for the OSF integration check.

Stdlib only (gzip + struct) — no numpy/nibabel — so it runs in a bare CI image.
Reads a qmrust output map and a qMRLab reference map, restricts to voxels both
finite and > 0, and asserts an agreement criterion. `--scale` multiplies the
qmrust map before comparison to reconcile a unit difference (e.g. 1000 for
qmrust seconds vs qMRLab milliseconds).

Exit status is 0 iff the fraction of voxels within `--rel-tol` is at least
`--min-frac` AND the Pearson correlation is at least `--min-corr`.
"""
import argparse
import gzip
import math
import struct
import sys


def read_nii(path):
    """Return the voxel values of a 2D/3D .nii.gz as a flat list of floats."""
    with gzip.open(path, "rb") as f:
        b = f.read()
    dim = struct.unpack("<8h", b[40:56])
    datatype = struct.unpack("<h", b[70:72])[0]
    vox_offset = int(struct.unpack("<f", b[108:112])[0])
    n = 1
    for d in dim[1 : dim[0] + 1]:
        n *= d
    fmt = {16: "f", 64: "d"}.get(datatype)
    if fmt is None:
        sys.exit(f"unsupported NIfTI datatype {datatype} in {path}")
    size = struct.calcsize(fmt)
    raw = b[vox_offset : vox_offset + n * size]
    return list(struct.unpack("<" + fmt * n, raw))


def finite_pos(x):
    return math.isfinite(x) and x > 0.0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("qmrust_map")
    ap.add_argument("ref_map")
    ap.add_argument("--scale", type=float, default=1.0, help="multiply qmrust map before comparing")
    ap.add_argument("--rel-tol", type=float, default=0.01)
    ap.add_argument("--min-frac", type=float, default=0.90)
    ap.add_argument("--min-corr", type=float, default=0.95)
    ap.add_argument("--label", default="map")
    args = ap.parse_args()

    q = read_nii(args.qmrust_map)
    r = read_nii(args.ref_map)
    if len(q) != len(r):
        sys.exit(f"{args.label}: voxel count mismatch {len(q)} vs {len(r)}")

    qs, rs = [], []
    for qi, ri in zip(q, r):
        qi *= args.scale
        if finite_pos(qi) and finite_pos(ri):
            qs.append(qi)
            rs.append(ri)
    if not qs:
        sys.exit(f"{args.label}: no comparable voxels")

    within = sum(1 for a, b in zip(qs, rs) if abs(a - b) <= args.rel_tol * abs(b))
    frac = within / len(qs)

    n = len(qs)
    mq, mr = sum(qs) / n, sum(rs) / n
    cov = sum((a - mq) * (b - mr) for a, b in zip(qs, rs))
    vq = sum((a - mq) ** 2 for a in qs)
    vr = sum((b - mr) ** 2 for b in rs)
    corr = cov / math.sqrt(vq * vr) if vq > 0 and vr > 0 else 0.0

    print(
        f"{args.label}: {n} voxels | within {args.rel_tol:.1%}: {frac:.3%} "
        f"(need {args.min_frac:.1%}) | corr: {corr:.4f} (need {args.min_corr:.2f})"
    )
    if frac < args.min_frac or corr < args.min_corr:
        sys.exit(f"{args.label}: FAILED agreement criterion")
    print(f"{args.label}: OK")


if __name__ == "__main__":
    main()
