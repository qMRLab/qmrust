// The single-voxel fit, as an interactive chart: measured points against the
// model's own forward curve. ECharts owns the axes, tooltip, legend and zoom, so
// none of that is hand-drawn here — and its theming is mapped onto this page's
// palette rather than shipping one of its own.
//
// `predicted[k]` may be NaN for a volume the fit excluded (mono_t2's dropped
// first echo, say); ECharts renders those as gaps, which is the honest picture.
import * as echarts from "./vendor/echarts.js";
import { $ } from "./dom.js";
import { app, editor, nvOut } from "./state.js";

// The ECharts instance for the voxel fit, created on first draw.
let curveChart = null;

// The last series drawn, so the chart can be repainted in new theme colours
// without re-running the fit. ECharts caches its option, so a colour change
// needs the option set again.
let lastSeries = null;

export function clearCurve() {
  curveChart?.clear();
  lastSeries = null;
}

// Repaint the current curve, picking up whatever the theme's tokens now say.
export function redrawCurve() {
  if (lastSeries) drawCurve(...lastSeries);
}

// The chart is sized by its flex parent, so a layout change has to tell it.
export function resizeCurve() {
  curveChart?.resize();
}

// Value of the currently-displayed map at `(x, y, z)`, shown beside the
// fitted-curve parameters — updates on every crosshair move, including while
// dragging (`onLocation` calls this on every `onLocationChange`).
export function updateVoxelValue(x, y, z, params = "") {
  const el = $("voxel-value");
  if (!app.shownOutput) {
    el.textContent = "";
    return;
  }
  const v = app.shownOutput.volume.getValue(x, y, z, 0);
  const unit = app.shownOutput.unit ? ` ${app.shownOutput.unit}` : "";
  el.textContent = Number.isFinite(v)
    ? `${app.shownOutput.name} = ${v.toPrecision(3)}${unit} at (${x}, ${y})${params ? ` · ${params}` : ""}`
    : `${app.shownOutput.name} = no fit at (${x}, ${y})`;
}

export function plotVoxel(x, y, z) {
  if (!app.current) return;
  app.current.lastVox = { x, y, z };
  const { meta, volume, auxVolumes } = app.current;
  const [nx, ny, nz, nt] = meta.dims;
  updateVoxelValue(x, y, z);
  if (!app.lastMaps) {
    $("curve-note").textContent = "fit the slice first, then click a voxel";
    clearCurve();
    return;
  }
  // `lastMaps` carries every entry `fit_volume` returns — the model's full
  // `output_names()`, including diagnostics that never make it into
  // `meta.outputs` (that list is filtered to quantitative maps only, for the
  // fitted-map viewer). A curve needs the fitted *parameters*, so the real
  // question is whether the fit result has them, not whether they happen to
  // be drawable maps.
  const idx = (x * ny + y) * nz + z;
  if (!meta.params.every((p) => p in app.lastMaps)) {
    const missing = meta.params.filter((p) => !(p in app.lastMaps));
    $("curve-note").textContent =
      `this model's fit does not report ${missing.join(", ")}, so no fitted ` +
      "curve is available for it.";
    clearCurve();
    return;
  }
  const params = meta.params.map((p) => app.lastMaps[p][idx]);
  if (params.some((v) => !Number.isFinite(v))) {
    $("curve-note").textContent = "no fit at that voxel (outside mask or fit failed)";
    clearCurve();
    return;
  }
  const measured = [];
  for (let t = 0; t < nt; t++) measured.push(volume.getValue(x, y, 0, t));
  // forward() takes one scalar per aux name (a single voxel), unlike
  // fit_volume's flat-array-per-name contract.
  const voxelAux = {};
  for (const [auxName, auxVolume] of Object.entries(auxVolumes)) {
    voxelAux[auxName] = auxVolume.getValue(x, y, z, 0);
  }
  const predicted = JSON.parse(
    app.wasm.forward(
      editor.text,
      Float64Array.from(params),
      JSON.stringify(voxelAux),
      meta.protocol_json ?? "",
    ),
  );
  // A `Series` model's own measurement doesn't necessarily cover every
  // acquired volume 1:1 — e.g. mono_t2's `drop_first_echo` excludes the
  // first echo from the fit entirely, so `forward()` returns one fewer
  // sample than `nt`. Blindly zipping that shorter array against `measured`
  // by position would silently shift every later point onto the wrong
  // volume. Instead, match each returned sample back to its volume by the
  // same identity (`meta.volume_ids`) the fit itself used, leaving NaN
  // (skipped when drawing) for any volume the fit excluded. `Named` models
  // already key their measurement by role name, which is exact.
  const values = Array.isArray(predicted)
    ? alignSeriesToVolumes(predicted, meta.volume_ids)
    : meta.volume_ids.map((r) => predicted[r]);
  drawCurve(measured, values, meta.labels);
  // The remaining fitted parameters join the displayed map's own value on one
  // line: the map is one of them, and the rest only mean anything beside it.
  $("curve-note").textContent = "";
  updateVoxelValue(
    x, y, z,
    meta.params
      .filter((p) => p !== app.shownOutput?.name)
      .map((p) => `${p} = ${app.lastMaps[p][idx].toPrecision(3)}`)
      .join(", "),
  );
}

