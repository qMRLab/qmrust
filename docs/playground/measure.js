// What the drawn regions measure, as a table.
//
// The numbers are the product of the drawing, so this half owns none of the
// input: `draw.js` decides what a region *is*, `stats.js` computes it, and this
// only decides how to show it. Statistics always come from the fitted maps
// (`app.lastMaps`) rather than from whichever volume was drawn on — drawing on
// anatomy and measuring T1 is the intended workflow.
import { $, roundBound } from "./dom.js";
import { app, nvOut } from "./state.js";
import { labelStats } from "./stats.js";
import { LABELS } from "./draw.js";

// One row per label, one column per statistic, for the map on screen. "All maps"
// trades the detail for a column per output, which is the comparative reading a
// single-map table cannot give: white matter against grey across T1, T2 and M0.
let allMaps = false;

// Names are presentation, not data: the drawing stores label *values*, so a name
// belongs to this module and never to the bitmap.
const names = new Map();

function labelName(value) {
  return names.get(value) ?? `label ${value}`;
}

// Every map the fit produced that is also a declared output, in the order the
// model lists them — so the columns read the way the model's own documentation
// does rather than in Map-insertion order.
function maps() {
  const outputs = app.current?.meta?.outputs ?? [];
  return outputs
    .filter((o) => app.lastMaps?.[o.name])
    .map((o) => ({ name: o.name, unit: o.unit, flat: app.lastMaps[o.name] }));
}

function shownMap() {
  const all = maps();
  return all.find((m) => m.name === app.shownOutput?.name) ?? all[0] ?? null;
}

// The drawing to measure. It is mirrored across every viewer, so the fitted-map
// one is asked: it is the viewer that must hold a map for these numbers to mean
// anything in the first place.
function bitmap() {
  return nvOut.drawBitmap ?? null;
}

// `[{ value, colour, name, stats }]` for whichever labels the drawing holds, in
// label order. A label with no measurable voxel is absent — `labelStats` drops it
// rather than reporting a NaN mean.
function rows(map) {
  if (!map || !app.current) return [];
  const stats = labelStats(bitmap(), map.flat, app.current.meta.dims);
  return [...stats.entries()]
    .sort((a, b) => a[0] - b[0])
    .map(([value, s]) => ({
      value,
      colour: LABELS.find((l) => l.value === value)?.color ?? "#888",
      name: labelName(value),
      stats: s,
    }));
}

function cell(text, className) {
  const el = document.createElement(className === "th" ? "th" : "td");
  if (className && className !== "th") el.className = className;
  el.textContent = text;
  return el;
}

// A swatch plus an editable name. Editing in place rather than through a dialog:
// naming a region is part of reading it, and a quoted number needs a referent
// better than "3".
function nameCell(row) {
  const td = document.createElement("td");
  td.className = "measure-label";
  const chip = document.createElement("i");
  chip.className = "measure-chip";
  chip.style.background = row.colour;
  const name = document.createElement("span");
  name.className = "measure-name";
  name.textContent = row.name;
  name.title = "Click to rename";
  name.contentEditable = "plaintext-only";
  name.spellcheck = false;
  const commit = () => {
    const text = name.textContent.trim();
    if (text) names.set(row.value, text);
    else name.textContent = labelName(row.value);
  };
  name.addEventListener("blur", commit);
  name.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      e.preventDefault();
      name.blur();
    }
    if (e.key === "Escape") {
      name.textContent = labelName(row.value);
      name.blur();
    }
  });
  td.append(chip, name);
  return td;
}

function renderOneMap(table, map) {
  const head = document.createElement("tr");
  head.append(
    cell("region", "th"),
    cell("n", "th"),
    cell("mean", "th"),
    cell("SD", "th"),
    cell("median", "th"),
    cell("Q1–Q3", "th"),
    cell("min–max", "th"),
  );
  table.append(head);
  for (const row of rows(map)) {
    const { stats: s } = row;
    const tr = document.createElement("tr");
    tr.append(
      nameCell(row),
      cell(String(s.n)),
      cell(String(roundBound(s.mean))),
      cell(String(roundBound(s.sd))),
      cell(String(roundBound(s.median))),
      cell(`${roundBound(s.q1)} – ${roundBound(s.q3)}`),
      cell(`${roundBound(s.min)} – ${roundBound(s.max)}`),
    );
    table.append(tr);
  }
}

function renderAllMaps(table, all) {
  const head = document.createElement("tr");
  head.append(cell("region", "th"), cell("n", "th"));
  for (const m of all) head.append(cell(`${m.name}${m.unit ? ` [${m.unit}]` : ""}`, "th"));
  table.append(head);
  // Every column measures the same voxels, so `n` is taken once from the first
  // map — a differing count would mean the maps disagree about where the fit
  // succeeded, which is worth seeing rather than hiding.
  for (const row of rows(all[0])) {
    const tr = document.createElement("tr");
    tr.append(nameCell(row), cell(String(row.stats.n)));
    for (const m of all) {
      const s = labelStats(bitmap(), m.flat, app.current.meta.dims).get(row.value);
      tr.append(cell(s ? `${roundBound(s.mean)} ±${roundBound(s.sd)}` : "—"));
    }
    table.append(tr);
  }
}

export function renderMeasurements() {
  const box = $("measure-table");
  const summary = $("measure-summary");
  box.replaceChildren();
  const all = maps();
  const map = shownMap();
  const present = rows(map);

  $("measure-scope").textContent = allMaps ? "One map" : "All maps";
  $("measure-scope").disabled = all.length < 2;

  if (!map) {
    summary.textContent = "Fit a slice first — the statistics come from the fitted maps.";
    return;
  }
  if (present.length === 0) {
    summary.textContent = "Draw a region to measure it.";
    return;
  }
  const voxels = present.reduce((n, r) => n + r.stats.n, 0);
  summary.textContent = allMaps
    ? `${present.length} region${present.length === 1 ? "" : "s"} · ${voxels} voxels · mean ±SD per map`
    : `${present.length} region${present.length === 1 ? "" : "s"} · ${voxels} voxels · ${map.name}${map.unit ? ` [${map.unit}]` : ""}`;

  const table = document.createElement("table");
  if (allMaps && all.length > 1) renderAllMaps(table, all);
  else renderOneMap(table, map);
  box.append(table);
}

export function showMeasureView(view) {
  const measuring = view === "measurements";
  $("tab-curve").classList.toggle("active", !measuring);
  $("tab-measure").classList.toggle("active", measuring);
  $("tab-curve").setAttribute("aria-selected", String(!measuring));
  $("tab-measure").setAttribute("aria-selected", String(measuring));
  $("curve-view").hidden = measuring;
  $("measure-view").hidden = !measuring;
  if (measuring) renderMeasurements();
}

export function toggleMeasureScope() {
  allMaps = !allMaps;
  renderMeasurements();
}

// The table as it stands, for pasting into whatever a reader writes up. Names and
// units travel with it: a column of numbers without either is not a measurement.
export function copyCsv() {
  const table = $("measure-table").querySelector("table");
  if (!table) return;
  const csv = [...table.rows]
    .map((tr) =>
      [...tr.cells]
        .map((td) => {
          const text = td.textContent.trim();
          return /[",]/.test(text) ? `"${text.replace(/"/g, '""')}"` : text;
        })
        .join(","),
    )
    .join("\n");
  navigator.clipboard?.writeText(csv);
}
