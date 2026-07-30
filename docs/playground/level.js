// Window/level control: the map's value histogram beside a colour bar whose two
// handles are the display window. The histogram is what makes a bad window
// diagnosable — you can see where the tissue sits relative to where the scale
// currently ends.
//
// One widget, instantiated per place it appears (the fitted-map panel and the
// modal), so both behave identically rather than drifting apart.
import { $, roundBound } from "./dom.js";
import { app, nvOut } from "./state.js";

// Repaint both instances. Every level control shows the same window, so anything
// that changes one changes them all.
export function repaintLevels() {
  app.levelMain?.paint();
  app.levelModal?.paint();
}

// The one place a display window is changed. Every control routes through here —
// the handles, the arrow keys and the typed bounds — so they cannot drift apart
// in what they update. `null` leaves that bound alone.
function setWindow(min, max) {
  if (!app.shownOutput) return;
  const vol = app.shownOutput.volume;
  if (min !== null && Number.isFinite(min)) vol.cal_min = min;
  if (max !== null && Number.isFinite(max)) vol.cal_max = max;
  // Both the plain property and its header twin: different NiiVue paths read
  // different ones (see `buildMapVolume`).
  vol.hdr.cal_min = vol.cal_min;
  vol.hdr.cal_max = vol.cal_max;
  nvOut.updateGLVolume();
  nvOut.drawScene();
  if (app.nvModal && app.nvModal.volumes.length) {
    app.nvModal.updateGLVolume();
    app.nvModal.drawScene();
  }
  repaintLevels();
}

const HIST_BINS = 64;
// How much of the distribution the scale spans. An unmasked fit puts division-by-
// near-zero voxels in the same array as tissue — the MTsat map of a maskless
// dataset runs to ±1e16 — and a domain taken from raw min/max makes the whole
// control useless: every real value collapses onto one pixel, so both handles
// land on the same spot and a drag resolves to ~1e16.
const DOMAIN_PCT = 1;
// Enough samples for a stable percentile without sorting a million-voxel map.
const DOMAIN_SAMPLES = 60000;
// How far past the window the bar reaches, as a multiple of the window's span.
// A percentile range alone is not enough: MTsat's robust range is still ~200 %
// wide against a 6 % window, which would leave the two handles a few pixels
// apart. Scaling to the window keeps the bar usable whatever the data does,
// while the robust range stays a hard outer bound so a drag can never reach 1e16.
const DOMAIN_MARGIN = 2;

// The `[lo, hi]` the scale should span: a robust percentile range of the finite
// values, estimated from a strided sample rather than a full sort.
function robustDomain(values) {
  const stride = Math.max(1, Math.floor(values.length / DOMAIN_SAMPLES));
  const sample = [];
  for (let i = 0; i < values.length; i += stride) {
    const v = values[i];
    if (Number.isFinite(v)) sample.push(v);
  }
  if (sample.length === 0) return null;
  sample.sort((a, b) => a - b);
  const at = (pct) => sample[Math.min(sample.length - 1, Math.floor((pct / 100) * sample.length))];
  return [at(DOMAIN_PCT), at(100 - DOMAIN_PCT)];
}

// Bin the finite values. NaN (unfitted) voxels are excluded: a histogram of "no
// data" says nothing about the window.
function buildHistogram(values, lo, hi) {
  const bins = new Float64Array(HIST_BINS);
  const span = hi - lo;
  if (!(span > 0)) return bins;
  for (let i = 0; i < values.length; i++) {
    const v = values[i];
    if (!Number.isFinite(v)) continue;
    // Clamped, not discarded: the domain is a robust range, so real values do
    // lie beyond it, and the end bins should show that they exist.
    let b = Math.floor(((v - lo) / span) * HIST_BINS);
    if (b < 0) b = 0;
    if (b >= HIST_BINS) b = HIST_BINS - 1;
    bins[b] += 1;
  }
  return bins;
}

