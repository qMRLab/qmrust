// Shared mutable state, and the two always-present viewers.
//
// Kept in one place because much of it is coupled, and the coupling is what goes
// wrong. Two invariants are load-bearing:
//
//  1. `current`, `lastMaps`, `outputVolumes` and `shownOutput` describe one fit
//     of one model. Whenever a model is (re)loaded they are reset together; a
//     partial update leaves the page reporting one model's fit over another's
//     data. `fitSlice` therefore wraps its display half in try/finally, so a
//     drawing failure cannot leave them half-updated with the UI disabled.
//  2. `dataset` is non-null exactly when a real BIDS dataset is loaded, and
//     `null` when the pre-baked demo slice is showing. The Files panel and the
//     file/JSON viewers all key off that distinction.
//
// Every field here is read or written by more than one module; anything only one
// module touches stays a `let` in that module.
import { Niivue } from "./vendor/niivue.js";

export const app = {
  // The wasm module, or null when it failed to load (viewers still work).
  wasm: null,

  // The loaded model and its data: { meta, volume (NVImage), maskU8, auxFlat,
  // auxVolumes, frame, lastVox }.
  current: null,

  // The fetched BIDS dataset behind `current`, when there is one:
  // { archive, files: Map<path, Uint8Array>, resolved: ResolvedCollection }.
  // `null` while the pre-baked demo slice is showing — see invariant 2.
  dataset: null,

  // True while a dataset is being fetched/extracted/resolved, so the tab switcher
  // leaves the skeletons in place instead of revealing a half-built view.
  loading: false,

  // A reader's own dropped dataset, waiting for the next `loadModel` to consume
  // instead of fetching. Cleared once taken, so switching models afterwards goes
  // back to the hosted datasets.
  droppedFiles: null,

  // Every model the payload index lists, for asking which of them can fit a
  // dropped dataset.
  modelNames: [],

  // The last fit's maps: { [outputName]: Float64Array } in C-order `[nx,ny,nz]` —
  // every entry fit_volume returns, i.e. the model's full output_names(), not just
  // the quantitative maps in meta.outputs.
  lastMaps: null,

  // The fitted maps as viewable volumes:
  // [{ name, unit, volume }].
  outputVolumes: [],

  // The `outputVolumes` entry currently drawn in the fitted-map viewer, and the one
  // every window/level and ROI action applies to.
  shownOutput: null,

  // Config keys restricted to a fixed set of values (dotted path -> allowed
  // values), from this model's bundle `enums` — the catalog/registry decides
  // which keys get a dropdown, never a name or key hard-coded here.
  enumFields: new Map(),

  // Accumulated wheel `deltaY` for the inputs viewer's frame-step gesture (see the
  // `wheel` listener in `wireInputsControls`); reset on every model load so a
  // gesture mid-flight on the old model cannot carry a partial step into the new
  // one.
  wheelAccum: 0,

  // Lazily created UI singletons. Each is null until first needed: the modal viewer
  // holds a rendering context, and the level controls bind to elements that must
  // exist first.
  nvModal: null, // the modal's NiiVue instance, reused across opens
  levelMain: null, // window/level control in the fitted-map panel
  levelModal: null, // window/level control in the modal

  // True while NiiVue's pen is active on the fitted map.
  roiDrawing: false,

  // A mask a segmentation method computed, on `current`'s own grid and in
  // `fit_volume`'s layout, or null when none has been asked for. It overrides the
  // mask the recipe resolves, so it is held apart from `current.maskU8`: the
  // recipe's own is kept in `current.resolvedMask` and restored when this clears.
  // Belongs to one grid, so a model load drops it.
  computedMask: null,

  // The segmentation exactly as its method produced it, before any shrink or
  // grow. The stepper always re-applies its net offset to this rather than to
  // the last result: morphology does not undo itself (a shrink followed by a
  // grow does not restore a lost thin structure), so compounding the passes
  // would make the control's own history part of the answer.
  segmentBase: null,
  // Net voxels the stepper has grown (positive) or shrunk (negative) the mask by.
  maskSteps: 0,

  // The selected model's full option surface from `effective_config`, or null
  // when wasm is unavailable or the recipe fails to deserialize — in which
  // case the form falls back to mirroring the recipe.
  surface: null,
  // Set by the slide-to-override control: the reader has deliberately unlocked
  // the sidecar-supplied fields for editing. Always reset on a model or dataset
  // load — an override is a decision about the dataset in front of you, and it
  // must not silently carry over to the next one.
  overrideProtocol: false,
  // Whether a resolved BIDS protocol is supplying the acquisition, which locks
  // the model's protocol keys. Read from the loaded dataset, never from the
  // recipe: the recipe's contents are the thing being rendered.
  protocolResolved: false,

  // Which activity the page is showing: "data" fits acquired volumes, "sim"
  // simulates from ground-truth parameters and reads no image data at all.
  pageMode: "data",
  // Which of the four simulations a run performs. Lives here rather than in
  // `sim.js` because the recipe form reads it too: a mode shows only the
  // settings its run consumes, per `SIM_MODES`.
  simMode: "single-voxel",
  // The recipe text for each page mode, held apart so a reader's edits to one
  // survive switching to the other and back. The live text is always
  // `editor.text`; these are the two parked copies.
  dataEditorText: "",
  simEditorText: "",
  // The noise kinds the payload index declares (e.g. none/gaussian/rician),
  // for the `sim.noise.type` dropdown. One list, read from the index alone.
  noiseKinds: [],

  // The loaded model's own parameters, each mapped to the unit the model
  // declares for it. A recipe key matching one of these is a symbol the model
  // itself defines, not an ordinary word, so the form prints it exactly rather
  // than title-casing it into a different symbol; the unit is the model's to
  // state, since the same-looking parameter carries different ones across
  // models. A parameter the model declares without a symbol entry maps to
  // `null`, and shows no unit rather than a guessed one.
  modelParams: new Map(),
};

// A misspelled field must not silently become a new one, leaving the real field
// forever unwritten. Sealing forbids adding or removing properties while leaving
// every field above writable, and module code is always strict — so `app.typo = x`
// throws instead of quietly doing nothing.
Object.seal(app);

// Recipe editor state: `text` is always the source of truth handed to
// fit_volume; `obj` is its parsed form (null while the text is invalid).
export const editor = { text: "", obj: null, valid: false };

// The inputs viewer shows an acquired image, so linear interpolation (the
// default) is the right look. The fitted-map viewer shows per-voxel
// quantitative values with hard NaN boundaries at the mask edge; blending
// those linearly with neighboring voxels invents in-between colours at the
// boundary that were never fitted, so it uses nearest-neighbour instead —
// every displayed pixel is then one real fitted value.
// `loadingText` is what NiiVue draws on an empty canvas (0 volumes) — blanked
// here since both viewers only ever go empty transiently (before the first
// volume/fit loads), not in a state a reader should read as "something is
// loading".
export const nvIn = new Niivue({
  isResizeCanvas: true,
  backColor: [0.02, 0.02, 0.03, 1],
  loadingText: "",
});
export const nvOut = new Niivue({
  isResizeCanvas: true,
  backColor: [0.02, 0.02, 0.03, 1],
  isNearestInterpolation: true,
  loadingText: "",
});
