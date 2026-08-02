// The index arithmetic behind brain extraction: a crop that reverses the axes on
// the way in, and a place-back that must undo it exactly. A network that reads one
// voxel while a mask is written at another is silently wrong everywhere, and no
// picture of a brain makes that visible — so it is checked as an identity here.
import test from "node:test";
import assert from "node:assert/strict";
import {
  argmaxChannels,
  boundingBox,
  cropReversed,
  backgroundThreshold,
  blurBox,
  dropSmallRegions,
  fillHoles,
  foregroundMask,
  growMask,
  normalizeToWhiteMatter,
  placeReversed,
} from "../../docs/playground/volume.js";

const dims = [7, 5, 4];

// A volume whose value *is* its storage index, so a misplaced voxel names the
// place it came from.
function ramp([nx, ny, nz]) {
  return Float32Array.from({ length: nx * ny * nz }, (_, i) => i + 1);
}

// The network is trained behind an intensity normalisation, and gets this wrong
// silently: a volume scaled elsewhere returns almost no brain rather than an error.
// So what is asserted is *where tissue lands*, not merely that the range is [0, 1].
const WHITE_MATTER = 110 / 255;

// Background, then a grey-matter peak, then a brighter and more populous
// white-matter peak — so the mode of the brighter half is unambiguously the latter.
function twoPeaks(grey, white) {
  return Float32Array.from([
    ...Array(1000).fill(0),
    ...Array(400).fill(grey),
    ...Array(600).fill(white),
  ]);
}

test("normalizeToWhiteMatter puts the bright tissue mode at 110/255", () => {
  const out = normalizeToWhiteMatter(twoPeaks(100, 200));
  assert.ok(Math.abs(out.at(-1) - WHITE_MATTER) < 0.005, `white matter at ${out.at(-1)}`);
  // Grey matter keeps its ratio to it: this is a scale, not a remapping.
  assert.ok(Math.abs(out[1000] - WHITE_MATTER / 2) < 0.005, `grey matter at ${out[1000]}`);
  assert.equal(out[0], 0);
});

test("the scale follows the data, not the numbers' size", () => {
  // The same anatomy in different units must reach the network identically.
  const small = normalizeToWhiteMatter(twoPeaks(1, 2));
  const large = normalizeToWhiteMatter(twoPeaks(1e5, 2e5));
  assert.ok(Math.abs(small.at(-1) - large.at(-1)) < 0.005);
  assert.ok(Math.abs(small.at(-1) - WHITE_MATTER) < 0.005);
});

test("anything brighter than the window clips rather than scaling everything down", () => {
  const values = twoPeaks(100, 200);
  const out = normalizeToWhiteMatter(
    Float32Array.from([...values, 800]), // a bright vessel, four times white matter
  );
  assert.equal(out.at(-1), 1);
  // The mode is unmoved by it — which is the whole point of using one.
  assert.ok(Math.abs(out.at(-2) - WHITE_MATTER) < 0.02, `white matter at ${out.at(-2)}`);
});

test("an empty volume normalises to zeros rather than NaN", () => {
  assert.deepEqual([...normalizeToWhiteMatter(Float32Array.from([0, 0, 0]))], [0, 0, 0]);
});

test("boundingBox finds the box above threshold and grows it by the padding", () => {
  const values = new Float32Array(7 * 5 * 4);
  const at = (x, y, z) => x + y * 7 + z * 35;
  values[at(3, 2, 1)] = 1;
  values[at(4, 3, 2)] = 1;
  assert.deepEqual(boundingBox(values, dims, 0.5, 0), { lo: [3, 2, 1], size: [2, 2, 2] });
  assert.deepEqual(boundingBox(values, dims, 0.5, 1), { lo: [2, 1, 0], size: [4, 4, 4] });
});

test("padding stops at the volume's edges", () => {
  const values = new Float32Array(7 * 5 * 4);
  values[0] = 1;
  assert.deepEqual(boundingBox(values, dims, 0.5, 99), { lo: [0, 0, 0], size: [7, 5, 4] });
});

test("an empty volume yields the whole grid, not a zero-sized box", () => {
  const values = new Float32Array(7 * 5 * 4);
  assert.deepEqual(boundingBox(values, dims, 0.5, 3), { lo: [0, 0, 0], size: [7, 5, 4] });
});

