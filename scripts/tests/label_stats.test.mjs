import { test } from "node:test";
import assert from "node:assert/strict";
import { labelStats, describeValues } from "../../docs/playground/stats.js";

// A 2x2x1 map. C-order index is (x*ny + y)*nz + z, so with ny=2, nz=1:
//   (0,0)->0  (0,1)->1  (1,0)->2  (1,1)->3
// NiiVue's bitmap is x-fastest: index x + y*nx + z*nx*ny, so with nx=2:
//   (0,0)->0  (1,0)->1  (0,1)->2  (1,1)->3
// The two orders disagree for (0,1) and (1,0) — which is exactly the mistake
// these tests exist to catch.
const DIMS = [2, 2, 1];
const MAP = Float64Array.from([10, 20, 30, 40]); // C-order

test("a label picks up the voxels it actually covers, in the right order", () => {
  // Paint label 1 at (1,0) only: x-fastest index 1.
  const bitmap = Uint8Array.from([0, 1, 0, 0]);
  const stats = labelStats(bitmap, MAP, DIMS);
  assert.deepEqual([...stats.keys()], [1]);
  // (1,0) is C-order index 2 -> value 30. Reading the bitmap index straight into
  // the map would wrongly give 20.
  assert.equal(stats.get(1).n, 1);
  assert.equal(stats.get(1).mean, 30);
});

test("labels are kept apart", () => {
  const bitmap = Uint8Array.from([1, 1, 2, 2]);
  const stats = labelStats(bitmap, MAP, DIMS);
  assert.deepEqual([...stats.keys()].sort(), [1, 2]);
  assert.equal(stats.get(1).n, 2);
  assert.equal(stats.get(2).n, 2);
  // label 1 covers (0,0) and (1,0) -> C-order 0 and 2 -> 10 and 30
  assert.equal(stats.get(1).mean, 20);
  // label 2 covers (0,1) and (1,1) -> C-order 1 and 3 -> 20 and 40
  assert.equal(stats.get(2).mean, 30);
});

test("painting everything with one label equals the whole-map statistics", () => {
  const bitmap = Uint8Array.from([3, 3, 3, 3]);
  const stats = labelStats(bitmap, MAP, DIMS);
  const whole = describeValues([...MAP]);
  assert.deepEqual(stats.get(3), whole);
});

test("unfitted voxels are excluded, not counted as zero", () => {
  const withNaN = Float64Array.from([10, NaN, 30, 40]);
  const bitmap = Uint8Array.from([1, 1, 1, 1]);
  const stats = labelStats(bitmap, withNaN, DIMS);
  assert.equal(stats.get(1).n, 3);
  assert.equal(stats.get(1).mean, (10 + 30 + 40) / 3);
});

test("a label whose every voxel is unfitted is absent, not zero-valued", () => {
  const allNaN = Float64Array.from([NaN, NaN, NaN, NaN]);
  const stats = labelStats(Uint8Array.from([1, 1, 1, 1]), allNaN, DIMS);
  assert.equal(stats.size, 0);
});

test("a single-voxel label reports SD 0 rather than NaN", () => {
  const stats = labelStats(Uint8Array.from([1, 0, 0, 0]), MAP, DIMS);
  assert.equal(stats.get(1).n, 1);
  assert.equal(stats.get(1).sd, 0);
});

test("no bitmap means no labels", () => {
  assert.equal(labelStats(null, MAP, DIMS).size, 0);
});
