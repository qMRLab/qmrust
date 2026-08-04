// The voxel skeleton that stands in for a viewer's image while it loads: a
// grid of small cells filling the panel edge to edge, rippling in a
// diagonal wave. Both callers that reveal a skeleton — a dataset download
// filling `#skel-in`, a fit filling `#skel-out` — build the same shape, so
// the grid math lives in this one module instead of being duplicated per
// caller.
//
// The wave comes from each cell's delay depending on its row and column, so
// it cannot be expressed as a fixed set of CSS classes: the column count
// itself depends on the panel's width, which is only known once the panel
// is on screen. `fillSkeleton` measures the panel and builds exactly the
// cells that size calls for.
//
// The same measurement decides which cells are left out to cut the umbrella
// void from the grid's middle.

import { iconPaths } from "./vendor/icons.js";

// A cell this size reads as a voxel without needing per-panel tuning: large
// enough to be a distinct block rather than grain, small enough that a panel
// holds a grid rather than a handful of tiles.
const TARGET_CELL_PX = 22;

// Caps the DOM node count a very large panel would otherwise ask for: a 4K
// panel at the target size would be tens of thousands of divs. Rather than
// emit that many, the cell size grows (never past what the cap allows) until
// the grid fits under it.
const MAX_CELLS = 900;

// How much later each step along the diagonal ripples, in milliseconds. This
// sets how fast the wave front crosses the panel, which is the motion a reader
// actually perceives; the cell's own fade duration only sets how long one block
// stays lit as the front passes it.
const STEP_MS = 65;

// The umbrella cut out of the middle of the grid: a void in the voxels rather
// than a glyph drawn over them, so the panel reads as an image with a hole in
// its middle. Sized as a share of the panel's shorter side, which keeps the
// glyph square and centred whatever proportions the panel has.
const VOID_SHARE = 0.42;

// Lucide authors its glyphs in a 24-unit square.
const GLYPH_VIEWBOX = 24;

// The pen the glyph is hit-tested with, in those units. Lucide strokes at 2; a
// wider pen is what keeps the handle and the finial from thinning to nothing
// when the shape is sampled at one point per cell.
const VOID_PEN = 3.4;

// The share of one cycle a cell stays lit, matching the plateau between the
// `ripple` keyframes' fill and wipe edges in app.css. The two must agree: this
// is the number `cycleMs` sizes the cycle against, and the keyframes are where
// the plateau actually lives.
const PLATEAU_SHARE = 0.45;

// A margin on top of the minimum cycle, so the last cell to light is fully lit
// for a beat before the first one starts to clear rather than the two events
// landing on the same frame.
const PLATEAU_MARGIN = 1.15;

/**
 * The column and row count that tiles a `width` by `height` panel with
 * cells at or near `targetCellPx`, growing the cell size if that would
 * exceed `maxCells`. Always at least one column and one row, so a
 * not-yet-laid-out panel (zero measured size) still yields a usable grid.
 */
export function computeGrid(width, height, targetCellPx = TARGET_CELL_PX, maxCells = MAX_CELLS) {
  let size = Math.max(1, targetCellPx);
  let cols = Math.max(1, Math.round(width / size));
  let rows = Math.max(1, Math.round(height / size));
  while (cols * rows > maxCells) {
    size *= 1.25;
    cols = Math.max(1, Math.round(width / size));
    rows = Math.max(1, Math.round(height / size));
  }
  return { cols, rows };
}

/** The diagonal step a cell at (row, col) sits on: the wave's own clock. */
function diagonalIndex(row, col) {
  return row + col;
}

/** How long a cell at (row, col) waits before its ripple starts. */
export function cellDelayMs(row, col, stepMs = STEP_MS) {
  return diagonalIndex(row, col) * stepMs;
}

/**
 * How far along the wave's path a cell at (row, col) sits, from 0 in the corner
 * the sweep starts from to 1 in the corner it ends at.
 *
 * The colour is mixed from this, so the gradient crosses the panel once instead
 * of repeating: the fraction is against the grid's own longest diagonal, which
 * is why it needs the grid's size rather than only the cell's position. A
 * single-cell grid has no diagonal to be partway along, so it sits at the start.
 */
export function cellRampFraction(row, col, cols, rows) {
  const last = diagonalIndex(rows - 1, cols - 1);
  return last > 0 ? diagonalIndex(row, col) / last : 0;
}

/**
 * Where a cell's centre falls in the glyph's own 24-unit space, or `null` when
 * the cell lies outside the glyph's box altogether.
 *
 * Only the mapping lives here, not the hit test: the shape's own geometry needs
 * `Path2D`, which a test environment has no reason to provide, while whether the
 * box is centred and square is exactly what can go wrong silently.
 */
