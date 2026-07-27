// The fitted-map panel, and what both viewers share: their size, their
// crosshair colour, and which output is on screen.
import { $, cssColorToRgba } from "./dom.js";
import { app, nvIn, nvOut } from "./state.js";
import { repaintLevels, setWindow } from "./level.js";
import { toggleRoi } from "./roi.js";
import { updateVoxelValue } from "./curve.js";

// Colormap per output unit, never per model/output name — mirrors
// scripts/docsfig/style.py's CMAP_BY_UNIT so the playground and the static
// figures agree. Only used as a *default*; the dropdown lets a reader
// override it with any colormap the vendored build actually ships.
const CMAP_BY_UNIT = { s: "magma", "1/s": "viridis", "%": "cividis", "": "inferno" };
const CANDIDATE_COLORMAPS = ["gray", "viridis", "magma", "inferno", "plasma", "hot", "cool", "turbo", "cividis"];

// Colormap names this NiiVue build actually provides, intersected with the
// candidates above.
let availableColormaps = [];

// The two viewer wraps are equal-width grid columns, stretched (CSS
// `align-items: stretch`) to the grid row's full height, with `.viewer`
// flexed to fill that in turn — so both viewers are always exactly the same
// size, whatever space the page has to give them. NiiVue fits a slice inside
// whatever box it is given, preserving the data's own aspect ratio (fit-to-
// contain, like an `object-fit: contain` image), so the box itself does not
// need to match the data's aspect ratio the way it used to.
export function sizeViewers() {
  nvIn.resizeListener();
  nvOut.resizeListener();
}

// Empty a viewer. NiiVue's `volumes` is the live array `removeVolume` mutates, so
// it must be copied before iterating.
export function clearVolumes(nv) {
  for (const v of [...nv.volumes]) nv.removeVolume(v);
}

// Every viewer's crosshair in the charts' accent colour, so a reader reads the
// image and the plots as one instrument.
export function applyCrosshairColor() {
  const rgba = cssColorToRgba("--rust");
  for (const nv of [nvIn, nvOut, app.nvModal]) {
    if (!nv) continue;
    nv.setCrosshairColor(rgba);
    nv.drawScene();
  }
}

// Slice or multiplanar, for the fitted map. Only meaningful for a volumetric
// map: a single-slice fit has no second or third plane, so the toggle disables
// itself rather than offering a view that would show two empty strips.
export function showMapView(view) {
  const planes = view === "planes";
  $("tab-slice").classList.toggle("active", !planes);
  $("tab-planes").classList.toggle("active", planes);
  $("tab-slice").setAttribute("aria-selected", String(!planes));
  $("tab-planes").setAttribute("aria-selected", String(planes));
  nvOut.setSliceType(planes ? nvOut.sliceTypeMultiplanar : nvOut.sliceTypeAxial);
  nvOut.drawScene();
}

// True when the fitted map has more than one slice to show.
export function mapIsVolumetric() {
  return (app.current?.meta?.dims?.[2] ?? 1) > 1;
}

export function syncMapViewControls() {
  const volumetric = mapIsVolumetric();
  const haveMap = app.outputVolumes.length > 0;
  $("tab-planes").disabled = !volumetric;
  $("tab-planes").title = volumetric
    ? "Show all three planes"
    : "This fit is a single slice — there are no other planes";
  $("open-map-modal").disabled = !haveMap;
  // Nothing to measure until a map exists.
  $("roi-toggle").disabled = !haveMap;
  if (!haveMap && app.roiDrawing) toggleRoi();
  if (!volumetric) showMapView("slice");
}

export function showOutput(name) {
  const entry = app.outputVolumes.find((o) => o.name === name);
  clearVolumes(nvOut);
  app.shownOutput = entry ?? null;
  if (!entry) return;
  nvOut.addVolume(entry.volume);
  const cmapSelect = $("colormap");
  const preferred = CMAP_BY_UNIT[entry.unit] ?? "inferno";
  const cmap = availableColormaps.includes(preferred) ? preferred : availableColormaps[0];
  if (availableColormaps.includes(cmap)) cmapSelect.value = cmap;
  nvOut.setColormap(entry.volume.id, cmapSelect.value || cmap);
  nvOut.drawScene();
  // Rebuild the histogram for whichever map is now shown — each output has its
  // own distribution, so the previous one's bins would misdescribe this window.
  app.levelMain?.open(entry, app.lastMaps?.[entry.name] ?? []);
  repaintLevels();
  if (app.current?.lastVox) {
    updateVoxelValue(app.current.lastVox.x, app.current.lastVox.y, app.current.lastVox.z);
  }
}

export function onColormapChange() {
  const entry = app.outputVolumes.find((o) => o.name === $("output").value) ?? app.outputVolumes[0];
  if (!entry) return;
  nvOut.setColormap(entry.volume.id, $("colormap").value);
  nvOut.drawScene();
  repaintLevels();
}

// Reader-adjustable colour scale: min/max feed `volume.cal_min`/`cal_max`
// directly (the same fields NiiVue's own auto windowing sets), so the
// colorbar — which reads off the volume — tracks the change for free.
export function resetCalRange() {
  if (!app.shownOutput) return;
  setWindow(app.shownOutput.defaultCalMin, app.shownOutput.defaultCalMax);
}

export function populateColormaps() {
  const select = $("colormap");
  select.replaceChildren();
  const have = new Set(nvOut.colormaps());
  availableColormaps = CANDIDATE_COLORMAPS.filter((c) => have.has(c));
  if (availableColormaps.length === 0) availableColormaps = ["gray"];
  for (const name of availableColormaps) {
    const opt = document.createElement("option");
    opt.value = name;
    opt.textContent = name;
    select.append(opt);
  }
}
