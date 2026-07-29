// Drawing on the map: the tool palette, the labels, and one drawing shared by
// both viewers.
//
// Manual tools only. The vendored NiiVue also offers Otsu, grow-cut and
// click-to-segment, and they are deliberately absent: a tolerance-based region
// silently includes or drops voxels, which biases a mean that then gets quoted.
// Every region here is traceable to a deliberate gesture.
import { DRAG_MODE } from "./vendor/niivue.js";
import { icon } from "./vendor/icons.js";
import { $, status } from "./dom.js";
import { app, nvIn, nvOut } from "./state.js";

// The labels available to paint with. The first three are the brand metals —
// verdigris, brass, copper — so a row in the measurements table and a region on
// the map are obviously the same object.
//
// A label is one *value* with one colour: the drawing stores the value, and the
// colour is only how NiiVue paints it. So a colour cannot be edited without
// repainting every voxel already drawn with that label — which is why choosing a
// fresh colour allocates a label rather than recolouring one. The list grows on
// demand; NiiVue's draw LUT holds 255.
export const LABELS = [
  { value: 1, color: "#7cc3b5" },
  { value: 2, color: "#c9a86a" },
  { value: 3, color: "#cf8a5f" },
  { value: 4, color: "#8fc39a" },
  { value: 5, color: "#8ab0d6" },
  { value: 6, color: "#d76a4c" },
];

// The freehand tools hand the stroke to NiiVue. The shapes do not: NiiVue anchors
// a rectangle at the corner where the drag began, which puts the shape beside the
// pointer rather than under it. Stamping them ourselves keeps them centred on the
// cursor, at the size the stepper says, which is also what the cursor previews.
//
// The wand and the fill are NiiVue's own region growers, seeded from the clicked
// voxel. Both are bounded to the current slice: everything else here is
// slice-aware, and a wand that silently ran through the volume would report a
// region far larger than the one a reader thinks they selected.
const TOOLS = [
  { id: "pen", icon: "pencil", title: "Freehand", setting: "size",
    hint: "Drag on the image to paint." },
  { id: "rect", icon: "square", title: "Rectangle", stamp: "rect", setting: "size",
    hint: "Click to stamp a square, centred on the cursor." },
  { id: "ellipse", icon: "circle", title: "Ellipse", stamp: "ellipse", setting: "size",
    hint: "Click to stamp a circle, centred on the cursor." },
  { id: "wand", icon: "wand", title: "Magic wand", wand: true, setting: "tolerance",
    hint: "Click a voxel to take everything within ±tolerance of its value. Mouse-wheel over the map regrows it live." },
  { id: "fill", icon: "cloud-rain", title: "Flood fill", fill: true,
    hint: "Click inside an outline you have drawn to fill it." },
  { id: "erase", icon: "eraser", title: "Erase", erase: true, setting: "size",
    hint: "Drag to remove. The dashed trail shows what you swept." },
  { id: "otsu", icon: "fold-vertical", title: "Otsu threshold", whole: true, setting: "levels",
    hint: "Splits the whole slice into classes by value." },
  { id: "growcut", icon: "bean", title: "Grow-cut", whole: true,
    hint: "Expands the regions you have drawn until they meet. Draw two labels first." },
];

let tool = TOOLS[0];
let label = 1;

// Every tool's adjustable quantity, in one place. Each tool names the one it uses,
// so the palette shows exactly one stepper and it always belongs to the tool in
// hand — rather than hiding options behind a gesture nobody would find.
const SETTINGS = {
  size: { value: 1, min: 1, max: 15, unit: "", title: "Brush size, in voxels" },
  // NiiVue grows the region to voxels within ±p of the clicked voxel's own value,
  // where p is `clickToSegmentPercent` — a fraction clamped to [0, 1], so the
  // stored percentage is divided by 100 on the way in. It is read *only* when
  // `clickToSegmentAutoIntensity` is on; with that off, NiiVue uses the fixed
  // `clickToSegmentIntensityMin/Max` instead and the tolerance does nothing.
  tolerance: { value: 5, min: 1, max: 100, unit: "%", title: "How far from the clicked value the wand may grow" },
  levels: { value: 2, min: 2, max: 5, unit: " classes", title: "How many classes to split the map into" },
};

const penSize = () => SETTINGS.size.value;
// Assigning `drawBitmap` fires `onDrawingChanged` on the receiving instance,
// which would mirror straight back; this flag is what stops the ping-pong.
let mirroring = false;