// The load-bearing test. A round-trip through crop and place-back is symmetric
// under *any* consistent index convention, including a wrong one — so what has to
// be asserted is the layout itself: the buffer is read by a tensor library as
// row-major of shape [sz, sy, sx], and its (i, j, k) must be the box's (k, j, i).
// Deliberately non-cubic extents, since every convention agrees when they match.
test("cropReversed is a row-major [sz,sy,sx] tensor of the box transposed", () => {
  const values = ramp(dims);
  const lo = [1, 1, 1];
  const size = [4, 3, 2]; // sx, sy, sz — all different
  const [sx, sy, sz] = size;
  const out = cropReversed(values, dims, lo, size);
  assert.equal(out.length, sx * sy * sz);
  for (let i = 0; i < sz; i++) {
    for (let j = 0; j < sy; j++) {
      for (let k = 0; k < sx; k++) {
        // Row-major flat index for shape [sz, sy, sx]: last axis varies fastest.
        const flat = (i * sy + j) * sx + k;
        const source = lo[0] + k + (lo[1] + j) * dims[0] + (lo[2] + i) * dims[0] * dims[1];
        assert.equal(out[flat], values[source], `at (${i},${j},${k})`);
      }
    }
  }
});

// The same statement from the other side: whatever the network returns is read
// back under the same strides it was fed under.
test("placeReversed reads labels as row-major [sz,sy,sx]", () => {
  const size = [4, 3, 2];
  const [sx, sy, sz] = size;
  const lo = [2, 1, 1];
  const labels = Uint8Array.from({ length: sx * sy * sz }, (_, i) => i + 1);
  const placed = placeReversed(labels, dims, lo, size);
  for (let i = 0; i < sz; i++) {
    for (let j = 0; j < sy; j++) {
      for (let k = 0; k < sx; k++) {
        const at = lo[0] + k + (lo[1] + j) * dims[0] + (lo[2] + i) * dims[0] * dims[1];
        assert.equal(placed[at], labels[(i * sy + j) * sx + k], `at (${i},${j},${k})`);
      }
    }
  }
});

test("placeReversed puts every cropped voxel back where it came from", () => {
  const values = ramp(dims);
  const lo = [2, 1, 0];
  const size = [4, 3, 3];
  // Labels standing in for the network's output: one per cropped voxel, in the
  // reversed order the crop produced.
  const cropped = cropReversed(values, dims, lo, size);
  const labels = Uint8Array.from(cropped, (v) => v % 251);
  const placed = placeReversed(labels, dims, lo, size);

  assert.equal(placed.length, values.length);
  let checked = 0;
  for (let x = 0; x < dims[0]; x++) {
    for (let y = 0; y < dims[1]; y++) {
      for (let z = 0; z < dims[2]; z++) {
        const at = x + y * dims[0] + z * dims[0] * dims[1];
        const inside =
          x >= lo[0] && x < lo[0] + size[0] &&
          y >= lo[1] && y < lo[1] + size[1] &&
          z >= lo[2] && z < lo[2] + size[2];
        assert.equal(placed[at], inside ? values[at] % 251 : 0);
        if (inside) checked++;
      }
    }
  }
  assert.equal(checked, size[0] * size[1] * size[2]);
});

test("argmaxChannels picks the winning class per voxel", () => {
  //            voxel 0            voxel 1            voxel 2
  const scores = [0.1, 0.9, 0.0, 5.0, 1.0, 2.0, -1, -3, -0.5];
  assert.deepEqual([...argmaxChannels(scores, 3)], [1, 0, 2]);
});

// The classical route. Its whole claim is that it needs no parameter and no
// assumption about the contrast or the slice count, so that is what is asserted:
// the same shape is found whether it is bright-on-dark or dark-on-bright, and
// whether it is one slice or many.
test("backgroundThreshold splits background off a two-class histogram", () => {
  const values = Float32Array.from([...Array(500).fill(10), ...Array(500).fill(200)]);
  const t = backgroundThreshold(values);
  assert.ok(t > 10 && t < 200, `threshold ${t}`);
});

// The case that decided the design. Two-class Otsu on this isolates the bright class
// and puts tissue on the background side of the split — which discards the anatomy.
test("backgroundThreshold splits below tissue, not below the bright class", () => {
  const values = Float32Array.from([
    ...Array(5000).fill(5),    // background
    ...Array(3000).fill(200),  // tissue
    ...Array(1500).fill(2000), // CSF, a bright rim, a spike
  ]);
  const t = backgroundThreshold(values);
  assert.ok(t > 5 && t < 200, `threshold ${t} should fall between background and tissue`);
});

test("backgroundThreshold survives an image with one value", () => {
  assert.equal(backgroundThreshold(Float32Array.from([7, 7, 7])), 7);
});

