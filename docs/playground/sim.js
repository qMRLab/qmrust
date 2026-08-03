// Simulate mode: the page's second activity. A simulation reads no image data,
// so this owns a recipe of its own (the model's sim recipe, carrying the
// acquisition plus a sim: block of ground-truth parameters) and a card of its
// own, and it works even when a dataset failed to load.
import { $, status } from "./dom.js";
import { app, editor } from "./state.js";
import { setEditorText } from "./recipe.js";
import { recipeForMode } from "./sim-series.js";

export function isSimMode() {
  return app.pageMode === "sim";
}

// The model's sim recipe, plus the two choices the sim block offers that are
// really fixed sets. Both are read from what the payload declares, never
// written down here: the noise kinds come from the index, and the sweepable
// parameters are the model's own parameter list.
export function seedSimRecipe(meta) {
  const canSim = Boolean(meta.config_sim);
  // A model whose payload cannot simulate must not leave the page stranded in
  // a mode it cannot serve. Leaving that mode first also keeps the departing
  // mode's park from writing the previous model's text over the seed below.
  if (!canSim && isSimMode()) setPageMode("data");
  app.simEditorText = meta.config_sim ?? "";
  app.simReport = null;
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
  ]) {
    $(id).hidden = hidden;
  }
  $("fit").textContent = next === "sim" ? "Simulate" : "Fit Data";
  setEditorText(
    recipeForMode(next, { dataText: app.dataEditorText, simText: app.simEditorText }),
  );
  status(next === "sim" ? "Ready to simulate" : "Ready", "ok");
}

export function wireSimControls() {
  const flip = () => setPageMode(isSimMode() ? "data" : "sim");
  $("page-data").onclick = flip;
  $("page-sim").onclick = flip;
}
