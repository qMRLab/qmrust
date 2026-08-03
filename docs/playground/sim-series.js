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
