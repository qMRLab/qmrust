// Volume arithmetic with no viewer attached: normalising an image to [0, 1],
// finding the box that holds everything above background, and the crop/place-back
// pair that lets a network run over the part of a grid that holds a head instead
// of over all of it.
//
// Indices here are *storage* indices — `x + y*nx + z*nx*ny` — because that is the
// layout every caller holds: NiiVue's own image buffers and the tensor a network
// consumes are both in it. Nothing here knows what a scanner or an affine is.

// The intensity a segmentation network's weights expect to see white matter at.
// FreeSurfer's `conform` — the step these networks were trained behind — puts it
// near 110 of 255, and a network shown a differently-scaled volume reads tissue as
// noise and returns almost nothing.
const WHITE_MATTER = 110 / 255;

// `src` rescaled so its bright-tissue mode lands at `WHITE_MATTER`, clipped to
// [0, 1].
//
// A min–max normalisation will not do, and neither will a display window. Both are
// set by the extremes — one bright voxel, or a range chosen for looking at an
// acquisition — while what the network is sensitive to is where *tissue* sits. So
// the mode of the bright half of the tissue histogram is found and scaled there,
// which is the one-parameter form of what FreeSurfer's intensity normalisation
// does.
//
// The histogram is built in one pass over the volume rather than by sorting it:
// these are 16 million voxels.
export function normalizeToWhiteMatter(src, bins = 256) {
  const out = new Float32Array(src.length);
  let max = 0;
  for (const v of src) if (v > max) max = v;
  if (!(max > 0)) return out;

  // Background outnumbers tissue several times over, so it must not vote.
  const floor = max * 0.02;
  const counts = new Float64Array(bins);
  const width = (max - floor) / bins;
  let tissue = 0;
  for (const v of src) {
    if (v <= floor) continue;
    counts[Math.min(bins - 1, Math.floor((v - floor) / width))]++;
    tissue++;
  }
  if (tissue === 0) return out;

  // The mode of the brighter half of the tissue. The whole tissue histogram's own
  // mode is the dark end — grey matter, CSF and the noise just above background —
  // and scaling *that* to white matter's level leaves the volume several times too
  // bright.
  let seen = 0;
  let from = 0;
  for (let b = 0; b < bins; b++) {
    seen += counts[b];
    if (seen >= tissue * 0.5) { from = b; break; }
  }
  let mode = from;
  for (let b = from; b < bins; b++) if (counts[b] > counts[mode]) mode = b;

  const level = floor + (mode + 0.5) * width;
  const scale = WHITE_MATTER / level;
  for (let i = 0; i < src.length; i++) {
    const v = src[i];
    out[i] = Number.isFinite(v) && v > 0 ? Math.min(1, v * scale) : 0;
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

// The intensity below which a voxel is background, by Otsu's criterion — the split
// that maximises the variance between the classes it makes.
//
// Three classes, and the *lower* threshold. MRI histograms have three parts, not
// two: background, tissue, and something much brighter (CSF on a T2, a bright rim,
// a noise spike). Asked for two classes, Otsu isolates that bright group and calls
// tissue background — which on a multi-echo dataset here kept only the bright rim
// and threw away the brain inside it. Asked for three, the lower split lands where
// it should. Nothing about the image needs to be known for this, which is the point:
// it is the same call for any contrast.
export function backgroundThreshold(values, bins = 128) {
  let min = Infinity;
  let max = -Infinity;
  for (const v of values) {
    if (!Number.isFinite(v)) continue;
    if (v < min) min = v;
    if (v > max) max = v;
  }
  if (!(max > min)) return min;
  const width = (max - min) / bins;
  const counts = new Float64Array(bins);
  for (const v of values) {
    if (!Number.isFinite(v)) continue;
    counts[Math.min(bins - 1, Math.floor((v - min) / width))]++;
  }
  // Prefix sums of the count and of the first moment, so a class's contribution to
  // the between-class variance is two lookups however wide it is.
  const count = new Float64Array(bins + 1);
  const moment = new Float64Array(bins + 1);
  for (let b = 0; b < bins; b++) {
    count[b + 1] = count[b] + counts[b];
    moment[b + 1] = moment[b] + b * counts[b];
  }
  const between = (from, to) => {
    const weight = count[to] - count[from];
    if (weight === 0) return 0;
    const sum = moment[to] - moment[from];
    return (sum * sum) / weight;
  };
  let best = -1;
  let cut = 1;
  for (let low = 1; low < bins - 1; low++) {
    for (let high = low + 1; high < bins; high++) {
      const variance = between(0, low) + between(low, high) + between(high, bins);
      if (variance > best) {
        best = variance;
        cut = low;
      }
    }
  }
  return min + cut * width;
}

// A box blur of `radius` voxels, separable and clamped at the edges.
//
// Thresholding without it is what fails on low-SNR data: noise above the threshold
// speckles the background, and — worse — noise *below* it perforates the subject, so
// the subject stops being one connected thing. Averaging shrinks noise and leaves
// anatomy, which is the whole reason this step exists.
//
// An axis of extent 1 is skipped, so a single slice blurs in-plane rather than
// pretending it has depth.
export function blurBox(values, dims, radius = 1) {
  const [nx, ny, nz] = dims;
  const strides = [1, nx, nx * ny];
  let current = Float32Array.from(values);
  for (let axis = 0; axis < 3; axis++) {
    if (dims[axis] < 2) continue;
    const step = strides[axis];
    const extent = dims[axis];
    const out = new Float32Array(current.length);
    for (let z = 0; z < nz; z++) {
      for (let y = 0; y < ny; y++) {
        for (let x = 0; x < nx; x++) {
          const at = x + y * nx + z * nx * ny;
          const index = axis === 0 ? x : axis === 1 ? y : z;
          let sum = 0;
          let n = 0;
          for (let d = -radius; d <= radius; d++) {
            const j = index + d;
            if (j < 0 || j >= extent) continue;
            sum += current[at + d * step];
            n++;
          }
          out[at] = sum / n;
        }
      }
    }
    current = out;
  }
  return current;
}

/**
 * Grow or shrink a binary mask by `steps` voxels — positive dilates, negative
 * erodes, zero returns a copy.
 *
 * One iteration is a 6-neighbour pass: dilation adds any background voxel
 * touching the mask across a face, erosion drops any mask voxel that does not
 * have all its neighbours inside. Faces rather than corners, because a
 * diagonal step is longer than a voxel and would grow the mask faster along
 * the diagonals than across them.
 *
 * An axis of extent 1 is skipped, so a single-slice mask is grown in-plane and
 * not through a depth it does not have. Eroding a 2D mask through a nonexistent
 * z would find every voxel missing a neighbour and erase the whole thing —
 * which is most of the example datasets.
 *
 * Erosion treats outside the volume as inside, so a mask running to the edge of
 * the field of view is not eaten away from the border it never really had.
 *
 * `strides` says how the caller indexes its own buffer; the default is this
 * module's storage order (`x + y*nx + z*nx*ny`), and the fit's own layout
 * (`(x*ny + y)*nz + z`) passes `[ny*nz, nz, 1]`.
 */
export function growMask(mask, dims, steps, strides = [1, dims[0], dims[0] * dims[1]]) {
  let current = Uint8Array.from(mask, (v) => (v ? 1 : 0));
  const grow = steps > 0;
  for (let pass = 0; pass < Math.abs(steps); pass++) {
    const out = new Uint8Array(current.length);
    for (let z = 0; z < dims[2]; z++) {
      for (let y = 0; y < dims[1]; y++) {
        for (let x = 0; x < dims[0]; x++) {
          const at = x * strides[0] + y * strides[1] + z * strides[2];
          const here = current[at] === 1;
          // Dilation only ever changes background voxels, erosion only mask
          // voxels; everything else keeps the value it already has.
          if (here === grow) {
            out[at] = here ? 1 : 0;
            continue;
          }
          // A candidate flips when it touches a voxel of the value we are
          // spreading: for dilation a neighbour inside the mask, for erosion a
          // neighbour outside it.
          let flips = false;
          const index = [x, y, z];
          for (let axis = 0; axis < 3 && !flips; axis++) {
            if (dims[axis] < 2) continue;
            for (const d of [-1, 1]) {
              const j = index[axis] + d;
              // Off the edge: nothing to grow in from, and nothing missing.
              if (j < 0 || j >= dims[axis]) continue;
              if ((current[at + d * strides[axis]] === 1) !== grow) continue;
              flips = true;
              break;
            }
          }
          out[at] = flips === grow ? 1 : 0;
        }
      }
    }
    current = out;
  }
  return current;
}

// Whether a voxel is on a face of the volume. An axis of extent 1 has no faces to
// be on: a single slice is not "surrounded by" the outside, or every voxel in it
// would be, and nothing could ever be enclosed.
function onBorder(x, y, z, [nx, ny, nz]) {
  return (
    (nx > 1 && (x === 0 || x === nx - 1)) ||
    (ny > 1 && (y === 0 || y === ny - 1)) ||
    (nz > 1 && (z === 0 || z === nz - 1))
  );
}

// Visit every voxel reachable from `seeds` through `passable`, six-connected,
// marking each in `seen` and returning how many were reached.
//
// An explicit stack, not recursion: a component can be sixteen million voxels.
function flood(seeds, passable, seen, dims) {
  const [nx, ny, nz] = dims;
  const plane = nx * ny;
  const stack = seeds.slice();
  let reached = 0;
  while (stack.length > 0) {
    const at = stack.pop();
    if (seen[at]) continue;
    seen[at] = 1;
    reached++;
    const z = Math.floor(at / plane);
    const y = Math.floor((at - z * plane) / nx);
    const x = at - z * plane - y * nx;
    if (x > 0 && passable(at - 1) && !seen[at - 1]) stack.push(at - 1);
    if (x < nx - 1 && passable(at + 1) && !seen[at + 1]) stack.push(at + 1);
    if (y > 0 && passable(at - nx) && !seen[at - nx]) stack.push(at - nx);
    if (y < ny - 1 && passable(at + nx) && !seen[at + nx]) stack.push(at + nx);
    if (z > 0 && passable(at - plane) && !seen[at - plane]) stack.push(at - plane);
    if (z < nz - 1 && passable(at + plane) && !seen[at + plane]) stack.push(at + plane);
  }
  return reached;
}

// Six-connected components smaller than `share` of the largest are removed.
//
// Not "keep only the largest", which is the tempting version and the wrong one: a
// threshold routinely splits one subject into a few large pieces, and keeping only
// the biggest discarded half a head on two of the datasets here. What noise looks
// like is not "second largest" but "thousands of tiny ones", and that is what this
// removes. For a mask whose job is to exclude background from a fit the asymmetry is
// the point — a dropped piece of anatomy is lost data, while a surviving speck of
// background only costs the time to fit it.
export function dropSmallRegions(mask, dims, share = 0.02) {
  const label = new Int32Array(mask.length).fill(-1);
  const passable = (i) => mask[i] === 1;
  const sizes = [];
  const seen = new Uint8Array(mask.length);
  for (let i = 0; i < mask.length; i++) {
    if (mask[i] !== 1 || seen[i]) continue;
    const before = new Uint8Array(mask.length);
    const size = flood([i], passable, before, dims);
    const id = sizes.length;
    sizes.push(size);
    for (let j = 0; j < mask.length; j++) {
      if (before[j]) {
        label[j] = id;
        seen[j] = 1;
      }
    }
  }
  const biggest = sizes.reduce((a, b) => Math.max(a, b), 0);
  const out = new Uint8Array(mask.length);
  for (let i = 0; i < mask.length; i++) {
    if (label[i] >= 0 && sizes[label[i]] >= biggest * share) out[i] = 1;
  }
  return out;
}

// Background enclosed by the mask becomes mask. Without this, every voxel the
// threshold missed inside the subject — dark CSF, bone, a signal void — would be
// excluded from the fit, which is the opposite of what a mask is for.
//
// "Enclosed" means not reachable from outside, so the outside is what gets flooded
// and the holes are what is left over.
export function fillHoles(mask, dims) {
  const [nx, ny, nz] = dims;
  const passable = (i) => mask[i] === 0;
  const seeds = [];
  for (let z = 0; z < nz; z++) {
    for (let y = 0; y < ny; y++) {
      for (let x = 0; x < nx; x++) {
        const at = x + y * nx + z * nx * ny;
        if (mask[at] === 0 && onBorder(x, y, z, dims)) seeds.push(at);
      }
    }
  }
  const outside = new Uint8Array(mask.length);
  flood(seeds, passable, outside, dims);
  const out = Uint8Array.from(mask);
  for (let i = 0; i < out.length; i++) if (out[i] === 0 && !outside[i]) out[i] = 1;
  return out;
}

// The subject, from any image of one: blur away the speckle, split background off
// the histogram, drop what is only noise, close what the result encloses. Four steps
// with one tuned number between them — the 2% below which a component is noise —
// which is why it does not care what the contrast is or how many slices there are.
//
// It finds the *subject*, not the brain: skull, scalp and neck are part of what is
// above background. For excluding background from a fit that is exactly right, and
// for anatomy it is not a substitute for brain extraction.
export function foregroundMask(values, dims) {
  const smoothed = blurBox(values, dims, 1);
  const threshold = backgroundThreshold(smoothed);
  const above = new Uint8Array(values.length);
  for (let i = 0; i < smoothed.length; i++) {
    if (Number.isFinite(smoothed[i]) && smoothed[i] > threshold) above[i] = 1;
  }
  return fillHoles(dropSmallRegions(above, dims), dims);
}