test("blurBox averages a neighbourhood and skips an axis with no depth", () => {
  // A single bright voxel in a 5x5 slice spreads over its 3x3 neighbourhood only.
  const values = new Float32Array(25);
  values[12] = 9; // centre of a 5x5
  const out = blurBox(values, [5, 5, 1], 1);
  assert.ok(out[12] > 0 && out[12] < 9, "the peak is averaged down");
  assert.ok(out[11] > 0, "and its neighbour picks some up");
  assert.equal(out[0], 0, "but a voxel two away does not");
  // Blurring along z would be blurring along nothing, and must not divide by more
  // than it summed.
  assert.equal(out.reduce((a, b) => a + b, 0).toFixed(4), (9).toFixed(4));
});

test("dropSmallRegions keeps every big piece and removes only the specks", () => {
  const dims = [12, 6, 1];
  const mask = new Uint8Array(72);
  const at = (x, y) => x + y * 12;
  // Two separate large blobs — one subject split by a threshold — plus two specks.
  for (let x = 1; x < 4; x++) for (let y = 1; y < 5; y++) mask[at(x, y)] = 1;  // 12
  for (let x = 7; x < 10; x++) for (let y = 1; y < 4; y++) mask[at(x, y)] = 1; // 9
  mask[at(0, 0)] = 1;
  mask[at(11, 5)] = 1;
  const out = dropSmallRegions(mask, dims, 0.5);
  assert.equal(out.reduce((n, v) => n + v, 0), 12 + 9, "both blobs survive");
  assert.equal(out[at(0, 0)], 0, "the specks do not");
  assert.equal(out[at(11, 5)], 0);
});

test("fillHoles closes what the mask encloses and leaves the outside alone", () => {
  const dims = [7, 7, 1];
  const mask = new Uint8Array(49);
  const at = (x, y) => x + y * 7;
  // A ring with a one-voxel hole at its centre.
  for (let x = 2; x <= 4; x++) for (let y = 2; y <= 4; y++) mask[at(x, y)] = 1;
  mask[at(3, 3)] = 0;
  const out = fillHoles(mask, dims);
  assert.equal(out[at(3, 3)], 1, "the enclosed voxel is filled");
  assert.equal(out[at(0, 0)], 0, "the outside is untouched");
});

// The single-slice case is the one a naive border test gets wrong: with nz === 1
// every voxel is on a z face, so seeding the flood from "the border" would seed the
// whole slice and nothing could ever be enclosed.
test("holes are still holes in a single slice", () => {
  const mask = new Uint8Array(25);
  const at = (x, y) => x + y * 5;
  for (let x = 1; x <= 3; x++) for (let y = 1; y <= 3; y++) mask[at(x, y)] = 1;
  mask[at(2, 2)] = 0;
  assert.equal(fillHoles(mask, [5, 5, 1])[at(2, 2)], 1);
});

// A block of signal in a dim noisy field, with a brighter shell around it — the
// three-class histogram real images have.
function subject(level, [nx, ny, nz]) {
  const v = new Float32Array(nx * ny * nz);
  for (let i = 0; i < v.length; i++) v[i] = (i % 5) * 0.02 * level;
  for (let z = 2; z < nz - 2; z++) {
    for (let y = 3; y < ny - 3; y++) {
      for (let x = 3; x < nx - 3; x++) {
        const rim = x === 3 || x === nx - 4 || y === 3 || y === ny - 4;
        v[x + y * nx + z * nx * ny] = rim ? level * 8 : level;
      }
    }
  }
  return v;
}

// What a mask for fitting has to get right, asserted directly rather than through a
// voxel count: it must contain the whole subject, because a missed voxel is data
// thrown away, and it must not have swallowed the volume, because then it is masking
// nothing. The blur leaves a voxel of halo, which is why the count itself is not the
// assertion.
function inside(x, y, z, [nx, ny, nz]) {
  return z >= 2 && z < nz - 2 && y >= 3 && y < ny - 3 && x >= 3 && x < nx - 3;
}

test("foregroundMask contains the whole subject and nothing like the whole volume", () => {
  const dims = [16, 16, 7];
  const [nx, ny, nz] = dims;
  const mask = foregroundMask(subject(20, dims), dims);
  let missed = 0;
  for (let z = 0; z < nz; z++) {
    for (let y = 0; y < ny; y++) {
      for (let x = 0; x < nx; x++) {
        if (inside(x, y, z, dims) && !mask[x + y * nx + z * nx * ny]) missed++;
      }
    }
  }
  assert.equal(missed, 0, "every voxel of the subject is masked");
  const found = mask.reduce((n, v) => n + v, 0);
  assert.ok(found < mask.length * 0.75, `${found} of ${mask.length} is not a mask`);
  assert.equal(mask[0], 0, "the far corner is still background");
});

test("the same object at any brightness gives the identical mask", () => {
  const dims = [16, 16, 7];
  assert.deepEqual(
    [...foregroundMask(subject(20, dims), dims)],
    [...foregroundMask(subject(2000, dims), dims)],
  );
});

