// Simulate mode: the page's second activity. A simulation reads no image data,
// so this owns a recipe of its own (the model's sim recipe, carrying the
// acquisition plus a sim: block of ground-truth parameters) and a card of its
// own, and it works even when a dataset failed to load.
import { $, hideProgress, setProgress, showProgress, status } from "./dom.js";
import { app, editor } from "./state.js";
import { setEditorText, syncFitArmed } from "./recipe.js";
import {
  SIM_MODES,
  recipeForMode,
  statsRows,
  signalSeries,
  singleVoxelSeries,
  sensitivitySeries,
  multiVoxelScatter,
  multiVoxelErrors,
  errorHistogram,
  histogramXRange,
  sensitivityFinitePoints,
  sensitivityYExtent,
} from "./sim-series.js";
import { showNotice } from "./modal.js";
import { LABELS } from "./draw.js";
import * as echarts from "./vendor/echarts.js";

export function isSimMode() {
  return app.pageMode === "sim";
}

// The model's sim recipe, plus the two choices the sim block offers that are
// really fixed sets. Both are read from what the payload declares, never
// written down here: the noise kinds come from the index, and the sweepable
// parameters are the model's own parameter list.
export function seedSimRecipe(meta) {
  // A model load ends the run that was in flight for the previous model: its
  // report belongs to parameters that no longer exist on screen.
  cancelSim({ announce: false });
  const canSim = Boolean(meta.config_sim);
  // A model whose payload cannot simulate must not leave the page stranded in
  // a mode it cannot serve. Leaving that mode first also keeps the departing
  // mode's park from writing the previous model's text over the seed below.
  if (!canSim && isSimMode()) setPageMode("data");
  app.simEditorText = meta.config_sim ?? "";
  renderSimReport(null);
  if (canSim) {
    app.enumFields.set("sim.sweep.param", meta.params);
    if (app.noiseKinds?.length) app.enumFields.set("sim.noise.type", app.noiseKinds);
  }
  $("page-mode").disabled = !canSim;
  $("mode-switch").title = canSim
    ? ""
    : "This model's payload carries no sim recipe, so it cannot be simulated";
}

function setPageMode(mode) {
  const next = mode === "sim" ? "sim" : "data";
  if (next === "sim" && $("page-mode").disabled) return;
  // Leaving Simulate mode ends any run in flight: its result would otherwise
  // land under whatever the Data page goes on to show.
  if (app.pageMode === "sim" && next !== "sim") cancelSim({ announce: false });
  // Park the live text under the mode being left, so edits survive the switch.
  if (app.pageMode === "sim") app.simEditorText = editor.text;
  else app.dataEditorText = editor.text;

  app.pageMode = next;
  $("page-mode").checked = next === "sim";
  for (const [id, hidden] of [
    ["viewer-in-wrap", next === "sim"],
    ["viewer-out-wrap", next === "sim"],
    ["curve-wrap", next === "sim"],
    ["drop-wrap", next === "sim"],
    ["sim-wrap", next !== "sim"],
  ]) {
    $(id).hidden = hidden;
  }
  $("fit").textContent = next === "sim" ? "Simulate" : "Fit Data";
  setEditorText(
    recipeForMode(next, { dataText: app.dataEditorText, simText: app.simEditorText }),
  );
  if (next !== "sim") renderSimReport(null);
  syncFitArmed();
  status(next === "sim" ? "Ready to simulate" : "Ready", "ok");
}

export function wireSimControls() {
  $("page-mode").onchange = () => setPageMode($("page-mode").checked ? "sim" : "data");
  buildModeTabs();
  setSimMode(simMode);
}

// One worker, created on first use. A cancel terminates it, since a wasm call
// already in flight cannot be interrupted from outside; the next run makes a
// fresh one.
let worker = null;
let pending = 0;

function ensureWorker() {
  worker ??= new Worker(new URL("./sim-worker.js", import.meta.url), { type: "module" });
  return worker;
}

// A button click cancels a run the reader is watching, and announces it. A
// model or page-mode change aborts a run whose context just vanished; the
// caller sets its own status right after, so it stays silent rather than
// stomping (or being stomped by) that message.
function cancelSim({ announce = true } = {}) {
  if (!pending) return;
  worker?.terminate();
  worker = null;
  pending = 0;
  hideProgress();
  setRunning(false);
  if (announce) status("Simulation cancelled", "info");
}

