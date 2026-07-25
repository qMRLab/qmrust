"""Minimal NIfTI-1 read/write — no nibabel dependency.

Handles the subset qmrust writes and the example dataset uses: single-file
`.nii`/`.nii.gz`, little-endian, float32 or float64, 2D–4D.
"""
import gzip
import struct

import numpy as np

_DTYPES = {2: "u1", 4: "<i2", 8: "<i4", 16: "<f4", 64: "<f8"}


def _open(path, mode="rb"):
    return gzip.open(path, mode) if str(path).endswith(".gz") else open(path, mode)


def read_nii(path):
    """Read a NIfTI volume as float64, shaped as stored (squeezed)."""
    with _open(path) as f:
        b = f.read()
    dim = struct.unpack("<8h", b[40:56])
    shape = tuple(dim[1 : 1 + dim[0]])
    code = struct.unpack("<h", b[70:72])[0]
    if code not in _DTYPES:
        raise ValueError(f"{path}: unsupported NIfTI datatype code {code}")
    vox = struct.unpack("<f", b[108:112])[0]
    off = int(vox) if vox > 0 else 352
    n = int(np.prod(shape))
    itemsize = np.dtype(_DTYPES[code]).itemsize
    data = np.frombuffer(b[off : off + itemsize * n], dtype=_DTYPES[code])
    return np.squeeze(data.reshape(shape, order="F")).astype(float)


def write_nii(path, arr):
    """Write `arr` as a float64 NIfTI-1. Used for single-voxel fit inputs."""
    arr = np.asarray(arr, dtype="<f8")
    shape = arr.shape
    hdr = bytearray(348)
    struct.pack_into("<i", hdr, 0, 348)
    dim = [len(shape)] + list(shape) + [1] * (7 - len(shape))
    struct.pack_into("<8h", hdr, 40, *dim)
    struct.pack_into("<h", hdr, 70, 64)      # datatype: float64
    struct.pack_into("<h", hdr, 72, 64)      # bitpix
    struct.pack_into("<f", hdr, 108, 352.0)  # vox_offset
    struct.pack_into("<f", hdr, 76, 1.0)     # pixdim[0]
    for i in range(1, 5):
        struct.pack_into("<f", hdr, 76 + 4 * i, 1.0)
    struct.pack_into("<f", hdr, 112, 1.0)    # scl_slope
    hdr[344:348] = b"n+1\x00"
    with _open(path, "wb") as f:
        f.write(bytes(hdr) + b"\x00" * 4 + arr.tobytes(order="F"))


def slice2d(arr):
    """A 2D display slice: 2D arrays pass through, 3D+ take the middle slice."""
    a = np.asarray(arr)
    while a.ndim > 2:
        a = a[..., a.shape[-1] // 2]
    return a