// `prefix` names this instance's elements: `<prefix>-hist`, `-bar`, `-grad`,
// `-lo`, `-hi`, `-readout`.
export function createLevelControl(prefix) {
  const el = (part) => $(`${prefix}-${part}`);
  // { dataMin, dataMax, bins, unit, values } — the data behind the current
  // histogram. `values` is retained so the domain can be re-derived when the
  // window changes; it is the caller's array, not a copy.
  let data = null;

  // Re-derive the domain for the current window, over the same values.
  function rescale() {
    if (data && app.shownOutput) build(app.shownOutput, data.values);
  }

  const frac = (v) => (v - data.dataMin) / (data.dataMax - data.dataMin || 1);
  const value = (f) => data.dataMin + f * (data.dataMax - data.dataMin);

  function drawHistogram() {
    const canvas = el("hist");
    const ctx = canvas.getContext("2d");
    const { width: w, height: h } = canvas;
    ctx.clearRect(0, 0, w, h);
    if (!data || !app.shownOutput) return;

    // A log scale keeps a long tail visible: a background peak orders of
    // magnitude above the tissue would otherwise flatten everything else.
    const scaled = Array.from(data.bins, (c) => Math.log10(1 + c));
    const peak = Math.max(...scaled, 1e-9);
    const barH = h / HIST_BINS;
    const style = getComputedStyle(document.documentElement);
    const inWindow = style.getPropertyValue("--accent").trim() || "#38c0cf";
    const outWindow = style.getPropertyValue("--line").trim() || "#232c37";
    const span = data.dataMax - data.dataMin;
    const lo = app.shownOutput.volume.cal_min;
    const hi = app.shownOutput.volume.cal_max;

    for (let b = 0; b < HIST_BINS; b++) {
      const binLo = data.dataMin + (b / HIST_BINS) * span;
      const binHi = data.dataMin + ((b + 1) / HIST_BINS) * span;
      // Bins inside the window are highlighted, so the window's effect on the
      // data is visible rather than implied.
      ctx.fillStyle = binHi >= lo && binLo <= hi ? inWindow : outWindow;
      const len = (scaled[b] / peak) * (w - 2);
      // Low values at the bottom, matching the colour bar's orientation.
      ctx.fillRect(w - len, h - (b + 1) * barH, len, Math.max(1, barH - 0.5));
    }
  }

  function paint() {
    if (!app.shownOutput) {
      el("grad").style.background = "none";
      return;
    }
    const lut = nvOut.colormap($("colormap").value || "gray");
    const stops = [];
    for (let i = 0; i <= 32; i++) {
      const px = Math.round((i / 32) * 255) * 4;
      stops.push(`rgb(${lut[px]}, ${lut[px + 1]}, ${lut[px + 2]})`);
    }
    // Low value at the bottom, high at the top — the usual vertical convention,
    // matching the committed docsfig figures.
    el("grad").style.background = `linear-gradient(to top, ${stops.join(", ")})`;

    if (!data) return;
    for (const [part, v] of [
      ["lo", app.shownOutput.volume.cal_min],
      ["hi", app.shownOutput.volume.cal_max],
    ]) {
      const handle = el(part);
      handle.style.bottom = `${Math.max(0, Math.min(1, frac(v))) * 100}%`;
      handle.style.top = "auto";
      handle.style.transform = "translateY(50%)";
      handle.querySelector("span").textContent =
        `${roundBound(v)}${data.unit ? ` ${data.unit}` : ""}`;
      handle.setAttribute("aria-valuenow", String(roundBound(v)));
    }
    drawHistogram();
  }

  function beginDrag(which, event) {
    if (!data || !app.shownOutput) return;
    event.preventDefault();
    const handle = el(which);
    handle.classList.add("dragging");
    const move = (e) => {
      const r = el("bar").getBoundingClientRect();
      const f = Math.max(0, Math.min(1, (r.bottom - e.clientY) / r.height));
      const v = value(f);
      // The handles may not cross: a window needs a floor below its ceiling.
      const gap = (data.dataMax - data.dataMin) / 200;
      if (which === "lo") setWindow(Math.min(v, app.shownOutput.volume.cal_max - gap), null);
      else setWindow(null, Math.max(v, app.shownOutput.volume.cal_min + gap));
    };
    const up = () => {
      handle.classList.remove("dragging");
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      // Re-derive the domain now the window has settled, so dragging a handle to
      // the end of the bar and letting go opens up more range rather than
      // stopping there. Rescaling mid-drag would move the bar under the cursor.
      rescale();
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }

  // Arrow keys nudge a handle, so the window is reachable without a pointer.
  function onKey(which, event) {
    if (!data || !app.shownOutput) return;
    const step = (data.dataMax - data.dataMin) / 100;
    const delta = { ArrowUp: step, ArrowRight: step, ArrowDown: -step, ArrowLeft: -step }[
      event.key
    ];
    if (delta === undefined) return;
    event.preventDefault();
    const gap = (data.dataMax - data.dataMin) / 200;
    if (which === "lo") {
      setWindow(
        Math.min(app.shownOutput.volume.cal_min + delta, app.shownOutput.volume.cal_max - gap),
        null,
      );
    } else {
      setWindow(
        null,
        Math.max(app.shownOutput.volume.cal_max + delta, app.shownOutput.volume.cal_min + gap),
      );
    }
  }

  function onHover(event) {
    if (!data) return;
    const r = el("bar").getBoundingClientRect();
    const f = Math.max(0, Math.min(1, (r.bottom - event.clientY) / r.height));
    const readout = el("readout");
    readout.hidden = false;
    readout.style.bottom = `${f * 100}%`;
    readout.textContent = `${roundBound(value(f))}${data.unit ? ` ${data.unit}` : ""}`;
  }

  // Click a bound's label to type an exact value, for when dragging is not
  // precise enough. The input replaces the label in place and commits on Enter
  // or blur; Escape abandons it.
  function editBound(which) {
    if (!app.shownOutput || el(which).querySelector("input")) return;
    const handle = el(which);
    const label = handle.querySelector("span");
    const current =
      which === "lo" ? app.shownOutput.volume.cal_min : app.shownOutput.volume.cal_max;
    const input = document.createElement("input");
    input.type = "number";
    input.step = "any";
    input.className = "level-edit";
    input.value = String(roundBound(current));
    label.style.visibility = "hidden";
    handle.append(input);
    input.focus();
    input.select();
    const finish = (commit) => {
      if (commit) {
        const v = Number(input.value);
        if (Number.isFinite(v)) {
          if (which === "lo") setWindow(Math.min(v, app.shownOutput.volume.cal_max), null);
          else setWindow(null, Math.max(v, app.shownOutput.volume.cal_min));
        }
      }
      input.remove();
      label.style.visibility = "";
      paint();
    };
    input.addEventListener("keydown", (e) => {
      if (e.key === "Enter") finish(true);
      if (e.key === "Escape") finish(false);
      // A drag handler on the parent must not see these.
      e.stopPropagation();
    });
    input.addEventListener("blur", () => finish(true));
    // Typing in the input must not start a handle drag.
    input.addEventListener("pointerdown", (e) => e.stopPropagation());
  }
  for (const which of ["lo", "hi"]) {
    el(which).querySelector("span").addEventListener("pointerdown", (e) => {
      // `stopPropagation` keeps the handle's drag from starting; `preventDefault`
      // is what makes the edit survive — without it the gesture's default action
      // moves focus to the handle (it is `tabindex=0`), which blurs the new input
      // and commits it before a key can be typed.
      e.stopPropagation();
      e.preventDefault();
      editBound(which);
    });
  }

  el("lo").addEventListener("pointerdown", (e) => beginDrag("lo", e));
  el("hi").addEventListener("pointerdown", (e) => beginDrag("hi", e));
  el("lo").addEventListener("keydown", (e) => onKey("lo", e));
  el("hi").addEventListener("keydown", (e) => onKey("hi", e));
  el("bar").addEventListener("pointermove", onHover);
  el("bar").addEventListener("pointerleave", () => {
    el("readout").hidden = true;
  });

  // Derive the domain and histogram for one map, then paint.
  //
  // The domain is *not* the data's raw extent. An unmasked fit puts
  // division-by-near-zero voxels in the same array as tissue — MTsat on a
  // maskless dataset runs to ±1e16 — and a raw domain collapses every real value
  // onto one pixel, which put both handles on the same spot and made a drag
  // resolve to 1e16. Instead the bar reaches a couple of window-spans either
  // side of the current window, bounded by a robust percentile range: usable
  // whatever the data does, and never reachable to absurd values.
  function build(entry, values) {
    const robust = robustDomain(values);
    if (!robust) {
      data = null;
      drawHistogram();
      return;
    }
    const [rLo, rHi] = robust;
    const wLo = entry.volume.cal_min;
    const wHi = entry.volume.cal_max;
    const margin = Math.max(wHi - wLo, Number.EPSILON) * DOMAIN_MARGIN;
    // Never narrower than the window itself, or a handle would sit off the bar.
    let dataMin = Math.min(wLo, Math.max(rLo, wLo - margin));
    let dataMax = Math.max(wHi, Math.min(rHi, wHi + margin));
    if (!(dataMax > dataMin)) dataMax = dataMin + 1;

    data = {
      dataMin,
      dataMax,
      unit: entry.unit,
      values,
      bins: buildHistogram(values, dataMin, dataMax),
    };
    // Match the canvas backing store to its laid-out size, or the bars stretch.
    const canvas = el("hist");
    const box = canvas.getBoundingClientRect();
    if (box.width > 0) {
      canvas.width = Math.round(box.width);
      canvas.height = Math.max(80, Math.round(box.height));
    }
    paint();
  }

  return {
    // Mark where a value sits on the scale — `null` clears the marker. This is
    // what ties a point in the image to a number on the colour bar.
    mark(v) {
      const dot = el("dot");
      if (!data || v === null || !Number.isFinite(v)) {
        dot.hidden = true;
        return;
      }
      const f = frac(v);
      // Outside the data's own extent there is no place on the bar for it.
      if (f < 0 || f > 1) {
        dot.hidden = true;
        return;
      }
      dot.hidden = false;
      dot.style.bottom = `${f * 100}%`;
      dot.querySelector("span").textContent =
        `${roundBound(v)}${data.unit ? ` ${data.unit}` : ""}`;
    },
    // Rebuilding is `build` above, defined before the returned object so
    // `rescale` can reach it too.
    open: (entry, values) => build(entry, values),
    paint,
  };
}