// While a run is in flight the primary button cancels it: a reader who started
// a long sweep by accident should not have to reload the page.
function setRunning(running) {
  const btn = $("fit");
  btn.textContent = running ? "Cancel" : "Simulate";
  btn.classList.toggle("cancel", running);
  for (const b of $("sim-modes").querySelectorAll("button")) b.disabled = running;
}

export async function runSim() {
  if (!app.wasm) {
    status("Simulation unavailable: wasm failed to load", "error");
    return;
  }
  if (pending) {
    cancelSim();
    return;
  }
  if (!editor.valid) {
    status("Recipe YAML is invalid: fix it before simulating", "error");
    return;
  }
  const id = ++pending;
  const yaml = editor.text;
  const mode = simMode;
  status(`Simulating ${mode}…`, "busy");
  // No progress is available inside one wasm call, so the bar runs full with
  // its stripes moving: a busy indicator rather than a false measurement.
  showProgress();
  setProgress(100);
  setRunning(true);
  const t0 = performance.now();

  const report = await new Promise((resolve) => {
    const w = ensureWorker();
    w.onmessage = (event) => {
      if (event.data.id !== id) return;
      resolve(event.data);
    };
    w.onerror = (e) => resolve({ id, ok: false, error: e.message ?? "worker failed" });
    w.postMessage({ id, mode, yaml });
  });

  // A cancel between posting and resolving already reset everything.
  if (pending !== id) return;
  pending = 0;
  hideProgress();
  setRunning(false);
  if (!report.ok) {
    status("Simulation failed", "error");
    showNotice("triangle-alert", "Simulation failed", report.error);
    return;
  }
  renderSimReport(report.report);
  status(`Simulated in ${((performance.now() - t0) / 1000).toFixed(1)} s`, "ok");
}

// Which simulation a run performs: one of the four modes the core implements.
// Touched only by this module, so it stays a local rather than shared state.
let simMode = "single-voxel";

function setSimMode(id) {
  simMode = id;
  for (const b of $("sim-modes").querySelectorAll("button")) {
    const on = b.dataset.mode === id;
    b.classList.toggle("active", on);
    b.setAttribute("aria-selected", String(on));
  }
  // A report answers one mode's question; it must not be read as another's.
  renderSimReport(null);
}

let chart = null;
// The last drawn report, so a theme change can repaint in the new palette
// without re-running the simulation.
let drawn = null;

function ensureChart() {
  const host = $("sim-chart");
  if (!chart) {
    chart = echarts.init(host, null, { renderer: "canvas" });
    new ResizeObserver(() => chart.resize()).observe(host);
  }
  return chart;
}

export function resizeSimChart() {
  chart?.resize();
}

export function redrawSimChart() {
  if (drawn) drawSim(drawn);
}

// The palette every sim chart is drawn in, read from the theme's own tokens so
// a family change needs no second definition here.
function palette() {
  const style = getComputedStyle(document.documentElement);
  const read = (token) => style.getPropertyValue(token).trim();
  return {
    ink: read("--ink"),
    muted: read("--muted"),
    line: read("--line"),
    accent: read("--accent"),
    rust: read("--rust"),
    panel: read("--panel"),
  };
}

// Four significant figures, with the trailing zeros a fixed-precision string
// would carry stripped back off. The one place a report's raw numbers (which
// range from a fraction near 1e-5 to a rate near 30) become the compact text
// a table cell or an axis tick label shows.
function formatTick(v) {
  return String(Number(v.toPrecision(4)));
}

// The font every axis name renders in. The one place this is stated, read by
// both the style ECharts is told to use and the offscreen measurement below,
// so the two can never disagree.
const AXIS_NAME_FONT_SIZE = 10;

// A canvas 2D context measures glyphs the way the chart's own canvas renderer
// does. Character count is not a usable proxy in a proportional font: ten Ws
// and ten i's render at very different widths despite equal length. ECharts'
// default font family, which this option never overrides, is "sans-serif".
let measureCtx = null;
function textWidth(text, fontSize) {
  measureCtx ??= document.createElement("canvas").getContext("2d");
  measureCtx.font = `${fontSize}px sans-serif`;
  return measureCtx.measureText(text).width;
}

