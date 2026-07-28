// Drawing on the map: the tool palette, the labels, and one drawing shared by
// both viewers.
//
// Manual tools only. The vendored NiiVue also offers Otsu, grow-cut and
// click-to-segment, and they are deliberately absent: a tolerance-based region
// silently includes or drops voxels, which biases a mean that then gets quoted.
// Every region here is traceable to a deliberate gesture.
import { DRAG_MODE } from "./vendor/niivue.js";
import { icon } from "./vendor/icons.js";
import { $ } from "./dom.js";
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
const LABELS = [
  { value: 1, color: "#7cc3b5" },
  { value: 2, color: "#c9a86a" },
  { value: 3, color: "#cf8a5f" },
  { value: 4, color: "#8fc39a" },
  { value: 5, color: "#8ab0d6" },
  { value: 6, color: "#d76a4c" },
];

// `penType`: 0 freehand, 1 rectangle, 2 ellipse — the values the vendored build
// compares against. Shapes are always solid.
const TOOLS = [
  { id: "pen", icon: "pencil", title: "Freehand", penType: 0 },
  { id: "rect", icon: "square", title: "Rectangle", penType: 1 },
  { id: "ellipse", icon: "circle", title: "Ellipse", penType: 2 },
  { id: "erase", icon: "eraser", title: "Erase", penType: 0, erase: true },
];

let tool = TOOLS[0];
let label = 1;
let penSize = 1;
// Assigning `drawBitmap` fires `onDrawingChanged` on the receiving instance,
// which would mirror straight back; this flag is what stops the ping-pong.
let mirroring = false;

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
  if (t.penType === 1) {
    shapes = `<rect x="${c - r}" y="${c - r}" width="${r * 2}" height="${r * 2}"/>`;
  } else if (t.penType === 2) {
    shapes = `<circle cx="${c}" cy="${c}" r="${r}"/>`;
  } else {
    // Freehand and erase: a cross, with erase ringed so the two are not confused.
    shapes =
      `<line x1="${c - arm}" y1="${c}" x2="${c + arm}" y2="${c}"/>` +
      `<line x1="${c}" y1="${c - arm}" x2="${c}" y2="${c + arm}"/>` +
      (t.erase ? `<circle cx="${c}" cy="${c}" r="${r}" stroke-dasharray="2 2"/>` : "");
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
  for (const nv of drawables()) {
    nv.setDrawingEnabled(app.roiDrawing);
    nv.opts.dragMode = app.roiDrawing ? DRAG_MODE.none : DRAG_MODE.contrast;
    nv.setCrosshairWidth(app.roiDrawing ? 0 : 1);
  }
  applyTool();
}

function applyTool() {
  const cursor = app.roiDrawing ? cursorFor(tool, penSize) : "";
  for (const nv of drawables()) {
    nv.opts.penType = tool.penType;
    nv.opts.penSize = penSize;
    nv.setPenValue(tool.erase ? 0 : label, true);
    nv.canvas.style.cursor = cursor;
  }
}

// One logical drawing, visible in both viewers: draw on anatomy where structure
// is clearer, measure on the map. Both render the same `current.meta.dims`, so
// this is bookkeeping — no resampling and no geometry of ours.
function mirrorDrawing(from) {
  if (mirroring) return;
  mirroring = true;
  try {
    for (const to of drawables()) {
      if (to === from) continue;
      to.drawBitmap = from.drawBitmap;
      to.refreshDrawing();
    }
  } finally {
    mirroring = false;
  }
}

function clearDrawing() {
  for (const nv of drawables()) {
    nv.drawBitmap = null;
    nv.updateGLVolume();
    nv.drawScene();
  }
}

function undoDrawing() {
  nvOut.drawUndo();
  mirrorDrawing(nvOut);
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

// A click with no drag stamps the shape at the brush size, so a region of known
// size takes one gesture; dragging still scales it, which is NiiVue's own
// behaviour and left alone. `drawPt` is the only bitmap-level paint the build
// exposes (`drawRect` is a WebGL selection box, not a paint), so the footprint is
// walked voxel by voxel.
const CLICK_SLOP = 3;

function stampAt(nv, event) {
  if (!app.current) return false;
  const rect = nv.canvas.getBoundingClientRect();
  if (!rect.width || !rect.height) return false;
  // `canvasPos2frac` works in backing-store pixels, which differ from CSS pixels
  // on a high-resolution canvas.
  const frac = nv.canvasPos2frac([
    (event.clientX - rect.left) * (nv.canvas.width / rect.width),
    (event.clientY - rect.top) * (nv.canvas.height / rect.height),
  ]);
  // A negative first component means the cursor was not over a slice at all.
  if (!frac || frac[0] < 0) return false;
  const [cx, cy, cz] = nv.frac2vox(frac);
  const [nx, ny] = app.current.meta.dims;
  const r = Math.max(1, Math.round(penSize));
  const value = tool.erase ? 0 : label;
  const round = tool.id === "ellipse";
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

// Stamping only makes sense for the shape tools: the pen already paints on click.
function wireStamp(nv) {
  let down = null;
  nv.canvas.addEventListener("pointerdown", (e) => {
    down = app.roiDrawing && tool.penType !== 0 ? { x: e.clientX, y: e.clientY } : null;
  });
  nv.canvas.addEventListener("pointerup", (e) => {
    if (!down) return;
    const moved = Math.hypot(e.clientX - down.x, e.clientY - down.y);
    down = null;
    if (moved > CLICK_SLOP) return; // a drag: NiiVue already scaled the shape
    if (stampAt(nv, e)) {
      mirrorDrawing(nv);
      nv.onDrawingChanged?.();
    }
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

  tools.append(actionButton("undo-2", "Undo", undoDrawing));
  tools.append(actionButton("trash-2", "Clear all", clearDrawing));

  // Brush size, in voxels — NiiVue reads `opts.penSize` on every stroke.
  const size = document.createElement("div");
  size.className = "draw-size";
  const slider = document.createElement("input");
  slider.type = "range";
  slider.min = "1";
  slider.max = "15";
  slider.step = "1";
  slider.value = String(penSize);
  slider.title = "Brush size";
  const readout = document.createElement("span");
  readout.textContent = String(penSize);
  slider.oninput = () => {
    penSize = Number(slider.value);
    readout.textContent = String(penSize);
    applyTool();
  };
  size.append(slider, readout);

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

  box.append(head, tools, size, swatches, picker);
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
  applyMode();
  if (nvOut.drawBitmap) mirrorDrawing(nvOut);
  app.nvModal.onDrawingChanged = () => {
    mirrorDrawing(app.nvModal);
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
  for (const nv of [nvIn, nvOut]) {
    wireStamp(nv);
    nv.onDrawingChanged = () => {
      mirrorDrawing(nv);
      onStroke?.();
    };
  }
}
