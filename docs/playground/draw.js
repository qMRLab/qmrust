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

// Six labels. The first three are the brand metals — verdigris, brass, copper —
// so a row in the measurements table and a region on the map are obviously the
// same object. Index 0 is the background and must stay fully transparent.
export const LABELS = [
  { value: 1, color: [124, 195, 181] },
  { value: 2, color: [201, 168, 106] },
  { value: 3, color: [207, 138, 95] },
  { value: 4, color: [143, 195, 154] },
  { value: 5, color: [138, 176, 214] },
  { value: 6, color: [215, 106, 76] },
];

// `penType`: 0 freehand, 1 rectangle, 2 ellipse — the values the vendored build
// compares against. `filled` is NiiVue's second `setPenValue` argument, which
// decides whether a shape is solid or an outline.
const TOOLS = [
  { id: "pen", icon: "pencil", title: "Freehand", penType: 0 },
  { id: "rect", icon: "square", title: "Rectangle", penType: 1 },
  { id: "ellipse", icon: "circle", title: "Ellipse", penType: 2 },
  { id: "erase", icon: "eraser", title: "Erase", penType: 0, erase: true },
];

let tool = TOOLS[0];
let label = 1;
let filled = true;
// Assigning `drawBitmap` fires `onDrawingChanged` on the receiving instance,
// which would mirror straight back; this flag is what stops the ping-pong.
let mirroring = false;

export function isDrawing() {
  return app.roiDrawing;
}

// Both viewers accept the pen, so the same gesture works wherever the structure
// is easier to see. Drawing and drag-to-window are the same gesture, so one must
// yield while the pen is active.
function applyMode() {
  for (const nv of [nvIn, nvOut]) {
    nv.setDrawingEnabled(app.roiDrawing);
    nv.opts.dragMode = app.roiDrawing ? DRAG_MODE.none : DRAG_MODE.contrast;
  }
  applyTool();
}

function applyTool() {
  for (const nv of [nvIn, nvOut]) {
    nv.opts.penType = tool.penType;
    nv.setPenValue(tool.erase ? 0 : label, filled);
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

function paintPalette() {
  const box = $("draw-palette");
  box.replaceChildren();

  const tools = document.createElement("div");
  tools.className = "draw-row";
  for (const t of TOOLS) {
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
    tools.append(b);
  }

  const acts = document.createElement("div");
  acts.className = "draw-row";
  for (const [name, title, fn] of [
    ["undo-2", "Undo", undoDrawing],
    ["trash-2", "Clear all", clearDrawing],
  ]) {
    const b = document.createElement("button");
    b.type = "button";
    b.className = "draw-tool";
    b.title = title;
    b.innerHTML = icon(name, 15);
    b.onclick = fn;
    acts.append(b);
  }

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
  acts.append(shape);

  const swatches = document.createElement("div");
  swatches.className = "draw-row draw-labels";
  for (const l of LABELS) {
    const b = document.createElement("button");
    b.type = "button";
    b.className = `draw-swatch${l.value === label ? " active" : ""}`;
    b.style.background = `rgb(${l.color.join(",")})`;
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

  box.append(tools, acts, swatches);
}

// The label colours NiiVue paints with. Index 0 is the background: transparent,
// or every unlabelled voxel would be tinted.
function applyLabelColours() {
  const cmap = {
    R: [0, ...LABELS.map((l) => l.color[0])],
    G: [0, ...LABELS.map((l) => l.color[1])],
    B: [0, ...LABELS.map((l) => l.color[2])],
    A: [0, ...LABELS.map(() => 255)],
    labels: ["", ...LABELS.map((l) => `label ${l.value}`)],
  };
  for (const nv of [nvIn, nvOut]) nv.setDrawColormap(cmap);
}

export function toggleDrawing() {
  app.roiDrawing = !app.roiDrawing;
  $("roi-toggle").classList.toggle("active", app.roiDrawing);
  $("roi-toggle-label").textContent = app.roiDrawing ? "drawing…" : "draw ROI";
  $("draw-palette").hidden = !app.roiDrawing;
  if (app.roiDrawing) paintPalette();
  applyMode();
}

export function wireDrawing() {
  applyLabelColours();
  $("roi-toggle").onclick = toggleDrawing;
}
