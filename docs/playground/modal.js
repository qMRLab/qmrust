// The two modals: a fitted map or an acquired image at a size the side-by-side
// layout cannot give it, and a sidecar's contents as text.
//
// One NiiVue instance, created on first open and reused by both image modals.
// Each holds a WebGL context and browsers cap those, so constructing one per
// open would leak contexts until the other viewers stopped rendering.
import { Niivue } from "./vendor/niivue.js";
import { icon } from "./vendor/icons.js";
import { $, cssColorToRgba, status } from "./dom.js";
import { app } from "./state.js";
import { volumeFromDataset } from "./nifti.js";
import { createLevelControl } from "./level.js";
import { clearMapHover, onMapHover } from "./curve.js";
import { highlightJson } from "./recipe.js";
import { clearVolumes } from "./viewers.js";
import { attachModalDrawing, detachModalDrawing } from "./draw.js";

async function ensureModalViewer() {
  if (app.nvModal) return;
  app.nvModal = new Niivue({
    isResizeCanvas: true,
    backColor: [0.02, 0.02, 0.03, 1],
    loadingText: "",
  });
  await app.nvModal.attachTo("gl-modal");
  app.nvModal.setHighResolutionCapable(true);
  app.nvModal.setCrosshairColor(cssColorToRgba("--rust"));
  app.nvModal.canvas.addEventListener("mousemove", onMapHover);
  app.nvModal.canvas.addEventListener("mouseleave", clearMapHover);
}


// The fitted map in a modal, at a size the side-by-side layout cannot give it.
export async function openMapModal() {
  // `shownOutput` is an `outputVolumes` entry, not a name — so the modal opens
  // whichever output the panel is showing rather than always the first.
  const shown =
    app.outputVolumes.find((o) => o.name === app.shownOutput?.name) ?? app.outputVolumes[0];
  if (!shown) return;
  $("file-modal").hidden = false;
  $("file-modal-title").textContent = `${shown.name}${shown.unit ? ` [${shown.unit}]` : ""}`;
  await ensureModalViewer();
  clearVolumes(app.nvModal);
  app.nvModal.addVolume(shown.volume);
  // Multiplanar whatever the data is: a single-slice fit still shows its one
  // plane, and the control that opens this is labelled Multiplanar.
  app.nvModal.setSliceType(app.nvModal.sliceTypeMultiplanar);
  // Only now: a drawing is bound to the background volume, so handing over the
  // pen and the strokes has to follow the volume they belong to. Doing it before
  // `addVolume` leaves the mirrored bitmap attached to a volume that has just
  // been replaced, and the strokes vanish.
  attachModalDrawing();
  app.nvModal.resizeListener();
  app.nvModal.drawScene();
  // A quantitative map is unreadable without a scale, so the modal always shows
  // one — with the histogram, so the window can be judged against the data.
  $("level").hidden = false;
  app.levelModal ??= createLevelControl("level");
  app.levelModal.open(shown, app.lastMaps?.[shown.name] ?? []);
  app.nvModal.resizeListener();
  app.nvModal.drawScene();
}

export async function openFileModal(path) {
  if (!app.dataset?.files?.has(path)) {
    status(`No data held for "${path}"`, "error");
    return;
  }
  $("file-modal").hidden = false;
  $("file-modal-title").textContent = path;
  // An acquired image has no quantitative scale to level, unlike a fitted map.
  $("level").hidden = true;
  await ensureModalViewer();
  detachModalDrawing();
  clearVolumes(app.nvModal);
  const volume = await volumeFromDataset(app.dataset.files, path, { colormap: "gray" });
  app.nvModal.addVolume(volume);
  // Slice type from the data, never from which dataset this is: a single-slice
  // volume has no second or third plane to show.
  const nz = volume.hdr?.dims?.[3] ?? 1;
  app.nvModal.setSliceType(
    nz > 1 ? app.nvModal.sliceTypeMultiplanar : app.nvModal.sliceTypeAxial,
  );
  app.nvModal.resizeListener();
  app.nvModal.drawScene();
}

export function closeFileModal() {
  $("file-modal").hidden = true;
}

// A sidecar's contents, syntax-highlighted. Reuses the recipe editor's
// highlighter rather than introducing a second one.
export function openJsonModal(path) {
  const bytes = app.dataset?.files?.get(path);
  if (!bytes) {
    status(`No data held for "${path}"`, "error");
    return;
  }
  let text = new TextDecoder().decode(bytes);
  try {
    // Re-indent, since a sidecar may arrive minified and unreadable.
    text = JSON.stringify(JSON.parse(text), null, 2);
  } catch {
    // Not valid JSON: show it verbatim, which is the honest thing when a
    // reader's own dataset holds a malformed sidecar.
  }
  $("json-modal-title").textContent = path;
  $("json-body").innerHTML = highlightJson(text);
  $("json-modal").hidden = false;
}

export function closeJsonModal() {
  $("json-modal").hidden = true;
}

// Something the reader has to be *told*, as opposed to the other two modals,
// which show them something. Used when a fetch fails in a way that changes what
// the page can offer — a dead archive host means no full datasets, and silently
// falling back to a built-in slice would misrepresent what they are looking at.
export function showNotice(iconName, title, body) {
  $("notice-icon").innerHTML = icon(iconName, 34);
  $("notice-title").textContent = title;
  $("notice-body").textContent = body;
  $("notice-modal").hidden = false;
}

export function closeNotice() {
  $("notice-modal").hidden = true;
}
