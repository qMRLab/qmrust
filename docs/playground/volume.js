// Volume arithmetic with no viewer attached: normalising an image to [0, 1],
// finding the box that holds everything above background, and the crop/place-back
// pair that lets a network run over the part of a grid that holds a head instead
// of over all of it.
//
// Indices here are *storage* indices — `x + y*nx + z*nx*ny` — because that is the
// layout every caller holds: NiiVue's own image buffers and the tensor a network
// consumes are both in it. Nothing here knows what a scanner or an affine is.

// `(v - min) / (max - min)`. A constant volume has no range to scale, and reads
// as all-zero rather than as NaN.
export function minMaxNormalize(src) {
  let min = Infinity;
  let max = -Infinity;
  for (const v of src) {
    if (!Number.isFinite(v)) continue;
    if (v < min) min = v;
    if (v > max) max = v;
  }
  const out = new Float32Array(src.length);
  if (!Number.isFinite(min) || max === min) return out;
  const scale = 1 / (max - min);
  for (let i = 0; i < src.length; i++) {
    const v = src[i];
    out[i] = Number.isFinite(v) ? (v - min) * scale : 0;
  }
  return out;
}

// The smallest box containing every voxel above `threshold`, grown by `pad` in
// each direction and clipped to the volume. `lo` is its near corner and `size`
// its extent, both in `[x, y, z]`.
//
// An empty volume yields the whole grid: a network given nothing to look at is a
// better outcome than one given a zero-sized tensor.
export function boundingBox(values, dims, threshold, pad = 0) {
  const [nx, ny, nz] = dims;
  const lo = [nx, ny, nz];
  const hi = [-1, -1, -1];
  for (let z = 0; z < nz; z++) {
    for (let y = 0; y < ny; y++) {
      const row = y * nx + z * nx * ny;
      for (let x = 0; x < nx; x++) {
        if (values[row + x] <= threshold) continue;
        if (x < lo[0]) lo[0] = x;
        if (x > hi[0]) hi[0] = x;
        if (y < lo[1]) lo[1] = y;
        if (y > hi[1]) hi[1] = y;
        if (z < lo[2]) lo[2] = z;
        if (z > hi[2]) hi[2] = z;
      }
    }
  }
  if (hi[0] < 0) return { lo: [0, 0, 0], size: [nx, ny, nz] };
  const near = lo.map((v) => Math.max(0, v - pad));
  const far = hi.map((v, d) => Math.min(dims[d] - 1, v + pad));
  return { lo: near, size: near.map((v, d) => far[d] - v + 1) };
}

// The box `lo`/`size` lifted out of `values`, with its axes reversed: a tensor of
// shape `[sz, sy, sx]` whose element `(i, j, k)` is the box's `(k, j, i)`.
//
// The reversal is not cosmetic. A network of dilated convolutions is not
// symmetric under an axis swap — its weights are not — so a volume has to reach
// it in the axis order it was trained on, and reversing while cropping avoids
// materialising the volume twice to do it.
//
// The result is laid out **row-major for that shape** — `i` slowest, `k` fastest —
// because that is the only layout a tensor library will read it as. Writing the
// box's own storage order (`x` fastest) into a buffer and then declaring the
// reversed shape over it does not transpose anything: it reinterprets the same
// bytes under mismatched strides, which shears the volume.
export function cropReversed(values, dims, lo, size) {
  const [nx, ny] = dims;
  const [sx, sy, sz] = size;
  const out = new Float32Array(sx * sy * sz);
  for (let i = 0; i < sz; i++) {
    for (let j = 0; j < sy; j++) {
      const src = lo[0] + (lo[1] + j) * nx + (lo[2] + i) * nx * ny;
      const dst = (i * sy + j) * sx;
      for (let k = 0; k < sx; k++) {
        out[dst + k] = values[src + k];
      }
    }
  }
  return out;
}

// The inverse of `cropReversed` for a label volume: labels of row-major shape
// `[sz, sy, sx]` written back into a full-size grid at `lo`, everything outside
// the box left at zero.
export function placeReversed(labels, dims, lo, size) {
  const [nx, ny, nz] = dims;
  const [sx, sy, sz] = size;
  const out = new Uint8Array(nx * ny * nz);
  for (let i = 0; i < sz; i++) {
    for (let j = 0; j < sy; j++) {
      const dst = lo[0] + (lo[1] + j) * nx + (lo[2] + i) * nx * ny;
      const src = (i * sy + j) * sx;
      for (let k = 0; k < sx; k++) {
        out[dst + k] = labels[src + k];
      }
    }
  }
  return out;
}

// The class with the highest score at each voxel, for scores laid out with the
// channel last — `[voxel, channel]`, which is how a channels-last network returns
// them. This is `argmax` over the channel axis and nothing more.
export function argmaxChannels(scores, classes) {
  const n = Math.floor(scores.length / classes);
  const out = new Uint8Array(n);
  for (let i = 0; i < n; i++) {
    const base = i * classes;
    let best = 0;
    let bestValue = scores[base];
    for (let c = 1; c < classes; c++) {
      if (scores[base + c] > bestValue) {
        bestValue = scores[base + c];
        best = c;
      }
    }
    out[i] = best;
  }
  return out;
}
