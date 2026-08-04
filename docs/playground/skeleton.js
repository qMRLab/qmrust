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

// The colour ramp's step count, `--ripple-1` through `--ripple-9`.
const RAMP_STEPS = 9;

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

/** Which of the ramp's steps a cell at (row, col) takes its colour from. */
export function cellRampIndex(row, col, steps = RAMP_STEPS) {
  return diagonalIndex(row, col) % steps;
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
  for (let row = 0; row < rows; row++) {
    for (let col = 0; col < cols; col++) {
      const cell = document.createElement("div");
      cell.className = "cell";
      cell.style.setProperty("--cell-color", `var(--ripple-${cellRampIndex(row, col) + 1})`);
      cell.style.animationDelay = `${cellDelayMs(row, col)}ms`;
      loader.append(cell);
    }
  }
}