// NiiVue's own default; restored when labels are shown again.
const DRAW_OPACITY = 0.8;
let labelsVisible = true;

// Hiding is opacity, not deletion: a reader needs to see the map under a region
// they have just drawn, and must not have to erase it to do so.
function toggleLabels() {
  labelsVisible = !labelsVisible;
  for (const nv of drawables()) nv.setDrawOpacity(labelsVisible ? DRAW_OPACITY : 0);
  syncLabelToggle();
}

function syncLabelToggle() {
  const btn = $("labels-toggle");
  if (!btn) return;
  btn.innerHTML = icon(labelsVisible ? "eye" : "eye-closed", 16);
  btn.title = labelsVisible ? "Hide labels" : "Show labels";
  btn.classList.toggle("active", !labelsVisible);
}

// True while the wheel belongs to the wand rather than to frame stepping. The
// inputs viewer intercepts the wheel in capture phase to scrub frames, which
// would otherwise stop the gesture ever reaching NiiVue there.
export function wandOwnsWheel() {
  return Boolean(app.roiDrawing && tool.wand);
}

export function isDrawing() {
  return app.roiDrawing;
}

function currentLabel() {
  return label;
}

function activeColor() {
  return LABELS.find((l) => l.value === label).color;
}

// The pointer shows where paint will land, and how much of it.
//
// The freehand pen gets a plain cross centred on the painted voxel: a pencil
// glyph would have to trace from its tip, and a tip hotspot is far harder to aim
// than a centre. The stamps get their own outline instead, so the shape and its
// size are visible before committing. Both scale with the brush.
//
// The scale is proportional, not a true footprint: a voxel's size on screen
// depends on the current zoom, which the cursor cannot know. Browsers also cap
// cursor images, so the box stays small.
function cursorFor(t, size) {
  const box = Math.min(64, 16 + size * 3);
  const c = box / 2;
  const r = Math.max(3, (box - 6) / 2);
  const arm = Math.max(4, r);
  let shapes;
  if (t.stamp === "rect") {
    shapes = `<rect x="${c - r}" y="${c - r}" width="${r * 2}" height="${r * 2}"/>`;
  } else if (t.stamp === "ellipse") {
    shapes = `<circle cx="${c}" cy="${c}" r="${r}"/>`;
  } else {
    // Freehand and erase: a cross, with erase ringed so the two are not confused.
    shapes =
      `<line x1="${c - arm}" y1="${c}" x2="${c + arm}" y2="${c}"/>` +
      `<line x1="${c}" y1="${c - arm}" x2="${c}" y2="${c + arm}"/>` +
      "";
  }
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" width="${box}" height="${box}" ` +
    `viewBox="0 0 ${box} ${box}" fill="none" stroke-linecap="round">` +
    `<g stroke="#000" stroke-opacity=".6" stroke-width="3">${shapes}</g>` +
    `<g stroke="#fff" stroke-width="1.3">${shapes}</g></svg>`;
  // Hotspot at the centre, which is the voxel that gets painted.
  return `url("data:image/svg+xml,${encodeURIComponent(svg)}") ${Math.round(c)} ${Math.round(c)}, crosshair`;
}

// Both viewers accept the pen, so the same gesture works wherever the structure
// is easier to see. Drawing and drag-to-window are the same gesture, so one must
// yield while the pen is active — and the crosshair goes away, because it sits
// exactly where you are trying to draw.
// Every viewer that currently exists. The modal's instance is created on first
// open, so this has to be asked for each time rather than captured once.
function drawables() {
  return [nvIn, nvOut, app.nvModal].filter(Boolean);
}

function applyMode() {
  // Before any stroke, so a mark made in one viewer has somewhere to land in the
  // others.
  if (app.roiDrawing) ensureDrawing();
  for (const nv of drawables()) {
    nv.setDrawingEnabled(app.roiDrawing && !tool.stamp && !tool.fill && !tool.whole);
    nv.opts.dragMode = app.roiDrawing ? DRAG_MODE.none : DRAG_MODE.contrast;
    nv.setCrosshairWidth(app.roiDrawing ? 0 : 1);
  }
  applyTool();
}

