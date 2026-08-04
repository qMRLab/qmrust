// Pure mappings from a sim report, and from a page mode, to the values the UI
// draws. No DOM and no wasm here, so every rule below is unit-testable.
import { identityLabel } from "./dom.js";

// Which recipe a page mode edits. The two texts are held apart rather than
// re-derived from the payload on every switch, so a reader's edits to either
// side survive switching away and back.
export function recipeForMode(mode, { dataText, simText }) {
  return mode === "sim" ? simText : dataText;
}

// The four modes, in the order they answer questions about a model: what does
// it predict, can the fit recover it, where does it break down, how does it
// behave over a population.
export const SIM_MODES = [
  { id: "signal", label: "Signal" },
  { id: "single-voxel", label: "Voxel" },
  { id: "sensitivity", label: "Sensitivity" },
  { id: "montecarlo", label: "Multi-Voxel" },
];

// The stats table's rows. The core reports one entry per parameter its fitter
// actually estimates, so a parameter the model declares but does not report
// simply has no row.
export function statsRows(report) {
  return (report?.stats ?? []).map((s) => ({
    name: s.name,
    truth: s.truth,
    mean: s.mean,
    bias: s.bias,
    std: s.std,
    rmse: s.rmse,
  }));
}

// A sample's x-axis label: its own identity, formatted the same way a
// BIDS-resolved volume's is, or (for a sample the report gives no identity
// for) its position. Position is the honest fallback — a sample with no
// identity is still a sample, and numbering it says only where it sits. The
// core built the measurement, so the report's own `identities` are the only
// source; a loaded dataset's labels describe a different acquisition once the
// recipe has been edited, and must never stand in for these.
function axisLabels(count, identities = []) {
  return Array.from({ length: count }, (_, k) => {
    const id = identities[k];
    return id === undefined
      ? String(k + 1)
      : identityLabel(typeof id === "string" ? { role: id } : { params: id });
  });
}

export function signalSeries(report) {
  const values = report?.signal ?? [];
  return { values, labels: axisLabels(values.length, report?.identities) };
}

// Three curves: the noise-free signal at the truth, the first trial's noisy
// measurement, and the forward signal at what that trial fitted. Read together
// they answer whether the fit recovered the truth, which is the question the
// mode exists for. All three come from the report; none is recomputed here.
export function singleVoxelSeries(report) {
  const noisy = report?.noisy_signal ?? [];
  return {
    clean: report?.clean_signal ?? [],
    noisy,
    fitted: report?.fitted_signal ?? [],
    labels: axisLabels(noisy.length, report?.identities),
  };
}

// A sweep's parameters have wildly different magnitudes (a fraction near 0.15
// beside a rate near 25), so plotted against each other in the same physical
// units, one flattens the other. Reporting each in its own units instead, one
// panel per parameter, is qMRLab's own SimVaryPlot convention and is what
// removes the shared-axis problem: no normalisation is needed once each
// parameter draws on its own axis.
export function sensitivitySeries(report) {
  const points = report?.points ?? [];
  const swept = report?.swept_param;
  const names = (points[0]?.stats ?? []).map((s) => s.name);
  return {
    swept,
    params: names.map((name) => {
      const x = [];
      const mean = [];
      const std = [];
      const truth = [];
      for (const point of points) {
        const s = point.stats.find((entry) => entry.name === name);
        x.push(point.value);
        // A gap here is a missing measurement, not a missing point, so the
        // input value is kept and only this parameter's series stops: a
        // gap must never shift a later point's x.
        mean.push(s ? s.mean : null);
        std.push(s ? s.std : null);
        truth.push(s ? s.truth : null);
      }
      return { name, isSwept: name === swept, x, mean, std, truth };
    }),
  };
}

// A pair is usable only if both its input and what the fit recovered from it
// are finite numbers. `sim-worker.js` carries the report through `JSON.parse`,
// and serde writes a non-finite fit as JSON `null`; `null` arithmetics as 0,
// so an unfiltered subtraction or comparison turns a diverged fit into a
// fabricated, finite-looking value instead of dropping it.
function isUsablePair(input, fit) {
  return Number.isFinite(input) && Number.isFinite(fit);
}

// One series per reported parameter, pairing each trial's input with what the
// fit recovered from it. This is qMRLab's `Input vs. Fit` view: a scatter
// close to the diagonal is a fit that recovers what it was given. A trial
// whose fit diverged is dropped from its parameter's scatter entirely, rather
// than plotted from a fabricated value.
export function multiVoxelScatter(report) {
  const inputs = report?.per_trial_input ?? [];
  const fitted = report?.per_trial_fitted ?? [];
  const stats = report?.stats ?? [];
  if (!inputs.length || !fitted.length || !stats.length) return [];
  return stats.map((s, j) => ({
    name: s.name,
    points: inputs
      .map((row, t) => [row[j], fitted[t][j]])
      .filter(([input, fit]) => isUsablePair(input, fit)),
  }));
}

