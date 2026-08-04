// The voxel skeleton's diagonal wave depends on a panel's measured size, so
// the grid math (how many columns and rows, and what delay a cell at a given
// position gets) has to hold for whatever size a panel turns out to be —
// this covers a wide panel, a tall one, and the cap a pathologically large
// one hits, rather than restating the formulas verbatim.
import test from "node:test";
import assert from "node:assert/strict";
import { computeGrid, cellDelayMs, cellRampIndex, cycleMs } from "../../docs/playground/skeleton.js";

test("a wide panel tiles exactly: cols * cell width covers the full width with no gap", () => {
  const { cols, rows } = computeGrid(1200, 200, 16, 10000);
  assert.ok(cols > rows, "a wide panel has more columns than rows");
  // `1fr` tracks mean the grid always covers the measured box exactly; what
  // matters is that the cell count is close to the target size, not a fixed
  // pixel width that could leave a remainder strip uncovered.
  assert.ok(Math.abs(cols * 16 - 1200) <= 16);
  assert.ok(Math.abs(rows * 16 - 200) <= 16);
});

test("a tall panel tiles exactly: rows outnumber columns", () => {
  const { cols, rows } = computeGrid(200, 1200, 16, 10000);
  assert.ok(rows > cols, "a tall panel has more rows than columns");
  assert.ok(Math.abs(cols * 16 - 200) <= 16);
  assert.ok(Math.abs(rows * 16 - 1200) <= 16);
});

test("a very large panel stays under the cap by growing the cell size, not the node count", () => {
  const { cols, rows } = computeGrid(4000, 3000, 16, 900);
  assert.ok(cols * rows <= 900, "the grid never exceeds the configured cap");
  assert.ok(cols * rows > 0, "the cap never collapses the grid to nothing");
});

test("a zero-size panel (not yet laid out) still yields a usable grid", () => {
  const { cols, rows } = computeGrid(0, 0, 16);
  assert.ok(cols >= 1 && rows >= 1);
});

test("delay increases moving right along a row and moving down a column", () => {
  assert.ok(cellDelayMs(0, 1) > cellDelayMs(0, 0));
  assert.ok(cellDelayMs(1, 0) > cellDelayMs(0, 0));
  assert.ok(cellDelayMs(2, 2) > cellDelayMs(1, 1), "further down the diagonal waits longer");
});

test("two cells on the same diagonal (row + col equal) share a delay and a colour", () => {
  assert.equal(cellDelayMs(1, 2), cellDelayMs(2, 1));
  assert.equal(cellRampIndex(1, 2), cellRampIndex(2, 1));
});

test("the ramp index cycles rather than running off the nine-step ramp", () => {
  for (let row = 0; row < 5; row++) {
    for (let col = 0; col < 5; col++) {
      const idx = cellRampIndex(row, col);
      assert.ok(idx >= 0 && idx < 9, "always a valid --ripple-N index");
    }
  }
});

// The grid must fill whole before any of it clears. A cell lights `LIT_AT` into
// the cycle and holds until `CLEARS_AT`, both as shares of the cycle, matching
// the `ripple` keyframes in app.css. If the first cell's hold does not outlast
// the wave's traverse, the top left empties while the bottom right is still
// filling, which is the one thing this animation must not do. These numbers
// have to be kept in step with those keyframes by hand, so the test states
// them rather than deriving them.
const LIT_AT = 0.08;
const CLEARS_AT = 0.53;

for (const [label, cols, rows] of [
  ["a wide panel", 48, 12],
  ["a tall panel", 12, 48],
  ["a square panel", 30, 30],
  ["a single cell", 1, 1],
]) {
  test(`${label} fills completely before any cell clears`, () => {
    const cycle = cycleMs(cols, rows);
    const traverse = cellDelayMs(rows - 1, cols - 1);
    const lastLitAt = traverse + LIT_AT * cycle;
    const firstClearsAt = CLEARS_AT * cycle;
    assert.ok(
      firstClearsAt >= lastLitAt,
      `first cell clears at ${firstClearsAt}ms but the last lights at ${lastLitAt}ms`,
    );
  });
}

test("the cycle grows with the panel, since a longer traverse needs a longer hold", () => {
  assert.ok(cycleMs(48, 12) > cycleMs(12, 12), "a wider panel takes longer to cross");
  assert.ok(cycleMs(60, 60) > cycleMs(30, 30), "a bigger grid takes longer to cross");
});