// Axes, tooltip and legend styling shared by every sim chart, so the four modes
// read as one family rather than four charts that happen to sit in one card.
function baseOption(p, { xName, yName, xData, legend = [] }) {
  return {
    animation: false,
    // The left inset has to fit the y-axis name in full. containLabel sizes
    // the grid to fit the numeric tick labels, whose width depends on the
    // data, but it reserves nothing for the axis name itself, so the name's
    // measured width is supplied as the floor containLabel builds on.
    grid: {
      left: Math.ceil(textWidth(yName, AXIS_NAME_FONT_SIZE)),
      right: 20,
      top: 30,
      bottom: 44,
      containLabel: true,
    },
    legend: legend.length
      ? { data: legend, top: 0, textStyle: { color: p.muted, fontSize: 11 }, inactiveColor: p.line }
      : undefined,
    tooltip: {
      trigger: "axis",
      backgroundColor: p.panel,
      borderColor: p.line,
      textStyle: { color: p.ink, fontSize: 11 },
      axisPointer: { type: "cross", label: { backgroundColor: p.muted } },
    },
    xAxis: {
      type: "category",
      data: xData,
      name: xName,
      nameLocation: "middle",
      nameGap: 26,
      nameTextStyle: { color: p.muted, fontSize: 10 },
      axisLine: { lineStyle: { color: p.line } },
      axisLabel: { color: p.muted, fontSize: 10, hideOverlap: true },
      axisTick: { lineStyle: { color: p.line } },
    },
    yAxis: {
      type: "value",
      scale: true,
      name: yName,
      nameTextStyle: { color: p.muted, fontSize: AXIS_NAME_FONT_SIZE, align: "right" },
      axisLine: { lineStyle: { color: p.line } },
      axisLabel: { color: p.muted, fontSize: 10 },
      splitLine: { lineStyle: { color: p.line, opacity: 0.45 } },
    },
  };
}

// A shared axis range for a scatter panel, from the full extent of both
// coordinates: an Input-vs-Fit diagonal only reads as 45 degrees when the x
// and y axes cover the same span. A degenerate (single-valued) parameter -
// qmt_spgr fixes R1f and R1r, so their input never varies and their fit
// matches it exactly - is padded around that one value instead of collapsing
// to a zero-width axis.
function sharedRange(values) {
  const lo = Math.min(...values);
  const hi = Math.max(...values);
  const span = hi > lo ? hi - lo : Math.abs(hi || 1) * 0.1 || 1;
  return hi > lo
    ? [lo - span / 20, hi + span / 20]
    : [lo - span / 2, hi + span / 2];
}