function applyTool() {
  const cursor = app.roiDrawing && !tool.whole ? cursorFor(tool, penSize()) : "";
  for (const nv of drawables()) {
    // A tool that seeds from a click must not also let NiiVue's pen trail marks
    // behind it, so those tools take drawing off and handle the click themselves.
    nv.setDrawingEnabled(app.roiDrawing && !tool.stamp && !tool.fill && !tool.whole);
    nv.opts.penType = 0;
    nv.opts.penSize = penSize();
    nv.opts.clickToSegment = Boolean(app.roiDrawing && tool.wand);
    nv.opts.clickToSegmentPercent = SETTINGS.tolerance.value / 100;
    // Without this the tolerance above is never consulted.
    nv.opts.clickToSegmentAutoIntensity = true;
    // Grow within the slice being looked at, not through the volume.
    nv.opts.clickToSegmentIs2D = true;
    nv.setPenValue(tool.erase ? 0 : label, true);
    nv.canvas.style.cursor = cursor;
  }
}

// One logical drawing, visible in both viewers: draw on anatomy where structure
// is clearer, measure on the map. Both render the same `current.meta.dims`, so
// this is bookkeeping — no resampling and no geometry of ours.
function mirrorDrawing(from) {
  if (mirroring) return;
  // The wand grows into a buffer of its own, and NiiVue only commits that into
  // `drawBitmap` when the interaction ends — so mid-grow the committed bitmap is
  // still the old region. Mirroring whatever the source is *rendering* keeps the
  // other viewers in step, and leaves NiiVue's own commit alone: forcing it early
  // would make scroll-to-shrink accumulate instead of shrink, since each grow
  // re-seeds from the committed state.
  const live = from.clickToSegmentIsGrowing
    ? from.clickToSegmentGrowingBitmap ?? from.drawBitmap
    : from.drawBitmap;
  if (!live) return;
  mirroring = true;
  try {
    for (const to of drawables()) {
      if (to === from) continue;
      // A viewer with no drawing of its own has no texture to render one, so it
      // must be given both before a bitmap means anything to it.
      if (!to.drawBitmap) to.createEmptyDrawing();
      // The file previewer shares this instance and shows a volume of its own
      // shape; a bitmap sized for the map does not belong to it.
      if (to.drawBitmap?.length !== live.length) continue;
      // A copy, not the same array: the source's growing buffer is rewritten on
      // every wheel tick, and the receivers must not alias it.
      to.drawBitmap = live.slice();
      to.refreshDrawing();
    }
  } finally {
    mirroring = false;
  }
}

// The label colours NiiVue paints with. Index 0 is the background: transparent,
// or every unlabelled voxel would be tinted.
function applyLabelColours() {
  const rgb = (hex) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));
  const cmap = {
    R: [0, ...LABELS.map((l) => rgb(l.color)[0])],
    G: [0, ...LABELS.map((l) => rgb(l.color)[1])],
    B: [0, ...LABELS.map((l) => rgb(l.color)[2])],
    A: [0, ...LABELS.map(() => 255)],
    labels: ["", ...LABELS.map((l) => `label ${l.value}`)],
  };
  for (const nv of drawables()) nv.setDrawColormap(cmap);
}