// Deep-ish equality for one row of `Series` params against a `forward()`
// sample's own `params` — both are plain `{key: number}` objects, so a
// numeric-tolerant compare over the union of keys is enough.
function sameParams(a, b) {
  const keys = new Set([...Object.keys(a ?? {}), ...Object.keys(b ?? {})]);
  for (const k of keys) {
    const x = a?.[k];
    const y = b?.[k];
    if (typeof x !== "number" || typeof y !== "number") return false;
    if (Math.abs(x - y) > 1e-9 * Math.max(1, Math.abs(x), Math.abs(y))) return false;
  }
  return true;
}

function alignSeriesToVolumes(samples, volumeIds) {
  const values = new Array(volumeIds.length).fill(NaN);
  for (const sample of samples) {
    const t = volumeIds.findIndex((row) => sameParams(row, sample.params));
    if (t !== -1) values[t] = sample.value;
  }
  return values;
}

function ensureCurveChart() {
  const host = $("curve");
  if (!host) return null;
  if (!curveChart) {
    curveChart = echarts.init(host, null, { renderer: "canvas" });
    // The card is flex-sized, so the chart has to follow it rather than assume
    // the size it was created at.
    new ResizeObserver(() => curveChart.resize()).observe(host);
  }
  return curveChart;
}

function drawCurve(measured, predicted, labels = []) {
  const chart = ensureCurveChart();
  if (!chart) return;
  lastSeries = [measured, predicted, labels];
  const style = getComputedStyle(document.documentElement);
  const ink = style.getPropertyValue("--ink").trim();
  const muted = style.getPropertyValue("--muted").trim();
  const line = style.getPropertyValue("--line").trim();
  const accent = style.getPropertyValue("--accent").trim();
  const rust = style.getPropertyValue("--rust").trim();
  const panel = style.getPropertyValue("--panel").trim();

  chart.setOption(
    {
      animation: false,
      grid: { left: 56, right: 16, top: 28, bottom: 40, containLabel: false },
      legend: {
        data: ["measured", "fitted"],
        top: 0,
        textStyle: { color: muted, fontSize: 11 },
        inactiveColor: line,
      },
      tooltip: {
        trigger: "axis",
        backgroundColor: panel,
        borderColor: line,
        textStyle: { color: ink, fontSize: 11 },
        // The x label is the volume's own identity (e.g. "InversionTime=0.65"),
        // so a reader sees which acquisition a point belongs to.
        axisPointer: { type: "cross", label: { backgroundColor: muted } },
      },
      xAxis: {
        type: "category",
        data: labels.length ? labels : measured.map((_, k) => String(k + 1)),
        axisLine: { lineStyle: { color: line } },
        axisLabel: { color: muted, fontSize: 10, hideOverlap: true },
        axisTick: { lineStyle: { color: line } },
      },
      yAxis: {
        type: "value",
        scale: true,
        name: "signal",
        nameTextStyle: { color: muted, fontSize: 10, align: "right" },
        axisLine: { lineStyle: { color: line } },
        axisLabel: { color: muted, fontSize: 10 },
        splitLine: { lineStyle: { color: line, opacity: 0.45 } },
      },
      // Drag to zoom into a subset of the acquisition axis; useful for a
      // 30-echo series where the interesting part is the first few.
      dataZoom: [
        { type: "inside", zoomOnMouseWheel: "shift", moveOnMouseWheel: false },
      ],
      series: [
        {
          name: "measured",
          type: "scatter",
          symbolSize: 7,
          itemStyle: { color: accent },
          data: measured.map((v) => (Number.isFinite(v) ? v : null)),
        },
        {
          name: "fitted",
          type: "line",
          smooth: false,
          showSymbol: false,
          connectNulls: false,
          lineStyle: { color: rust, width: 2 },
          itemStyle: { color: rust },
          data: predicted.map((v) => (Number.isFinite(v) ? v : null)),
        },
      ],
    },
    { notMerge: true },
  );
}

