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

// One series per reported parameter, pairing each trial's input with what the
// fit recovered from it. This is qMRLab's `Input vs. Fit` view: a scatter
// close to the diagonal is a fit that recovers what it was given.
export function multiVoxelScatter(report) {
  const inputs = report?.per_trial_input ?? [];
  const fitted = report?.per_trial_fitted ?? [];
  const stats = report?.stats ?? [];
  if (!inputs.length || !fitted.length || !stats.length) return [];
  return stats.map((s, j) => ({
    name: s.name,
    points: inputs.map((row, t) => [row[j], fitted[t][j]]),
  }));
}

// One series per reported parameter, the trial-by-trial error the fit made
// (fitted minus input, never stored beside the pair it is derived from) plus
// the two references the histogram's markLines need: zero error and the mean
// error actually observed. A trial whose fit diverged reports a non-finite
// error; it is dropped from the histogram, and counted rather than silently
// discarded, since a fit that failed is information rather than noise.
export function multiVoxelErrors(report) {
  const inputs = report?.per_trial_input ?? [];
  const fitted = report?.per_trial_fitted ?? [];
  const stats = report?.stats ?? [];
  if (!inputs.length || !fitted.length || !stats.length) return [];
  return stats.map((s, j) => {
    const all = inputs.map((row, t) => fitted[t][j] - row[j]);
    const errors = all.filter(Number.isFinite);
    const meanError = errors.length
      ? errors.reduce((a, b) => a + b, 0) / errors.length
      : 0;
    return { name: s.name, errors, dropped: all.length - errors.length, truthMean: 0, meanError };
  });
}

// A fixed bin count over a value's own range, matching qMRLab's `hist(x, 30)`.
// A degenerate (zero-width) range is padded rather than divided by, so a
// parameter whose every trial lands on the same value still gets a chart
// instead of a division by zero.
export function errorHistogram(values, bins = 30) {
  const finite = values.filter(Number.isFinite);
  if (!finite.length) return { centers: [], counts: [] };
  const lo = Math.min(...finite);
  const hi = Math.max(...finite);
  const span = hi > lo ? hi - lo : Math.abs(hi || 1) * 0.1 || 1;
  const min = hi > lo ? lo : lo - span / 2;
  const width = span / bins;
  const counts = new Array(bins).fill(0);
  for (const v of finite) {
    const idx = Math.min(bins - 1, Math.max(0, Math.floor((v - min) / width)));
    counts[idx]++;
  }
  const centers = Array.from({ length: bins }, (_, k) => min + width * (k + 0.5));
  return { centers, counts };
}
