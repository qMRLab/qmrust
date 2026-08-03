// Simulate mode: the page's second activity. A simulation reads no image data,
// so this owns a recipe of its own (the model's sim recipe, carrying the
// acquisition plus a sim: block of ground-truth parameters) and a card of its
// own, and it works even when a dataset failed to load.
import { $, hideProgress, setProgress, showProgress, status } from "./dom.js";
import { app, editor } from "./state.js";
import { setEditorText } from "./recipe.js";
import { SIM_MODES, recipeForMode, statsRows } from "./sim-series.js";
import { showNotice } from "./modal.js";

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
  app.simReport = null;
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
  status(next === "sim" ? "Ready to simulate" : "Ready", "ok");
}

export function wireSimControls() {
  const flip = () => setPageMode(isSimMode() ? "data" : "sim");
  $("page-data").onclick = flip;
  $("page-sim").onclick = flip;
  buildModeTabs();
  setSimMode(app.simMode);
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
  const mode = app.simMode;
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
  app.simReport = report.report;
  renderSimReport(report.report);
  status(`Simulated in ${((performance.now() - t0) / 1000).toFixed(1)} s`, "ok");
}

function setSimMode(id) {
  app.simMode = id;
  for (const b of $("sim-modes").querySelectorAll("button")) {
    const on = b.dataset.mode === id;
    b.classList.toggle("active", on);
    b.setAttribute("aria-selected", String(on));
  }
  // A report answers one mode's question; it must not be read as another's.
  app.simReport = null;
  renderSimReport(null);
}

function renderSimReport(report) {
  renderStatsTable(report);
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
