// Pure mappings from a sim report, and from a page mode, to the values the UI
// draws. No DOM and no wasm here, so every rule below is unit-testable.

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
  { id: "sensitivity", label: "Sweep" },
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

// Positions are the honest fallback label: a sample the caller has no identity
// for is still a sample, and numbering it says only where it sits.
function axisLabels(count, labels = []) {
  return Array.from({ length: count }, (_, k) => labels[k] ?? String(k + 1));
}

export function signalSeries(report, labels = []) {
  const values = report?.signal ?? [];
  return { values, labels: axisLabels(values.length, labels) };
}

// Three curves: the noise-free signal at the truth, the first trial's noisy
// measurement, and the forward signal at what that trial fitted. Read together
// they answer whether the fit recovered the truth, which is the question the
// mode exists for. All three come from the report; none is recomputed here.
export function singleVoxelSeries(report, labels = []) {
  const noisy = report?.noisy_signal ?? [];
  return {
    clean: report?.clean_signal ?? [],
    noisy,
    fitted: report?.fitted_signal ?? [],
    labels: axisLabels(noisy.length, labels),
  };
}
