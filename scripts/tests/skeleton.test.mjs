// The voxel skeleton's diagonal wave depends on a panel's measured size, so
// the grid math (how many columns and rows, and what delay a cell at a given
// position gets) has to hold for whatever size a panel turns out to be —
// this covers a wide panel, a tall one, and the cap a pathologically large
// one hits, rather than restating the formulas verbatim.
import test from "node:test";
import assert from "node:assert/strict";
import { computeGrid, cellDelayMs, cellRampFraction, cellCentreInGlyph, cycleMs } from "../../docs/playground/skeleton.js";

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
  assert.equal(cellRampFraction(1, 2, 8, 8), cellRampFraction(2, 1, 8, 8));
});

test("the gradient spans the grid once, corner to corner, rather than repeating", () => {
  // The whole point of the fraction: the far corner is the end of the gradient
  // whatever the grid's size, so no colour recurs on the way there.
  for (const [cols, rows] of [[8, 8], [40, 6], [3, 27], [2, 2]]) {
    assert.equal(cellRampFraction(0, 0, cols, rows), 0, "starts at the near corner");
    assert.equal(cellRampFraction(rows - 1, cols - 1, cols, rows), 1, "ends at the far one");
    // Strictly increasing along the diagonal is what makes it one gradient; a
    // repeating ramp would come back down.
    let prev = -1;
    for (let d = 0; d <= (rows - 1) + (cols - 1); d++) {
      const row = Math.min(d, rows - 1);
      const f = cellRampFraction(row, d - row, cols, rows);
      assert.ok(f > prev, `fraction fell back from ${prev} to ${f}`);
      prev = f;
    }
  }
});

test("a single-cell grid has no diagonal to be partway along", () => {
  assert.equal(cellRampFraction(0, 0, 1, 1), 0);
});

test("the umbrella's box is centred on the panel and square on any aspect", () => {
  // A wide panel: the void must sit in the middle of the width, not at the left.
  const cols = 60, rows = 20, w = 1200, h = 400;
  const centre = cellCentreInGlyph(Math.floor(rows / 2), Math.floor(cols / 2), cols, rows, w, h);
  assert.ok(centre, "the middle cell is inside the glyph box");
  // The glyph is 24 units across, so its own centre is (12, 12).
  assert.ok(Math.abs(centre.x - 12) < 2, `x was ${centre.x}`);
  assert.ok(Math.abs(centre.y - 12) < 2, `y was ${centre.y}`);
  // Every corner is outside it, so the void never touches the panel's edge.
  for (const [r, c] of [[0, 0], [0, cols - 1], [rows - 1, 0], [rows - 1, cols - 1]]) {
    assert.equal(cellCentreInGlyph(r, c, cols, rows, w, h), null, `corner ${r},${c} is inside`);
  }
});

test("the umbrella's box stays square when the panel is tall rather than wide", () => {
  const inside = cellCentreInGlyph(30, 5, 10, 60, 300, 1800);
  assert.ok(inside, "the middle of a tall panel is inside the glyph box");
  assert.ok(inside.x >= 0 && inside.x <= 24 && inside.y >= 0 && inside.y <= 24);
});

test("a panel with no measured size has no void rather than a degenerate one", () => {
  assert.equal(cellCentreInGlyph(0, 0, 1, 1, 0, 0), null);
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
    // Guarded before the comparison below, which a zero cycle would satisfy
    // vacuously (0 >= 0) while describing an animation that never runs.
    assert.ok(cycle > 0, "every grid gets a cycle of real duration");
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