// Two rows of panels, one column per reported parameter: the top row is
// qMRLab's `Input vs. Fit` scatter, the bottom its `Error` histogram. Follows
// `sensitivityOption`'s one-grid-per-parameter layout, extended with a second
// row rather than a second scheme, since a T1 error in seconds and an `a`
// error in signal units cannot share an axis any more than their means could.
function montecarloOption(p, scatter, errors) {
  const cols = scatter.length || 1;
  const hues = LABELS.map((l) => l.color);
  const axisText = { color: p.muted, fontSize: 10 };
  const hists = errors.map((e) => errorHistogram(e.errors));
  const ranges = scatter.map((param) => sharedRange(param.points.flat()));

  const col = (i) => ({ left: `${4 + (i * 96) / cols}%`, width: `${96 / cols - 6}%` });

  return {
    animation: false,
    tooltip: {
      trigger: "item",
      backgroundColor: p.panel,
      borderColor: p.line,
      textStyle: { color: p.ink, fontSize: 11 },
    },
    grid: [
      ...scatter.map((_, i) => ({ ...col(i), top: "8%", bottom: "58%" })),
      ...scatter.map((_, i) => ({ ...col(i), top: "64%", bottom: "12%" })),
    ],
    xAxis: [
      ...scatter.map((param, i) => ({
        gridIndex: i,
        type: "value",
        min: ranges[i][0],
        max: ranges[i][1],
        name: `Input ${param.name}`,
        nameLocation: "middle",
        nameGap: 18,
        nameTextStyle: axisText,
        axisLine: { lineStyle: { color: p.line } },
        axisLabel: { ...axisText, hideOverlap: true, formatter: formatTick },
        axisTick: { lineStyle: { color: p.line } },
        splitLine: { show: false },
      })),
      ...hists.map((h, i) => {
        const [min, max] = histogramXRange(h, errors[i].meanError);
        return {
          gridIndex: cols + i,
          type: "value",
          min,
          max,
          name: `Error ${scatter[i].name}`,
          nameLocation: "middle",
          nameGap: 18,
          nameTextStyle: axisText,
          axisLine: { lineStyle: { color: p.line } },
          axisLabel: { ...axisText, hideOverlap: true, formatter: formatTick },
          axisTick: { lineStyle: { color: p.line } },
          splitLine: { show: false },
        };
      }),
    ],
    yAxis: [
      ...scatter.map((param, i) => ({
        gridIndex: i,
        type: "value",
        min: ranges[i][0],
        max: ranges[i][1],
        name: `Fitted ${param.name}`,
        nameTextStyle: { ...axisText, align: "left" },
        axisLine: { lineStyle: { color: p.line } },
        axisLabel: { ...axisText, formatter: formatTick },
        splitLine: { lineStyle: { color: p.line, opacity: 0.45 } },
      })),
      ...hists.map((h, i) => {
        const top = Math.max(1, ...h.counts);
        return {
          gridIndex: cols + i,
          type: "value",
          min: 0,
          max: top + Math.max(1, Math.ceil(top / 10)),
          name: "voxels",
          nameTextStyle: { ...axisText, align: "left" },
          axisLine: { lineStyle: { color: p.line } },
          axisLabel: axisText,
          splitLine: { lineStyle: { color: p.line, opacity: 0.45 } },
        };
      }),
    ],
    series: [
      ...scatter.map((param, i) => {
        const colour = hues[i % hues.length];
        return {
          name: param.name,
          type: "scatter",
          xAxisIndex: i,
          yAxisIndex: i,
          symbolSize: 5,
          itemStyle: { color: colour, opacity: 0.5 },
          data: param.points,
          markLine: {
            silent: true,
            symbol: "none",
            label: { show: false },
            lineStyle: { color: p.ink, type: "dashed" },
            data: [[{ coord: [ranges[i][0], ranges[i][0]] }, { coord: [ranges[i][1], ranges[i][1]] }]],
          },
        };
      }),
      ...hists.map((h, i) => {
        const colour = hues[i % hues.length];
        return {
          name: `${scatter[i].name} error`,
          type: "bar",
          xAxisIndex: cols + i,
          yAxisIndex: cols + i,
          barWidth: "100%",
          itemStyle: { color: colour, opacity: 0.6 },
          data: h.centers.map((c, k) => [c, h.counts[k]]),
          // Zero and the mean error need to read apart at a glance, since a
          // reader who cannot tell them apart cannot map the note's sentence
          // onto the picture: zero stays the family's dashed reference, the
          // mean gets the accent's own solid line and a label of its own.
          markLine: {
            silent: true,
            symbol: "none",
            data: [
              {
                xAxis: 0,
                label: { show: true, formatter: "zero", color: p.muted, fontSize: 9 },
                lineStyle: { color: p.ink, type: "dashed" },
              },
              {
                xAxis: errors[i].meanError,
                label: { show: true, formatter: "mean", color: p.rust, fontSize: 9 },
                lineStyle: { color: p.rust, type: "solid" },
              },
            ],
          },
        };
      }),
    ],
  };
}

// A vertical whisker from mean - std to mean + std, with short caps, drawn as
// a custom series: ECharts has no built-in error-bar series, and this is the
// qMRLab `errorbar(X, mean, std)` convention the mode is named for. Each datum
// is [x, mean, std]; the two thin-line alternative was passed over because a
// cap-and-whisker reads at a glance as "this is the spread", where two lines
// in the same hue as the mean line could be mistaken for two more series.
function errorBarRenderItem(colour) {
  return (_params, api) => {
    const x = api.value(0);
    const mean = api.value(1);
    const std = api.value(2);
    const top = api.coord([x, mean + std]);
    const bottom = api.coord([x, mean - std]);
    const halfCap = 5;
    const style = { stroke: colour, lineWidth: 1.4 };
    const line = (x1, y1, x2, y2) => ({ type: "line", shape: { x1, y1, x2, y2 }, style });
    return {
      type: "group",
      children: [
        line(top[0], top[1], bottom[0], bottom[1]),
        line(top[0] - halfCap, top[1], top[0] + halfCap, top[1]),
        line(bottom[0] - halfCap, bottom[1], bottom[0] + halfCap, bottom[1]),
      ],
    };
  };
}