export function onMapHover(event) {
  if (!app.shownOutput || !app.current) return;
  // While the pen is out, the pointer is painting rather than inspecting: a dot
  // chasing the cursor up and down the colour bar is noise, and the marker sits
  // exactly where a reader is watching their stroke land.
  if (app.roiDrawing) {
    app.levelMain?.mark(null);
    app.levelModal?.mark(null);
    return;
  }
  const nv = event.currentTarget === app.nvModal?.canvas ? app.nvModal : nvOut;
  // `canvasPos2frac` works in the canvas's backing-store pixels, which differ
  // from CSS pixels whenever the canvas is high-resolution. Deriving the ratio
  // from the canvas itself is right whatever the display's density.
  const rect = nv.canvas.getBoundingClientRect();
  if (!rect.width || !rect.height) return;
  const sx = nv.canvas.width / rect.width;
  const sy = nv.canvas.height / rect.height;
  const frac = nv.canvasPos2frac([
    (event.clientX - rect.left) * sx,
    (event.clientY - rect.top) * sy,
  ]);
  // `canvasPos2frac` reports a negative first component when the cursor is not
  // over a slice at all (the gaps in a multiplanar view, say).
  if (!frac || frac[0] < 0) {
    app.levelMain?.mark(null);
    app.levelModal?.mark(null);
    return;
  }
  const [x, y, z] = nv.frac2vox(frac);
  const v = app.shownOutput.volume.getValue(x, y, z, 0);
  app.levelMain?.mark(Number.isFinite(v) ? v : null);
  app.levelModal?.mark(Number.isFinite(v) ? v : null);
}

export function clearMapHover() {
  app.levelMain?.mark(null);
  app.levelModal?.mark(null);
}

export function onLocation(loc) {
  if (!app.current || !loc?.vox) return;
  // While the pen is out a click is a stroke, not a probe: re-running the voxel
  // fit on every dab would replace the curve a reader is working against, and
  // burn a `forward()` call per stroke.
  if (app.roiDrawing) return;
  const [x, y, z] = loc.vox.map((v) => Math.round(v));
  const [nx, ny, nz] = app.current.meta.dims;
  if (x < 0 || y < 0 || z < 0 || x >= nx || y >= ny || z >= nz) return;
  plotVoxel(x, y, z);
  // Mark the crosshair's own value too. These voxel indices are NiiVue's, so
  // this path needs no coordinate conversion of ours.
  if (app.shownOutput && !app.roiDrawing) {
    const v = app.shownOutput.volume.getValue(x, y, z, 0);
    app.levelMain?.mark(Number.isFinite(v) ? v : null);
    app.levelModal?.mark(Number.isFinite(v) ? v : null);
  }
}
