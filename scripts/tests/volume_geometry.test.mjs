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