test("foregroundMask works on a single slice with nothing changed", () => {
  const dims = [16, 16, 1];
  const v = new Float32Array(256);
  for (let i = 0; i < v.length; i++) v[i] = (i % 5) * 0.4;
  for (let y = 3; y < 13; y++) {
    for (let x = 3; x < 13; x++) {
      const rim = x === 3 || x === 12 || y === 3 || y === 12;
      v[x + y * 16] = rim ? 160 : 20;
    }
  }
  const mask = foregroundMask(v, dims);
  for (let y = 3; y < 13; y++) {
    for (let x = 3; x < 13; x++) assert.equal(mask[x + y * 16], 1, `missed (${x},${y})`);
  }
  assert.ok(mask.reduce((n, b) => n + b, 0) < 256 * 0.75);
  assert.equal(mask[0], 0);
});

// Shrink/grow on a segmented mask. The single-slice cases are the ones that
// decide the implementation: four of the five example datasets are one slice
// thick, and a 6-neighbour pass that counts the absent z direction would find
// every voxel short of a neighbour and erase the mask on the first shrink.
const at3 = ([nx, ny], x, y, z) => x + y * nx + z * nx * ny;

test("growing a single voxel reaches its faces, not its corners", () => {
  const d = [5, 5, 5];
  const seed = new Uint8Array(125);
  seed[at3(d, 2, 2, 2)] = 1;
  const grown = growMask(seed, d, 1);
  // Itself plus one neighbour across each of six faces.
  assert.equal([...grown].reduce((a, b) => a + b, 0), 7);
  assert.equal(grown[at3(d, 3, 2, 2)], 1);
  // A diagonal is further than a voxel away, so it must stay out.
  assert.equal(grown[at3(d, 3, 3, 2)], 0);
});

test("a single-slice mask grows in plane and not through the depth it lacks", () => {
  const d = [5, 5, 1];
  const seed = new Uint8Array(25);
  seed[at3(d, 2, 2, 0)] = 1;
  const grown = growMask(seed, d, 1);
  // Four in-plane neighbours plus itself — never the six of a 3D pass.
  assert.equal([...grown].reduce((a, b) => a + b, 0), 5);
});

test("shrinking a single-slice mask erodes its rim rather than erasing it", () => {
  // The regression this guards: counting the absent z axis makes every voxel
  // look like it is missing a neighbour, and one shrink empties the mask.
  const d = [6, 6, 1];
  const solid = new Uint8Array(36).fill(1);
  const shrunk = growMask(solid, d, -1);
  // Outside the volume counts as inside, so a mask running to the edge keeps
  // its border: a full slice survives untouched.
  assert.equal([...shrunk].reduce((a, b) => a + b, 0), 36);

  // An interior blob does lose its rim: a 4x4 square becomes 2x2.
  const blob = new Uint8Array(36);
  for (let x = 1; x <= 4; x++) for (let y = 1; y <= 4; y++) blob[at3(d, x, y, 0)] = 1;
  assert.equal([...growMask(blob, d, -1)].reduce((a, b) => a + b, 0), 4);
});

test("zero steps is a copy, and the caller's buffer is never mutated", () => {
  const d = [4, 4, 1];
  const seed = new Uint8Array(16);
  seed[at3(d, 1, 1, 0)] = 1;
  const same = growMask(seed, d, 0);
  assert.deepEqual([...same], [...seed]);
  growMask(seed, d, 2);
  assert.equal([...seed].reduce((a, b) => a + b, 0), 1, "input was mutated");
});

test("the fit's own layout grows the same mask as storage order", () => {
  // The stepper applies this to `app.computedMask`, which is indexed
  // (x*ny + y)*nz + z rather than this module's x + y*nx + z*nx*ny. Same
  // geometry, different arithmetic: the two must agree voxel for voxel.
  const d = [4, 3, 2];
  const [nx, ny, nz] = d;
  const storage = new Uint8Array(nx * ny * nz);
  const fit = new Uint8Array(nx * ny * nz);
  storage[at3(d, 1, 1, 0)] = 1;
  fit[(1 * ny + 1) * nz + 0] = 1;

  const grownStorage = growMask(storage, d, 1);
  const grownFit = growMask(fit, d, 1, [ny * nz, nz, 1]);
  for (let x = 0; x < nx; x++) {
    for (let y = 0; y < ny; y++) {
      for (let z = 0; z < nz; z++) {
        assert.equal(
          grownFit[(x * ny + y) * nz + z],
          grownStorage[at3(d, x, y, z)],
          `disagreement at ${x},${y},${z}`,
        );
      }
    }
  }
});