// Drag by the header. The palette is `fixed`, so the coordinates are the
// viewport's and it can be parked anywhere on the page — over a card, beside the
// viewers, out of the way of the region being painted. Clamped to the window so
// it can never be dropped somewhere it cannot be grabbed back from.
function makeDraggable(box, handle) {
  handle.addEventListener("pointerdown", (down) => {
    down.preventDefault();
    const start = box.getBoundingClientRect();
    const dx = down.clientX - start.left;
    const dy = down.clientY - start.top;
    const move = (e) => {
      const x = e.clientX - dx;
      const y = e.clientY - dy;
      box.style.left = `${Math.max(0, Math.min(x, window.innerWidth - start.width))}px`;
      box.style.top = `${Math.max(0, Math.min(y, window.innerHeight - start.height))}px`;
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  });
}

// A shape is stamped centred on the pointer at the brush size, so one click
// gives a region of known size and position. `drawPt` is the only bitmap-level
// paint the build exposes (`drawRect` is a WebGL selection box, not a paint), so
// the footprint is walked voxel by voxel.
// NiiVue allocates the drawing bitmap on its first freehand stroke, so every tool
// that writes without one — a stamp, a fill, grow-cut — would otherwise throw on a
// fresh session: `drawPt` and `drawGrowCut` both error outright with no bitmap.
//
// It matters for a second reason. `createEmptyDrawing` is also what creates the GL
// texture a drawing is rendered through, so a viewer that never had one cannot
// display a mirrored bitmap at all — the strokes arrive and paint nothing. Every
// viewer therefore gets its drawing up front, not on first use.
function ensureDrawing() {
  for (const nv of drawables()) {
    if (!nv.drawBitmap) nv.createEmptyDrawing();
  }
  return Boolean(nvOut.drawBitmap);
}

// The voxel under the pointer, or null when the pointer is not over a slice.
// `canvasPos2frac` works in backing-store pixels, which differ from CSS pixels on
// a high-resolution canvas.
function voxelAt(nv, event) {
  if (!app.current) return null;
  const rect = nv.canvas.getBoundingClientRect();
  if (!rect.width || !rect.height) return null;
  const frac = nv.canvasPos2frac([
    (event.clientX - rect.left) * (nv.canvas.width / rect.width),
    (event.clientY - rect.top) * (nv.canvas.height / rect.height),
  ]);
  // A negative first component means the pointer was not over a slice at all.
  if (!frac || frac[0] < 0) return null;
  return nv.frac2vox(frac);
}

// Fill the enclosed area the click lands in — the companion to drawing an
// outline. Bounded to the slice, like every other tool here.
function fillAt(nv, event) {
  const at = voxelAt(nv, event);
  if (!at || !ensureDrawing()) return false;
  nv.drawFloodFill(at, tool.erase ? 0 : label, 0, NaN, NaN, 6, Number.POSITIVE_INFINITY, true);
  return true;
}

function stampAt(nv, event) {
  const at = voxelAt(nv, event);
  if (!at || !ensureDrawing()) return false;
  const [cx, cy, cz] = at;
  const [nx, ny] = app.current.meta.dims;
  const r = Math.max(1, Math.round(penSize()));
  const value = tool.erase ? 0 : label;
  const round = tool.stamp === "ellipse";
  for (let y = cy - r; y <= cy + r; y++) {
    for (let x = cx - r; x <= cx + r; x++) {
      if (x < 0 || y < 0 || x >= nx || y >= ny) continue;
      if (round && (x - cx) ** 2 + (y - cy) ** 2 > r * r) continue;
      nv.drawPt(x, y, cz, value);
    }
  }
  nv.refreshDrawing(true);
  return true;
}

// Otsu and grow-cut act on the whole slice at once rather than where a pointer
// is, so they are actions rather than tools. Both write into the drawing, so both
// become history steps like any stroke.
function segmentOtsu() {
  if (!app.current || !ensureDrawing()) return;
  nvOut.drawOtsu(SETTINGS.levels.value);
  nvOut.refreshDrawing(true);
  mirrorDrawing(nvOut);
  pushHistory();
  onStroke?.();
}

// Grow-cut expands the regions already drawn until they meet, using the image to
// decide where the boundary falls. It needs at least two labels to grow *between*;
// with one it has nothing to compete against and would flood the slice.
function segmentGrowCut() {
  if (usedLabels().size < 2) {
    status("Grow-cut needs two labels drawn as seeds — one region, one background", "error");
    return;
  }
  if (!ensureDrawing()) return;
  nvOut.drawGrowCut();
  nvOut.refreshDrawing(true);
  mirrorDrawing(nvOut);
  pushHistory();
  onStroke?.();
}

// NiiVue adjusts `clickToSegmentPercent` itself when the wheel turns with the wand
// armed — a genuinely good gesture, since it grows and shrinks the region live. It
// does mean the option can change without the palette knowing, so the palette
// reads it back rather than assuming it still owns the value.
function syncWandTolerance(nv) {
  if (!tool.wand) return;
  // Every viewer shares the setting, so one changing it means all of them did.
  const pct = Math.round((nv.opts.clickToSegmentPercent ?? 0) * 100);
  if (pct === SETTINGS.tolerance.value) return;
  SETTINGS.tolerance.value = Math.max(SETTINGS.tolerance.min, Math.min(SETTINGS.tolerance.max, pct));
  paintPalette();
}

// Where the eraser has been, drawn as a dashed path over the image.
//
// Erasing removes colour, so the stroke leaves no mark of its own: without this
// there is no way to see which perimeter was swept, and a reader is deleting
// labels blind. The trail is transient — it says "this is what you just took",
// not "this is a region" — so it fades once the stroke ends.
//
// It is drawn in the canvas's own pixels straight from pointer coordinates, so it
// needs no voxel mapping and cannot disagree with where the pointer actually was.
const TRAIL_FADE_MS = 900;
const trails = new WeakMap();

function trailFor(nv) {
  let trail = trails.get(nv);
  if (!trail) {
    const layer = document.createElement("canvas");
    layer.className = "erase-trail";
    nv.canvas.parentElement.append(layer);
    trail = { layer, timer: 0 };
    trails.set(nv, trail);
  }
  const rect = nv.canvas.getBoundingClientRect();
  if (trail.layer.width !== Math.round(rect.width) || trail.layer.height !== Math.round(rect.height)) {
    trail.layer.width = Math.round(rect.width);
    trail.layer.height = Math.round(rect.height);
  }
  return trail;
}

function clearTrail(nv) {
  const trail = trails.get(nv);
  if (!trail) return;
  trail.layer.getContext("2d").clearRect(0, 0, trail.layer.width, trail.layer.height);
}

// The wand is the one tool NiiVue does not announce: `doClickToSegment` never
// calls `onDrawingChanged`, unlike every other path (draw, undo, load, close). So
// nothing downstream — the mirror to the other viewers, the history, the
// statistics — hears about a grown region unless we watch for it ourselves.
//
// *When* we look matters as much as that we look. NiiVue also runs the segmenter
// on hover, to preview what a click would take, and that preview is written
// straight into `drawBitmap`. Mirroring at an arbitrary later moment therefore
// copies whatever the pointer happens to be hovering over — and the receiving
// viewer, which never previews, has no reason to ever clear it. So the mirror
// runs only at moments the region is confirmed: the release of a click, and each
// wheel tick of a deliberate grow.
let wandSettle = 0;

function commitWand(nv) {
  mirrorDrawing(nv);
  syncWandTolerance(nv);
  pushHistory();
  onStroke?.();
}

function wireWand(nv) {
  const armed = () => app.roiDrawing && tool.wand;
  // Released, not pressed: the press only seeds the region, and the value under
  // the pointer can still change before it is let go.
  nv.canvas.addEventListener("pointerup", () => {
    if (!armed()) return;
    clearTimeout(wandSettle);
    requestAnimationFrame(() => commitWand(nv));
  });
  // A scroll grows the region live, so it is mirrored on every tick — but recorded
  // as one undo step once the gesture stops, not forty.
  nv.canvas.addEventListener(
    "wheel",
    () => {
      if (!armed()) return;
      requestAnimationFrame(() => {
        mirrorDrawing(nv);
        syncWandTolerance(nv);
        onStroke?.();
      });
      clearTimeout(wandSettle);
      wandSettle = setTimeout(() => pushHistory(), 400);
    },
    { passive: true },
  );
}

function wireEraseTrail(nv) {
  let last = null;
  const point = (e) => {
    const rect = nv.canvas.getBoundingClientRect();
    return [e.clientX - rect.left, e.clientY - rect.top];
  };
  nv.canvas.addEventListener("pointerdown", (e) => {
    if (!app.roiDrawing || !tool.erase) return;
    const trail = trailFor(nv);
    clearTimeout(trail.timer);
    clearTrail(nv);
    last = point(e);
  });
  nv.canvas.addEventListener("pointermove", (e) => {
    if (!last) return;
    const trail = trailFor(nv);
    const ctx = trail.layer.getContext("2d");
    const now = point(e);
    // Two passes so the dashes read on light tissue and on black background.
    for (const [colour, width, dash] of [["rgba(0,0,0,.6)", 3, []], ["#fff", 1.4, [4, 3]]]) {
      ctx.strokeStyle = colour;
      ctx.lineWidth = width;
      ctx.setLineDash(dash);
      ctx.lineCap = "round";
      ctx.beginPath();
      ctx.moveTo(last[0], last[1]);
      ctx.lineTo(now[0], now[1]);
      ctx.stroke();
    }
    last = now;
  });
  for (const end of ["pointerup", "pointerleave", "pointercancel"]) {
    nv.canvas.addEventListener(end, () => {
      if (!last) return;
      last = null;
      const trail = trailFor(nv);
      trail.timer = setTimeout(() => clearTrail(nv), TRAIL_FADE_MS);
    });
  }
}

// Stamping only makes sense for the shape tools: the pen already paints on click.
function wireStamp(nv) {
  nv.canvas.addEventListener("pointerdown", (e) => {
    if (!app.roiDrawing) return;
    // Acted on press, not release: the mark lands where the pointer went down, so
    // a small drag afterwards cannot move it somewhere unexpected.
    const did = tool.stamp ? stampAt(nv, e) : tool.fill ? fillAt(nv, e) : false;
    if (!did) return;
    nv.refreshDrawing(true);
    mirrorDrawing(nv);
    nv.onDrawingChanged?.();
  });
}

function toolButton(t) {
  const b = document.createElement("button");
  b.type = "button";
  b.className = `draw-tool${t === tool ? " active" : ""}`;
  b.title = t.title;
  b.innerHTML = icon(t.icon, 15);
  b.onclick = () => {
    tool = t;
    applyTool();
    paintPalette();
  };
  return b;
}

function actionButton(name, title, fn) {
  const b = document.createElement("button");
  b.type = "button";
  b.className = "draw-tool";
  b.title = title;
  b.innerHTML = icon(name, 15);
  b.onclick = fn;
  return b;
}

// Which label values the drawing actually contains. A label nobody has drawn
// with is free to be recoloured; one with voxels in it is not.
function usedLabels() {
  const used = new Set();
  const bitmap = nvOut.drawBitmap;
  if (bitmap) for (const v of bitmap) if (v !== 0) used.add(v);
  return used;
}

// Give `color` a label to paint as. An untouched label is reused — six swatches
// that each spawned a new one would fill up in a minute — and otherwise the
// palette grows, so no existing region is ever repainted.
function allocateLabel(color) {
  const used = usedLabels();
  const free = LABELS.find((l) => !used.has(l.value));
  if (free) {
    free.color = color;
    return free.value;
  }
  const value = Math.max(...LABELS.map((l) => l.value)) + 1;
  LABELS.push({ value, color });
  return value;
}

function paintPalette() {
  const box = $("draw-palette");
  box.replaceChildren();

  const head = document.createElement("div");
  head.className = "draw-head";
  head.title = "Drag to move";
  const grip = document.createElement("span");
  grip.className = "draw-grip";
  grip.textContent = "⠿";
  const close = document.createElement("button");
  close.type = "button";
  close.className = "draw-close";
  close.title = "Close (stops drawing)";
  close.textContent = "×";
  // Closing turns drawing off rather than only hiding the palette: a hidden
  // palette with the pen still armed is a state with no way back.
  close.onclick = () => toggleDrawing();
  head.append(grip, close);
  makeDraggable(box, head);

  const tools = document.createElement("div");
  tools.className = "draw-tools";
  for (const t of TOOLS) tools.append(toolButton(t));

  const actions = document.createElement("div");
  actions.className = "draw-tools";
  const undo = actionButton("undo-2", `Undo (up to ${HISTORY_STEPS - 1} steps)`, undoDrawing);
  undo.id = "draw-undo";
  const redo = actionButton("redo-2", "Redo", redoDrawing);
  redo.id = "draw-redo";
  actions.append(undo, redo);


  // What the tool in hand expects, in one line. The tools differ enough that
  // guessing is not reasonable — a wand takes a click, grow-cut takes seeds and a
  // press of Apply — so the palette says so rather than hiding it in a tooltip.
  const hint = document.createElement("p");
  hint.className = "draw-hint";
  hint.textContent = tool.hint;

  const size = document.createElement("div");
  size.className = "draw-size";
  const knob = tool.setting ? SETTINGS[tool.setting] : null;
  if (knob) {
    const value = document.createElement("span");
    value.className = "draw-size-value";
    value.title = knob.title;

    // Updated in place rather than by repainting the palette: a repaint would
    // replace the button being held and the repeat would die on the first tick.
    const refresh = () => {
      value.textContent = `${knob.value}${knob.unit}`;
      minus.disabled = knob.value <= knob.min;
      plus.disabled = knob.value >= knob.max;
    };
    const step = (delta) => {
      const next = Math.max(knob.min, Math.min(knob.max, knob.value + delta));
      if (next === knob.value) return false;
      knob.value = next;
      applyTool();
      refresh();
      return true;
    };

    // Press and hold to run: a pause first, so a single click stays a single
    // step, then accelerating, because these ranges reach 100.
    const held = (delta) => (down) => {
      down.preventDefault();
      if (!step(delta)) return;
      let timer = 0;
      let ticks = 0;
      const tick = () => {
        if (!step(delta)) return stop();
        ticks += 1;
        timer = setTimeout(tick, ticks > 12 ? 25 : ticks > 5 ? 55 : 90);
      };
      const stop = () => {
        clearTimeout(timer);
        for (const e of ["pointerup", "pointercancel", "pointerleave"]) {
          window.removeEventListener(e, stop);
        }
      };
      timer = setTimeout(tick, 330);
      for (const e of ["pointerup", "pointercancel", "pointerleave"]) {
        window.addEventListener(e, stop);
      }
    };

    const minus = document.createElement("button");
    minus.type = "button";
    minus.className = "draw-step";
    minus.textContent = "−";
    minus.title = knob.title;
    minus.addEventListener("pointerdown", held(-1));
    const plus = document.createElement("button");
    plus.type = "button";
    plus.className = "draw-step";
    plus.textContent = "+";
    plus.title = knob.title;
    plus.addEventListener("pointerdown", held(1));
    refresh();
    size.append(minus, value, plus);
  }

  // A whole-slice tool has nothing to click on the image, so it needs a trigger.
  const apply = document.createElement("button");
  apply.type = "button";
  apply.className = "draw-apply";
  apply.textContent = "Apply";
  apply.hidden = !tool.whole;
  apply.onclick = () => (tool.id === "otsu" ? segmentOtsu() : segmentGrowCut());

  const swatches = document.createElement("div");
  swatches.className = "draw-labels";
  for (const l of LABELS) {
    const b = document.createElement("button");
    b.type = "button";
    b.className = `draw-swatch${l.value === label ? " active" : ""}`;
    b.style.background = l.color;
    b.title = `Label ${l.value}`;
    b.onclick = () => {
      label = l.value;
      // Picking a colour means painting, not erasing.
      if (tool.erase) tool = TOOLS[0];
      applyTool();
      paintPalette();
    };
    swatches.append(b);
  }

  // A fresh colour means a fresh label, so nothing already drawn changes colour.
  // `change` rather than `input`: the native picker streams `input` while the
  // pointer moves through the gradient, which would allocate a label per pixel
  // travelled — `change` fires once, when the choice is confirmed.
  const picker = document.createElement("input");
  picker.type = "color";
  picker.className = "draw-picker";
  picker.value = activeColor();
  picker.title = "New colour — paints as a new label";
  picker.onchange = () => {
    label = allocateLabel(picker.value);
    if (tool.erase) tool = TOOLS[0];
    applyLabelColours();
    applyTool();
    paintPalette();
  };

  box.append(head, tools, actions, hint, size, apply, swatches, picker);
}

// NiiVue keeps 8 undo bitmaps and offers no redo, so the history is kept here
// instead: a bounded stack of bitmap snapshots with a cursor into it. That gives
// a depth we can state, and a redo.
//
// Bounded twice over — by steps and by bytes — because a snapshot is one byte per
// voxel: trivial for a 128x128 slice, but a volumetric dataset would otherwise
// hold hundreds of megabytes of undo.
const HISTORY_STEPS = 24;
const HISTORY_BYTES = 48 * 1024 * 1024;
let history = [];
let historyAt = -1;

function historyBytes() {
  return history.reduce((n, b) => n + b.length, 0);
}

// Record the state *after* a stroke. Anything undone is discarded first: a new
// stroke is a new branch, and keeping the old future would let redo resurrect
// something the reader has painted over.
function pushHistory() {
  const bitmap = nvOut.drawBitmap;
  if (!bitmap) return;
  // The first entry is the blank canvas, so the first undo returns to empty.
  if (history.length === 0) history.push(new Uint8Array(bitmap.length));
  history = history.slice(0, historyAt + 1 || 1);
  history.push(bitmap.slice());
  while (history.length > HISTORY_STEPS || historyBytes() > HISTORY_BYTES) history.shift();
  historyAt = history.length - 1;
  syncHistoryButtons();
}

function restoreHistory(at) {
  historyAt = at;
  const snapshot = history[at];
  for (const nv of drawables()) {
    nv.drawBitmap = snapshot.slice();
    nv.refreshDrawing();
  }
  onStroke?.();
  syncHistoryButtons();
}

function undoDrawing() {
  if (historyAt > 0) restoreHistory(historyAt - 1);
}

function redoDrawing() {
  if (historyAt < history.length - 1) restoreHistory(historyAt + 1);
}

function syncHistoryButtons() {
  const undo = $("draw-undo");
  const redo = $("draw-redo");
  if (undo) undo.disabled = historyAt <= 0;
  if (redo) redo.disabled = historyAt >= history.length - 1;
}

let placed = false;

// Park the palette in the gutter between the inputs and the fitted map: near both
// viewers, over neither. Only until the reader drags it somewhere they prefer.
function placePalette(box) {
  if (placed) return;
  const inputs = $("viewer-in-wrap")?.getBoundingClientRect();
  const map = $("viewer-out-wrap")?.getBoundingClientRect();
  if (!inputs || !map) return;
  const width = box.offsetWidth || 40;
  const mid = (inputs.right + map.left) / 2 - width / 2;
  box.style.left = `${Math.max(4, Math.min(mid, window.innerWidth - width - 4))}px`;
  box.style.top = `${Math.round(map.top + 6)}px`;
  placed = true;
}

// A drawing belongs to the volume it was made on: a label is a value at a voxel
// index into one model's grid, with a name a reader gave it for that anatomy.
// Carrying it across a model switch would paint one dataset's regions over
// another's — and against different dims, over the wrong voxels entirely — so a
// model load drops the drawing rather than reinterpreting it.
//
// `closeDrawing` is NiiVue's own teardown: it frees the GL texture as well as the
// bitmap, so the next `ensureDrawing` builds both fresh at the new dims.
export function resetDrawing() {
  if (app.roiDrawing) toggleDrawing();
  if (!labelsVisible) toggleLabels();
  for (const nv of drawables()) {
    if (nv.drawBitmap) nv.closeDrawing();
  }
  history = [];
  historyAt = -1;
  syncHistoryButtons();
  onStroke?.();
}

export function toggleDrawing() {
  app.roiDrawing = !app.roiDrawing;
  $("roi-toggle").classList.toggle("active", app.roiDrawing);
  $("roi-toggle-label").textContent = app.roiDrawing ? "Drawing…" : "Draw ROI";
  const box = $("draw-palette");
  box.hidden = !app.roiDrawing;
  if (app.roiDrawing) {
    paintPalette();
    placePalette(box);
  }
  applyMode();
}

// Called when the modal shows the fitted map, so a reader can keep drawing in the
// larger multiplanar view rather than having to close it first.
// The modal is the only viewer that comes and goes, so it is the only one that
// needs catching up.
export function attachModalDrawing() {
  if (!app.nvModal) return;
  applyLabelColours();
  wireStamp(app.nvModal);
  wireEraseTrail(app.nvModal);
  wireWand(app.nvModal);
  applyMode();
  app.nvModal.setDrawOpacity(labelsVisible ? DRAW_OPACITY : 0);
  if (nvOut.drawBitmap) mirrorDrawing(nvOut);
  app.nvModal.onDrawingChanged = () => {
    mirrorDrawing(app.nvModal);
    pushHistory();
    onStroke?.();
  };
}

// What to run after a stroke, whoever drew it. Registered by the wiring module,
// which is where the knowledge of "recompute the statistics" belongs.
let onStroke = null;
export function onDrawingStroke(fn) {
  onStroke = fn;
}

// The modal viewer is shared with the file previewer, which shows an acquired
// image that need not share the fitted map's shape. Painting there would write
// through a bitmap sized for a different volume, so the pen is taken away and the
// stale drawing dropped whenever the modal is used for something else.
export function detachModalDrawing() {
  if (!app.nvModal) return;
  app.nvModal.onDrawingChanged = () => {};
  app.nvModal.setDrawingEnabled(false);
  app.nvModal.drawBitmap = null;
  app.nvModal.canvas.style.cursor = "";
}

export function wireDrawing() {
  applyLabelColours();
  $("roi-toggle").onclick = toggleDrawing;
  $("labels-toggle").onclick = toggleLabels;
  syncLabelToggle();
  for (const nv of [nvIn, nvOut]) {
    wireStamp(nv);
    wireEraseTrail(nv);
    wireWand(nv);
    nv.onDrawingChanged = () => {
      mirrorDrawing(nv);
      pushHistory();
      syncWandTolerance(nv);
      onStroke?.();
    };
  }
}
