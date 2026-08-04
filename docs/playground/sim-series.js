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
  { id: "montecarlo", label: "Population" },
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

// Linear-interpolated quantile of an already-sorted array, which is the
// convention ECharts' own boxplot transform uses.
function quantile(sorted, q) {
  if (!sorted.length) return NaN;
  const pos = (sorted.length - 1) * q;
  const lo = Math.floor(pos);
  const hi = Math.ceil(pos);
  return sorted[lo] + (sorted[hi] - sorted[lo]) * (pos - lo);
}

// One box per reported parameter, over the trial errors the report carries. The
// whiskers are the full range rather than 1.5 IQR: a fit that failed at a
// boundary is exactly what a reader is looking for here, and hiding it as an
// outlier would be the wrong default.
export function montecarloBoxes(report) {
  const inputs = report?.per_trial_input ?? [];
  const fitted = report?.per_trial_fitted ?? [];
  const stats = report?.stats ?? [];
  if (!inputs.length || !fitted.length || !stats.length) return [];
  const rows = inputs.map((row, t) => row.map((v, j) => fitted[t][j] - v));
  return stats.map((s, j) => {
    const sorted = rows.map((row) => row[j]).filter(Number.isFinite).sort((a, b) => a - b);
    return {
      name: s.name,
      box: [
        sorted[0],
        quantile(sorted, 0.25),
        quantile(sorted, 0.5),
        quantile(sorted, 0.75),
        sorted[sorted.length - 1],
      ],
      outliers: [],
    };
  });
}
