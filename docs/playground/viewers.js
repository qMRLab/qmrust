// The fitted-map panel, and what both viewers share: their size, their
// crosshair colour, and which output is on screen.
import { $, cssColorToRgba } from "./dom.js";
import { app, nvIn, nvOut } from "./state.js";
import { repaintLevels } from "./level.js";
import { isDrawing, toggleDrawing } from "./draw.js";
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

// Both viewer cards end in a foot of the same height, because that is what makes
// their two black boxes the same size: each box fills whatever its own foot
// leaves. The height is the taller foot's content, measured rather than declared
// — the inputs foot holds one slider, the fitted map's holds controls that wrap
// onto a second row in a narrow column, and a value fixed for the wrapped case
// leaves dead space under both cards in every other case.
//
// The feet themselves are clipped to this height, so it is their rows that are
// measured. Nothing here depends on card height, so setting it cannot feed back.
let footWatcher = null;

function syncFootHeight() {
  // A window resize narrows the columns without going through here, and that is
  // exactly when the fitted map's controls wrap. Watching the rows catches it,
  // and cannot loop: no row's size depends on the height this sets.
  footWatcher ??= new ResizeObserver(() => syncFootHeight());
  let tallest = 0;
  for (const foot of document.querySelectorAll(".viewer-foot")) {
    for (const row of foot.children) {
      footWatcher.observe(row);
      const { marginTop, marginBottom } = getComputedStyle(row);
      tallest = Math.max(
        tallest,
        row.offsetHeight + parseFloat(marginTop) + parseFloat(marginBottom),
      );
    }
  }
  if (tallest > 0) {
    document.documentElement.style.setProperty("--foot-h", `${Math.ceil(tallest)}px`);
  }
}

// The two viewer wraps are equal-width grid columns, stretched (CSS
// `align-items: stretch`) to the grid row's full height, with `.viewer`
// flexed to fill that in turn — so both viewers are always exactly the same
// size, whatever space the page has to give them. NiiVue fits a slice inside
// whatever box it is given, preserving the data's own aspect ratio (fit-to-
// contain, like an `object-fit: contain` image), so the box itself does not
// need to match the data's aspect ratio the way it used to.

export function sizeViewers() {
  syncFootHeight();
  nvIn.resizeListener();
  nvOut.resizeListener();
}

// Empty a viewer. NiiVue's `volumes` is the live array `removeVolume` mutates, so
// it must be copied before iterating.
export function clearVolumes(nv) {
  for (const v of [...nv.volumes]) nv.removeVolume(v);
}

// The viewers' share of the theme: crosshair in the charts' accent colour, so a
// reader takes the image and the plots as one instrument, and the canvas ground
// from `--viewer-bg`.
//
// `--viewer-bg` is near-black in every theme, including the light ones. A pale
// canvas behind a brain image shifts its apparent intensity, and dark is the
// radiological convention — so the token exists to be tuned, not lightened.
export function applyViewerTheme() {
  const crosshair = cssColorToRgba("--rust");
  const back = cssColorToRgba("--viewer-bg");
  for (const nv of [nvIn, nvOut, app.nvModal]) {
    if (!nv) continue;
    nv.setCrosshairColor(crosshair);
    // Read at draw time rather than through a setter, which this build lacks.
    nv.opts.backColor = back;
    nv.drawScene();
  }
}

// Slice or multiplanar, for the fitted map. Available whatever the data is: a
// single-slice fit shows its one plane in the multiplanar layout rather than
// having the view withheld.
export function showMapView(view) {
  const planes = view === "planes";
  $("tab-slice").classList.toggle("active", !planes);
  $("tab-planes").classList.toggle("active", planes);
  $("tab-slice").setAttribute("aria-selected", String(!planes));
  $("tab-planes").setAttribute("aria-selected", String(planes));
  nvOut.setSliceType(planes ? nvOut.sliceTypeMultiplanar : nvOut.sliceTypeAxial);
  nvOut.drawScene();
}

export function syncMapViewControls() {
  const haveMap = app.outputVolumes.length > 0;
  $("open-map-modal").disabled = !haveMap;
  // Nothing to measure until a map exists.
  $("roi-toggle").disabled = !haveMap;
  if (!haveMap && isDrawing()) toggleDrawing();
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