// One series per reported parameter, the trial-by-trial error the fit made
// (fitted minus input, never stored beside the pair it is derived from) plus
// the mean error actually observed. A trial whose fit diverged is dropped
// before the subtraction, not after: testing the difference for finiteness
// would still accept it, since a diverged fit crosses the JSON boundary as
// `null`, and `null - input` is the finite number `-input`.
export function multiVoxelErrors(report) {
  const inputs = report?.per_trial_input ?? [];
  const fitted = report?.per_trial_fitted ?? [];
  const stats = report?.stats ?? [];
  if (!inputs.length || !fitted.length || !stats.length) return [];
  return stats.map((s, j) => {
    const pairs = inputs.map((row, t) => [row[j], fitted[t][j]]);
    const errors = pairs.filter(([input, fit]) => isUsablePair(input, fit)).map(([input, fit]) => fit - input);
    const meanError = errors.length
      ? errors.reduce((a, b) => a + b, 0) / errors.length
      : 0;
    return { name: s.name, errors, dropped: pairs.length - errors.length, meanError };
  });
}

// The finite `[lo, hi]` of a value list, in one pass, or `null` when nothing in
// it is finite. Every axis range in the simulation charts starts here rather
// than at `Math.min(...values)`, which is wrong in three ways that all reach the
// charts: an all-diverged list yields `Infinity`/`-Infinity` instead of
// announcing that it has no range, a single non-finite entry poisons the result
// with NaN, and one spread argument per trial overflows the call stack once a
// run asks for enough trials. Returning `null` rather than a fallback keeps the
// choice of degenerate range with the panel that has to draw it.
export function extent(values) {
  let lo = Infinity;
  let hi = -Infinity;
  for (const v of values) {
    if (!Number.isFinite(v)) continue;
    if (v < lo) lo = v;
    if (v > hi) hi = v;
  }
  return lo <= hi ? [lo, hi] : null;
}

// A fixed bin count over a value's own range, matching qMRLab's `hist(x, 30)`.
// A degenerate (zero-width) range collapses to a single bin centred exactly on
// that value, rather than a padded range whose bin centres land near but not
// on it, so a parameter whose every trial lands on the same value gets a
// chart with its one bar where the value actually is.
export function errorHistogram(values, bins = 30) {
  const finite = values.filter(Number.isFinite);
  const span = extent(finite);
  if (!span) return { centers: [], counts: [] };
  const [lo, hi] = span;
  if (hi === lo) return { centers: [lo], counts: [finite.length] };
  const width = (hi - lo) / bins;
  const counts = new Array(bins).fill(0);
  for (const v of finite) {
    const idx = Math.min(bins - 1, Math.max(0, Math.floor((v - lo) / width)));
    counts[idx]++;
  }
  const centers = Array.from({ length: bins }, (_, k) => lo + width * (k + 0.5));
  return { centers, counts };
}

// The histogram's x extent, padded from the bin span the same way
// `sharedRange` pads a scatter, then widened to include both markLines the
// chart draws: zero error and the mean error actually observed. Without this
// a distribution that never crosses zero (every trial biased the same way)
// draws only one of the two promised reference lines.
export function histogramXRange(hist, meanError) {
  const centers = hist.centers;
  const refs = [0, meanError].filter(Number.isFinite);
  if (!centers.length) return extent(refs) ?? [-1, 1];
  const halfW = centers.length > 1
    ? (centers[1] - centers[0]) / 2
    : Math.abs(centers[0] || 1) * 0.05 + 0.01;
  const values = [centers[0] - halfW, centers[centers.length - 1] + halfW, ...refs];
  return extent(values) ?? [-1, 1];
}

// A sweep panel's y extent before padding: the errorbar spread of every point
// whose mean and standard deviation are both finite, widened to include the
// reference geometry the panel draws — the identity diagonal's own [lo, hi]
// span for the swept parameter, the constant truth for every other one.
// Without this the reference can sit outside a range built from the fit
// alone, which is exactly the regime (the fit breaking down) this mode exists
// to show. A point whose mean or std is not finite (a sweep step where the
// fit diverged) is left out of the extent and out of what is drawn: a
// diverged fit contributes nothing rather than a fabricated "zero spread".
export function sensitivityFinitePoints(param) {
  const keep = param.x
    .map((_, k) => k)
    .filter((k) => Number.isFinite(param.mean[k]) && Number.isFinite(param.std[k]));
  return {
    x: keep.map((k) => param.x[k]),
    mean: keep.map((k) => param.mean[k]),
    std: keep.map((k) => param.std[k]),
  };
}

export function sensitivityYExtent(param, finite) {
  const points = finite.mean.flatMap((m, k) => [m - finite.std[k], m + finite.std[k]]);
  if (param.isSwept) {
    const sweep = extent(param.x);
    if (sweep) points.push(...sweep);
  } else {
    const truth = param.truth.find((t) => Number.isFinite(t));
    if (truth !== undefined) points.push(truth);
  }
  return extent(points) ?? [0, 1];
}
