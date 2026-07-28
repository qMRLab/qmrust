// Drawing on the map: the tool palette, the labels, and one drawing shared by
// both viewers.
//
// Manual tools only. The vendored NiiVue also offers Otsu, grow-cut and
// click-to-segment, and they are deliberately absent: a tolerance-based region
// silently includes or drops voxels, which biases a mean that then gets quoted.
// Every region here is traceable to a deliberate gesture.
import { DRAG_MODE } from "./vendor/niivue.js";
import { icon, ICON_SHAPES } from "./vendor/icons.js";
import { $ } from "./dom.js";
import { app, nvIn, nvOut } from "./state.js";

// Six labels. The first three are the brand metals — verdigris, brass, copper —
// so a row in the measurements table and a region on the map are obviously the
// same object. Each is recolourable; the *value* is what the drawing stores, and
// the colour is only how NiiVue paints that value.
export const LABELS = [
  { value: 1, color: "#7cc3b5" },
  { value: 2, color: "#c9a86a" },
  { value: 3, color: "#cf8a5f" },
  { value: 4, color: "#8fc39a" },
  { value: 5, color: "#8ab0d6" },
  { value: 6, color: "#d76a4c" },
];

// `penType`: 0 freehand, 1 rectangle, 2 ellipse — the values the vendored build
// compares against. `filled` is `setPenValue`'s second argument, which decides
// whether a shape lands solid or as an outline.
const TOOLS = [
  { id: "pen", icon: "pencil", title: "Freehand", penType: 0 },
  { id: "rect", icon: "square", title: "Rectangle", penType: 1 },
  { id: "ellipse", icon: "circle", title: "Ellipse", penType: 2 },
  { id: "erase", icon: "eraser", title: "Erase", penType: 0, erase: true },
];

let tool = TOOLS[0];
let label = 1;
let filled = true;
let penSize = 3;
// Assigning `drawBitmap` fires `onDrawingChanged` on the receiving instance,
// which would mirror straight back; this flag is what stops the ping-pong.
let mirroring = false;

export function isDrawing() {
  return app.roiDrawing;
}

export function currentLabel() {
  return label;
}

function activeColor() {
  return LABELS.find((l) => l.value === label).color;
}

// The pointer carries the tool's own glyph, so which tool is armed is visible
// where the work happens rather than only in the palette. `currentColor` cannot
// resolve inside a cursor image, so the shape is stroked explicitly: white over a
// dark halo, which stays legible on both bright tissue and black background.
function cursorFor(t) {
  const shapes = ICON_SHAPES[t.icon];
  if (!shapes) return "crosshair";
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" width="22" height="22" viewBox="0 0 24 24" ` +
    `fill="none" stroke-linecap="round" stroke-linejoin="round">` +
    `<g stroke="#000" stroke-opacity=".55" stroke-width="3.4">${shapes}</g>` +
    `<g stroke="#fff" stroke-width="1.8">${shapes}</g></svg>`;
  // The hotspot sits at the glyph's centre; a keyword fallback is required.
  return `url("data:image/svg+xml,${encodeURIComponent(svg)}") 11 11, crosshair`;
}

// Both viewers accept the pen, so the same gesture works wherever the structure
// is easier to see. Drawing and drag-to-window are the same gesture, so one must
// yield while the pen is active — and the crosshair goes away, because it sits
// exactly where you are trying to draw.
function applyMode() {
  for (const nv of [nvIn, nvOut]) {
    nv.setDrawingEnabled(app.roiDrawing);
    nv.opts.dragMode = app.roiDrawing ? DRAG_MODE.none : DRAG_MODE.contrast;
    nv.setCrosshairWidth(app.roiDrawing ? 0 : 1);
  }
  applyTool();
}

function applyTool() {
  const cursor = app.roiDrawing ? cursorFor(tool) : "";
  for (const nv of [nvIn, nvOut]) {
    nv.opts.penType = tool.penType;
    nv.opts.penSize = penSize;
    nv.setPenValue(tool.erase ? 0 : label, filled);
    nv.canvas.style.cursor = cursor;
  }
}

// One logical drawing, visible in both viewers: draw on anatomy where structure
// is clearer, measure on the map. Both render the same `current.meta.dims`, so
// this is bookkeeping — no resampling and no geometry of ours.
export function mirrorDrawing(from) {
  if (mirroring) return;
  const to = from === nvOut ? nvIn : nvOut;
  mirroring = true;
  try {
    to.drawBitmap = from.drawBitmap;
    to.refreshDrawing();
  } finally {
    mirroring = false;
  }
}

export function clearDrawing() {
  for (const nv of [nvIn, nvOut]) {
    nv.drawBitmap = null;
    nv.updateGLVolume();
    nv.drawScene();
  }
}

export function undoDrawing() {
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
  for (const nv of [nvIn, nvOut]) nv.setDrawColormap(cmap);
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
      // Outline rather than solid: keep only the shape's border ring.
      if (!filled) {
        const edge = round
          ? (x - cx) ** 2 + (y - cy) ** 2 > (r - 1) * (r - 1)
          : Math.abs(x - cx) === r || Math.abs(y - cy) === r;
        if (!edge) continue;
      }
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

  const shape = document.createElement("button");
  shape.type = "button";
  shape.className = `draw-tool draw-filled${filled ? " active" : ""}`;
  shape.title = filled ? "Shapes are solid" : "Shapes are outlines";
  shape.textContent = filled ? "◼" : "◻";
  shape.onclick = () => {
    filled = !filled;
    applyTool();
    paintPalette();
  };
  tools.append(shape);
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

  // Recolour the active label. The drawing stores label *values*, so this edits
  // the palette NiiVue paints them with and leaves every drawn voxel where it is.
  const picker = document.createElement("input");
  picker.type = "color";
  picker.className = "draw-picker";
  picker.value = activeColor();
  picker.title = `Colour of label ${label}`;
  picker.oninput = () => {
    LABELS.find((l) => l.value === label).color = picker.value;
    applyLabelColours();
    paintPalette();
  };

  box.append(head, tools, size, swatches, picker);
}

export function toggleDrawing() {
  app.roiDrawing = !app.roiDrawing;
  $("roi-toggle").classList.toggle("active", app.roiDrawing);
  $("roi-toggle-label").textContent = app.roiDrawing ? "drawing…" : "draw ROI";
  const box = $("draw-palette");
  box.hidden = !app.roiDrawing;
  if (app.roiDrawing) paintPalette();
  applyMode();
}

export function wireDrawing() {
  applyLabelColours();
  $("roi-toggle").onclick = toggleDrawing;
  for (const nv of [nvIn, nvOut]) wireStamp(nv);
}