// One panel per reported parameter, x and y both in the parameter's own
// physical units so no normalisation is needed to keep a fraction near 0.15
// legible beside a rate near 25. Follows `montecarloOption`'s layout: one
// grid/xAxis/yAxis triple per parameter, laid out across the width as
// percentages.
function sensitivityOption(p, s) {
  const cols = s.params.length || 1;
  const hues = LABELS.map((l) => l.color);
  const axisText = { color: p.muted, fontSize: 10 };
  const finites = s.params.map(sensitivityFinitePoints);
  const xRanges = s.params.map((param) => {
    const lo = Math.min(...param.x);
    const hi = Math.max(...param.x);
    const pad = (hi - lo) / 50;
    return [lo - pad, hi + pad];
  });
  const yRanges = s.params.map((param, i) => {
    const [lo, hi] = sensitivityYExtent(param, finites[i]);
    // A fixed parameter's std is exactly zero at every point and carries no
    // other reference, so lo === hi: an errorbar-free flat line still needs
    // headroom to sit inside its axis rather than pinned to a degenerate
    // zero-height range.
    const pad = hi > lo ? (hi - lo) / 10 : Math.abs(hi || 1) / 10;
    return [lo - pad, hi + pad];
  });
  return {
    animation: false,
    tooltip: {
      trigger: "item",
      backgroundColor: p.panel,
      borderColor: p.line,
      textStyle: { color: p.ink, fontSize: 11 },
    },
    grid: s.params.map((_, i) => ({
      left: `${4 + (i * 96) / cols}%`,
      width: `${96 / cols - 6}%`,
      top: 30,
      bottom: 44,
    })),
    xAxis: s.params.map((param, i) => ({
      gridIndex: i,
      type: "value",
      min: xRanges[i][0],
      max: xRanges[i][1],
      name: `Input ${s.swept}`,
      nameLocation: "middle",
      nameGap: 26,
      nameTextStyle: axisText,
      axisLine: { lineStyle: { color: p.line } },
      axisLabel: { ...axisText, hideOverlap: true, formatter: formatTick },
      axisTick: { lineStyle: { color: p.line } },
      // A category-dense sweep (many points, or many narrow columns) packs its
      // tick labels tighter than the panel can legibly show; capping the tick
      // count keeps every remaining label readable rather than relying on
      // hideOverlap alone to drop whichever ones collide.
      splitNumber: cols > 2 ? 4 : 6,
      splitLine: { show: false },
    })),
    yAxis: s.params.map((param, i) => ({
      gridIndex: i,
      type: "value",
      min: yRanges[i][0],
      max: yRanges[i][1],
      name: `Fitted ${param.name}`,
      nameTextStyle: { ...axisText, align: "left" },
      axisLine: { lineStyle: { color: p.line } },
      axisLabel: { ...axisText, formatter: formatTick },
      splitLine: { lineStyle: { color: p.line, opacity: 0.45 } },
    })),
    series: s.params.flatMap((param, i) => {
      const colour = hues[i % hues.length];
      const finite = finites[i];
      // The reference the panel promises: the identity diagonal spanning the
      // padded axis range for the swept parameter, the constant truth line for
      // every other one. Drawn ink-solid-dashed rather than the gridlines'
      // `--line` at reduced opacity, so it reads apart from the six-panel
      // splitLines rather than disappearing among them.
      const reference = param.isSwept
        ? { data: [[{ coord: [xRanges[i][0], xRanges[i][0]] }, { coord: [xRanges[i][1], xRanges[i][1]] }]] }
        : { data: [{ yAxis: param.truth.find((t) => Number.isFinite(t)) }] };
      return [
        {
          type: "custom",
          xAxisIndex: i,
          yAxisIndex: i,
          silent: true,
          renderItem: errorBarRenderItem(colour),
          data: finite.x.map((x, k) => [x, finite.mean[k], finite.std[k]]),
        },
        {
          name: param.name,
          type: "line",
          xAxisIndex: i,
          yAxisIndex: i,
          showSymbol: true,
          symbolSize: 6,
          lineStyle: { color: colour, width: 2 },
          itemStyle: { color: colour },
          data: param.x.map((x, k) => [x, param.mean[k]]),
          markLine: {
            silent: true,
            symbol: "none",
            label: { show: true, formatter: param.isSwept ? "identity" : "truth", color: p.muted, fontSize: 9 },
            lineStyle: { color: p.ink, type: "dashed" },
            ...reference,
          },
        },
      ];
    }),
  };
}