export function cellCentreInGlyph(row, col, cols, rows, width, height) {
  const side = Math.min(width, height) * VOID_SHARE;
  if (!(side > 0)) return null;
  const x = ((col + 0.5) / cols) * width - (width - side) / 2;
  const y = ((row + 0.5) / rows) * height - (height - side) / 2;
  if (x < 0 || y < 0 || x > side || y > side) return null;
  const scale = side / GLYPH_VIEWBOX;
  return { x: x / scale, y: y / scale };
}

/**
 * A predicate answering whether a point in glyph space is inside the umbrella,
 * or `null` where the platform cannot say.
 *
 * The glyph's paths come from the vendored icon set, so the void is the same
 * artwork the rest of the interface draws rather than a second copy of it. Its
 * closed subpath (the canopy) is tested filled and every subpath is tested
 * stroked, which is what gives a solid dome with a handle attached rather than
 * a hollow outline. Where `Path2D` is missing the grid simply builds whole: a
 * decorative void is not worth failing a loading state over.
 */
function umbrellaHitTest() {
  if (typeof Path2D === "undefined" || typeof document === "undefined") return null;
  const ctx = document.createElement("canvas").getContext("2d");
  if (!ctx) return null;
  const paths = iconPaths("umbrella").map((d) => new Path2D(d));
  // Glyph coordinates are passed straight through, so the pen is in those units
  // and no transform is needed.
  ctx.lineWidth = VOID_PEN;
  return (x, y) => paths.some((p) => ctx.isPointInPath(p, x, y) || ctx.isPointInStroke(p, x, y));
}

/**
 * How long one cycle must run so that no cell begins to clear before every
 * cell has lit.
 *
 * A cell lights `delay` into the cycle and holds for `PLATEAU_SHARE` of it.
 * The last cell's delay is the wave's full traverse, so the first cell's
 * plateau has to cover that traverse or the grid starts emptying from the top
 * left while the bottom right is still filling. The traverse grows with the
 * panel, so the cycle cannot be a constant in the stylesheet and is set per
 * panel instead.
 */
export function cycleMs(cols, rows, stepMs = STEP_MS, plateauShare = PLATEAU_SHARE) {
  // A single-cell grid — what a panel measured before layout falls back to —
  // has a zero-length traverse, and a zero-duration animation does not run at
  // all, leaving the cell transparent forever. One step is the floor, so every
  // grid gets a cycle that is a real duration.
  const traverseMs = Math.max(stepMs, cellDelayMs(rows - 1, cols - 1, stepMs));
  return Math.round((traverseMs / plateauShare) * PLATEAU_MARGIN);
}

/**
 * Fills `skel`'s `.loader` with a grid of cells sized to the panel's own
 * measured area, each coloured and delayed by its position on the diagonal.
 * Call this after the skeleton's `hidden` attribute is cleared (so its size
 * is real) but before the caller yields, so nothing paints an empty grid.
 */
export function fillSkeleton(skel) {
  const loader = skel.querySelector(".loader");
  const { width, height } = skel.getBoundingClientRect();
  const { cols, rows } = computeGrid(width || TARGET_CELL_PX, height || TARGET_CELL_PX);
  loader.style.gridTemplateColumns = `repeat(${cols}, 1fr)`;
  loader.style.gridTemplateRows = `repeat(${rows}, 1fr)`;
  // One duration for every cell in this panel: the cells differ only by delay,
  // and the plateau that keeps the grid whole is a share of this.
  loader.style.setProperty("--cycle", `${cycleMs(cols, rows)}ms`);
  loader.replaceChildren();
  const inUmbrella = umbrellaHitTest();
  for (let row = 0; row < rows; row++) {
    for (let col = 0; col < cols; col++) {
      const at = inUmbrella && cellCentreInGlyph(row, col, cols, rows, width, height);
      if (at && inUmbrella(at.x, at.y)) {
        // A plain div, holding the cell's place in the grid so the voxels around
        // the void stay on their tracks, but carrying nothing that paints. The
        // void is the absence of a cell, not a cell coloured to look absent.
        loader.append(document.createElement("div"));
        continue;
      }
      const cell = document.createElement("div");
      cell.className = "cell";
      cell.style.setProperty("--mix", `${cellRampFraction(row, col, cols, rows) * 100}%`);
      cell.style.animationDelay = `${cellDelayMs(row, col)}ms`;
      loader.append(cell);
    }
  }
}
