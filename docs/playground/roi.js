// ROI statistics. NiiVue's pen writes into a drawing bitmap the same shape as
// the volume, so "the voxels beneath the ROI" is the fitted map sampled where
// that bitmap is non-zero — no geometry of our own to get wrong. The pen itself
// lives in `draw.js`: this half only reports.
import { $, roundBound } from "./dom.js";
import { app, nvOut } from "./state.js";
import { describeValues } from "./stats.js";

// Sample the shown map wherever the drawing bitmap is set. NiiVue's bitmap is in
// the volume's own voxel order, which is the order `buildMapVolume` wrote — so
// index `i` is the same voxel in both.
function roiValues() {
  const bitmap = nvOut.drawBitmap;
  if (!bitmap || !app.shownOutput || !app.lastMaps) return [];
  const flat = app.lastMaps[app.shownOutput.name];
  if (!flat) return [];
  const [nx, ny, nz] = app.current.meta.dims;
  const out = [];
  for (let z = 0; z < nz; z++) {
    for (let y = 0; y < ny; y++) {
      for (let x = 0; x < nx; x++) {
        // The bitmap is NiiVue's x-fastest order; `lastMaps` is C-order
        // [nx,ny,nz] (see `readVolumeSeries`), so each needs its own index.
        if (bitmap[x + y * nx + z * nx * ny] === 0) continue;
        const v = flat[(x * ny + y) * nz + z];
        if (Number.isFinite(v)) out.push(v);
      }
    }
  }
  return out;
}

export function renderRoiStats() {
  const box = $("roi-stats");
  if (!app.roiDrawing && !nvOut.drawBitmap) {
    box.hidden = true;
    return;
  }
  box.hidden = false;
  const values = roiValues();
  box.replaceChildren();
  if (values.length === 0) {
    const empty = document.createElement("div");
    empty.className = "roi-empty";
    empty.textContent = "Draw on the map to measure a region.";
    box.append(empty);
    return;
  }
  const s = describeValues(values);
  const unit = app.shownOutput?.unit ? ` ${app.shownOutput.unit}` : "";
  const rows = [
    ["voxels", String(s.n)],
    ["mean", `${roundBound(s.mean)}${unit}`],
    ["SD", `${roundBound(s.sd)}${unit}`],
    ["median", `${roundBound(s.median)}${unit}`],
    ["min", `${roundBound(s.min)}${unit}`],
    ["max", `${roundBound(s.max)}${unit}`],
    ["IQR", `${roundBound(s.iqr)}${unit}`],
    ["Q1–Q3", `${roundBound(s.q1)} – ${roundBound(s.q3)}`],
  ];
  for (const [label, value] of rows) {
    const row = document.createElement("div");
    row.className = "roi-stat";
    const k = document.createElement("span");
    k.textContent = label;
    const val = document.createElement("b");
    val.textContent = value;
    row.append(k, val);
    box.append(row);
  }
}