function drawSim(report) {
  drawn = report;
  const p = palette();
  if (report.mode === "signal") {
    const s = signalSeries(report);
    ensureChart().setOption(
      {
        ...baseOption(p, { xName: "acquisition", yName: "signal", xData: s.labels }),
        series: [{
          name: "predicted",
          type: "line",
          showSymbol: true,
          symbolSize: 6,
          lineStyle: { color: p.rust, width: 2 },
          itemStyle: { color: p.rust },
          data: s.values,
        }],
      },
      { notMerge: true },
    );
    $("sim-note").textContent =
      "Noise-free forward signal at the ground-truth parameters.";
    return;
  }
  if (report.mode === "single-voxel") {
    const s = singleVoxelSeries(report);
    ensureChart().setOption(
      {
        ...baseOption(p, {
          xName: "acquisition",
          yName: "signal",
          xData: s.labels,
          legend: ["truth", "noisy", "fitted"],
        }),
        series: [
          {
            name: "truth",
            type: "line",
            showSymbol: false,
            lineStyle: { color: p.muted, width: 1.5, type: "dashed" },
            itemStyle: { color: p.muted },
            data: s.clean,
          },
          {
            name: "noisy",
            type: "scatter",
            symbolSize: 7,
            itemStyle: { color: p.accent },
            data: s.noisy,
          },
          {
            name: "fitted",
            type: "line",
            showSymbol: false,
            lineStyle: { color: p.rust, width: 2 },
            itemStyle: { color: p.rust },
            data: s.fitted,
          },
        ],
      },
      { notMerge: true },
    );
    $("sim-note").textContent =
      `The first of ${report.trials} noisy trials, against the truth it was `
      + `generated from and the curve fitted back from it. The table summarises all ${report.trials} trials.`;
    return;
  }
  if (report.mode === "sensitivity") {
    const s = sensitivitySeries(report);
    ensureChart().setOption(sensitivityOption(p, s), { notMerge: true });
    $("sim-note").textContent =
      "Fitted value against the input that produced it, one panel per reported "
      + "parameter, each in its own units. A point on the diagonal is perfect "
      + "recovery; other parameters carry a horizontal line at their constant "
      + "truth. Error bars span the mean plus or minus one standard deviation.";
    return;
  }
  if (report.mode === "montecarlo") {
    const scatter = multiVoxelScatter(report);
    const errors = multiVoxelErrors(report);
    ensureChart().setOption(montecarloOption(p, scatter, errors), { notMerge: true });
    const dropped = Math.max(0, ...errors.map((e) => e.dropped));
    $("sim-note").textContent =
      `Input against fit over ${report.trials} voxels, per parameter, with each `
      + "parameter's error below it. The scatter's diagonal is perfect recovery; "
      + "the histogram's two lines are zero error and the mean error actually observed."
      + (dropped ? ` ${dropped} voxel${dropped === 1 ? "" : "s"} whose fit diverged are excluded.` : "");
    return;
  }
  $("sim-note").textContent = "";
}

function renderSimReport(report) {
  renderStatsTable(report);
  if (!report) {
    drawn = null;
    chart?.clear();
    $("sim-note").textContent = "";
    return;
  }
  drawSim(report);
}

function renderStatsTable(report) {
  const host = $("sim-table");
  const rows = statsRows(report);
  if (!rows.length) {
    host.replaceChildren();
    return;
  }
  const table = document.createElement("table");
  const head = document.createElement("tr");
  for (const h of ["param", "truth", "mean", "bias", "std", "RMSE"]) {
    const th = document.createElement("th");
    th.textContent = h;
    head.append(th);
  }
  table.append(head);
  for (const r of rows) {
    const tr = document.createElement("tr");
    for (const v of [r.name, r.truth, r.mean, r.bias, r.std, r.rmse]) {
      const td = document.createElement("td");
      td.textContent = typeof v === "number" ? formatTick(v) : v;
      tr.append(td);
    }
    table.append(tr);
  }
  host.replaceChildren(table);
}

function buildModeTabs() {
  const host = $("sim-modes");
  host.replaceChildren();
  for (const m of SIM_MODES) {
    const b = document.createElement("button");
    b.type = "button";
    b.dataset.mode = m.id;
    b.textContent = m.label;
    b.setAttribute("role", "tab");
    b.onclick = () => setSimMode(m.id);
    host.append(b);
  }
}
