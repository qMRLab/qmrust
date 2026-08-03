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
  montecarloBoxes,
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
  $("page-sim").disabled = !canSim;
  $("page-sim").title = canSim
    ? ""
    : "This model's payload carries no sim recipe, so it cannot be simulated";
}

function setPageMode(mode) {
  const next = mode === "sim" ? "sim" : "data";
  if (next === "sim" && $("page-sim").disabled) return;
  // Leaving Simulate mode ends any run in flight: its result would otherwise
  // land under whatever the Data page goes on to show.
  if (app.pageMode === "sim" && next !== "sim") cancelSim({ announce: false });
  // Park the live text under the mode being left, so edits survive the switch.
  if (app.pageMode === "sim") app.simEditorText = editor.text;
  else app.dataEditorText = editor.text;

  app.pageMode = next;
  $("page-data").classList.toggle("active", next === "data");
  $("page-sim").classList.toggle("active", next === "sim");
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
  const flip = () => setPageMode(isSimMode() ? "data" : "sim");
  $("page-data").onclick = flip;
  $("page-sim").onclick = flip;
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

// One panel per reported parameter, each with its own y axis: a T1 error of
// 0.1 s and an `a` error of 12 cannot share a scale, and forcing them onto
// one would flatten the smaller to an invisible line. This is `measure.js`'s
// `chartOption` pattern (one grid/xAxis/yAxis triple per item, laid out
// across the width as percentages), applied to one box per parameter instead
// of one box per region. `baseOption` builds a single shared-axis option and
// is kept as-is for the other three modes; this mode needs its own shape
// rather than a forced fit, so it gets its own builder.
function montecarloOption(p, boxes) {
  const cols = boxes.length || 1;
  const hues = LABELS.map((l) => l.color);
  const axisText = { color: p.muted, fontSize: 10 };
  return {
    animation: false,
    tooltip: {
      trigger: "item",
      backgroundColor: p.panel,
      borderColor: p.line,
      textStyle: { color: p.ink, fontSize: 11 },
    },
    grid: boxes.map((_, i) => ({
      left: `${4 + (i * 96) / cols}%`,
      width: `${96 / cols - 6}%`,
      top: 30,
      bottom: 30,
    })),
    xAxis: boxes.map((_, i) => ({
      gridIndex: i,
      type: "category",
      data: [""],
      axisLine: { lineStyle: { color: p.line } },
      axisTick: { show: false },
      axisLabel: { show: false },
    })),
    yAxis: boxes.map((b, i) => ({
      gridIndex: i,
      type: "value",
      scale: true,
      name: `${b.name} error`,
      nameTextStyle: { ...axisText, align: "left" },
      axisLine: { lineStyle: { color: p.line } },
      axisLabel: axisText,
      splitLine: { lineStyle: { color: p.line, opacity: 0.45 } },
    })),
    series: boxes.map((b, i) => {
      const colour = hues[i % hues.length];
      return {
        type: "boxplot",
        xAxisIndex: i,
        yAxisIndex: i,
        data: [{
          value: b.box,
          itemStyle: { color: `${colour}55`, borderColor: colour, borderWidth: 1.4 },
        }],
        boxWidth: [12, 44],
        markLine: {
          silent: true,
          symbol: "none",
          label: { show: false },
          lineStyle: { color: p.line, type: "dashed" },
          data: [{ yAxis: 0 }],
        },
      };
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
    // One hue per parameter, from the app's own categorical set (the same one
    // draw.js and measure.js use for a fixed set of named items), so a sweep
    // reporting all six of qmt_spgr's parameters keeps every line distinct.
    // Cycles if a model ever reports more than the palette has.
    const hues = LABELS.map((l) => l.color);
    const series = [];
    for (const [i, param] of s.params.entries()) {
      const colour = hues[i % hues.length];
      // The band is two stacked series: an invisible lower edge, then its
      // height drawn as a faded area sitting on top. A point's lower edge is
      // routinely negative (bias minus a std larger than the bias), and
      // ECharts' default stacking keeps positive and negative stacks separate
      // rather than adding a positive height onto a negative base, which would
      // misplace the band above zero instead of straddling the line.
      // stackStrategy "all" makes the stack ignore sign so the two combine.
      series.push({
        name: `${param.name} band`,
        type: "line",
        stack: `band-${param.name}`,
        stackStrategy: "all",
        symbol: "none",
        lineStyle: { opacity: 0 },
        itemStyle: { opacity: 0 },
        tooltip: { show: false },
        silent: true,
        data: param.lower,
      });
      series.push({
        name: `${param.name} spread`,
        type: "line",
        stack: `band-${param.name}`,
        stackStrategy: "all",
        symbol: "none",
        lineStyle: { opacity: 0 },
        areaStyle: { color: colour, opacity: 0.14 },
        tooltip: { show: false },
        silent: true,
        data: param.band,
      });
      series.push({
        name: param.name,
        type: "line",
        showSymbol: true,
        symbolSize: 5,
        lineStyle: { color: colour, width: 2 },
        itemStyle: { color: colour },
        data: param.bias,
      });
    }
    // Zero bias is the line every parameter is being judged against, so it is
    // drawn rather than left to be inferred from the axis labels.
    if (series.length) {
      series[series.length - 1].markLine = {
        silent: true,
        symbol: "none",
        label: { show: false },
        lineStyle: { color: p.line, type: "dashed" },
        data: [{ yAxis: 0 }],
      };
    }
    ensureChart().setOption(
      {
        ...baseOption(p, {
          xName: report.swept_param,
          yName: "bias [% of truth]",
          xData: s.xLabels,
          legend: s.params.map((param) => param.name),
        }),
        series,
      },
      { notMerge: true },
    );
    $("sim-note").textContent =
      `Bias against ${report.swept_param}, as a percentage of the truth at each `
      + "point. The band is plus and minus one standard deviation over the trials.";
    return;
  }
  if (report.mode === "montecarlo") {
    const boxes = montecarloBoxes(report);
    ensureChart().setOption(montecarloOption(p, boxes), { notMerge: true });
    $("sim-note").textContent =
      `Error over ${report.trials} draws, per parameter. Whiskers span the full `
      + "range, so a fit that failed at a bound is visible rather than hidden.";
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
      td.textContent = typeof v === "number" ? String(Number(v.toPrecision(4))) : v;
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
