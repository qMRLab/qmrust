// Loading a model's data: a real BIDS dataset when the payload names one,
// the pre-baked slice otherwise. Both paths end in the same place — `app.current`
// holding the volume, mask and auxiliary inputs a fit needs — so everything
// downstream is blind to where the data came from.
import { NVImage } from "./vendor/niivue.js";
import { $, status } from "./dom.js";
import { app, editor, nvIn, nvOut } from "./state.js";
import { loadBundle } from "./bundles.js";
import { fetchDataset, identityLabel, stage } from "./dataset.js";
import { setEditorText } from "./recipe.js";
import {
  buildSeriesNifti,
  readMask,
  readScalarVolume,
  volumeFromDataset,
} from "./nifti.js";
import {
  endLoading,
  renderFiles,
  renderFilesError,
  setFrameUi,
  showInputsTab,
  showLoading,
} from "./inputs.js";
import { clearVolumes, linkViewers, sizeViewers, syncMapViewControls } from "./viewers.js";
import { resetDrawing } from "./draw.js";
import { clearComputedMask, syncSegmentButton } from "./segment.js";
import { clearLabelNames } from "./measure.js";
import { repaintLevels } from "./level.js";
import { clearCurve } from "./curve.js";
import { showNotice } from "./modal.js";

// Both viewers back to a single axial slice, sized to whatever the new data is.
// The fitted-map viewer is reset too: its previous model's map is gone, and a
// multiplanar view left over from a volumetric fit would otherwise persist onto
// single-slice data.
function resetViewers() {
  nvIn.setSliceType(nvIn.sliceTypeAxial);
  nvOut.setSliceType(nvOut.sliceTypeAxial);
  sizeViewers();
  nvIn.drawScene();
}

// Fetch → unzip → resolve → load → ready to fit. Returns false when the dataset
// holds nothing this model can fit, having already explained that in the panel;
// throws when the fetch or the archive itself failed.
async function loadModelFromBids(name, meta) {
  const ds = await fetchDataset(name, meta);
  if (ds.collections.length === 0) {
    const files = app.wasm.annotate_non_matching(ds.files, meta.config_bids);
    app.dataset = { ...ds, resolved: { files } };
    renderFiles();
    $("files-summary").textContent =
      `${ds.archive} · ${files.length} files · nothing this model can fit`;
    showInputsTab("files");
    status(`No ${meta.title} data in that dataset: see Files for why`, "error");
    return false;
  }

  const resolved = ds.collections[0];
  app.dataset = { ...ds, resolved };
  // A resolved collection means the sidecars supply the acquisition, so the
  // model's protocol keys are context, not controls.
  app.protocolResolved = Boolean(resolved.protocol_json);
  // The recipe for BIDS input carries options only; the acquisition comes from
  // the sidecars, via `resolved.protocol_json`.
  setEditorText(meta.config_bids);

  stage(`Loading ${resolved.data_files.length} volumes…`);
  const parts = resolved.data_files.map((path) => {
    const bytes = ds.files.get(path);
    if (!bytes) throw new Error(`resolution named "${path}", which the archive does not hold`);
    return bytes;
  });
  const { bytes, nt } = buildSeriesNifti(parts);
  const volume = await NVImage.loadFromFile({
    file: new File([bytes], `${meta.model}.nii`),
    name: meta.model,
    colormap: "gray",
    colorbarVisible: false,
  });
  nvIn.addVolume(volume);
  // The grid every voxel index in this app refers to is the RAS one, not the
  // file's own dim order: reads go through `getValue`, which takes RAS indices,
  // and NiiVue's crosshair and drawing bitmap speak the same space. The two
  // differ whenever the affine stores the axes in another order.
  const [nx, ny, nz] = volume.dimsRAS.slice(1, 4);

  let maskU8 = null;
  if (resolved.mask_file) {
    maskU8 = readMask(await volumeFromDataset(ds.files, resolved.mask_file), nx, ny, nz);
  }
  // A failure here propagates: the caller falls back to the pre-baked slice, which
  // is the honest outcome when a hosted dataset is unreadable.
  const auxFlat = {};
  const auxVolumes = {};
  for (const [auxName, path] of resolved.aux_files) {
    const auxVolume = await volumeFromDataset(ds.files, path);
    auxFlat[auxName] = Array.from(readScalarVolume(auxVolume, nx, ny, nz));
    auxVolumes[auxName] = auxVolume;
  }

  const ids = JSON.parse(resolved.volume_ids_json);
  const labels = ids.map((id) =>
    identityLabel(typeof id === "string" ? { role: id } : { params: id }),
  );
  // A BIDS-resolved view of this model: the model's own facts come from the
  // payload, everything data-derived from resolution.
  const bidsMeta = {
    ...meta,
    dims: [nx, ny, nz, nt],
    labels,
    volume_ids: ids,
    protocol_json: resolved.protocol_json,
    config: meta.config_bids,
  };

  resetViewers();
  setFrameUi(nt, labels);
  // `resolution` is what these inputs were built from, so a later fit can tell
  // whether the recipe has since changed its mind about them.
  app.current = {
    meta: bidsMeta, volume, maskU8, auxFlat, auxVolumes, frame: 0, resolution: resolved,
  };
  renderFiles();
  syncMapViewControls();
  syncSegmentButton();
  $("fit").disabled = false;
  for (const w of resolved.warnings) status(w, "error");
  status("Ready", "ok");
  return true;
}

// The recipe governs input *resolution*, not only fitting options: its `mask:`
// and auxiliary selectors decide which of the dataset's files the model consumes.
// `resolve_bids` reads them, and it ran when the dataset was loaded — so an edit
// to those selectors means nothing until resolution runs again. Fitting is where
// that happens, which is the recipe's own stated contract: edits apply to the
// next fit.
//
// Only what resolution owns is rebuilt — the mask, the auxiliary inputs, and the
// acquisition the sidecars declare. The loaded series is left alone unless the
// recipe now names different volumes: that is a change of input rather than of
// options, so it is reported instead of fitting one series under another's
// resolution.
//
// Returns null when the inputs are ready, or the reason a fit must not proceed.
// The pre-baked slice resolves nothing — its mask is named by the payload — and
// it is also what a dataset that failed to load falls back to, so the test is
// whether *these* inputs came from resolution, not whether a dataset is held.
export async function reresolveInputs() {
  const ds = app.dataset;
  if (!ds?.files || !app.current?.resolution) return null;
  let collections;
  try {
    collections = app.wasm.resolve_bids(ds.files, editor.text, "");
  } catch (e) {
    return `the recipe could not be resolved against this dataset: ${e.message}`;
  }
  if (collections.length === 0) return "the recipe now matches nothing in this dataset";
  const resolved = collections[0];
  if (String(resolved.data_files) !== String(app.current.resolution.data_files)) {
    return "the recipe now selects different volumes: reload the model to fit them";
  }

  const [nx, ny, nz] = app.current.meta.dims;
  const maskU8 = resolved.mask_file
    ? readMask(await volumeFromDataset(ds.files, resolved.mask_file), nx, ny, nz)
    : null;
  const auxFlat = {};
  const auxVolumes = {};
  for (const [auxName, path] of resolved.aux_files) {
    const auxVolume = await volumeFromDataset(ds.files, path);
    auxFlat[auxName] = Array.from(readScalarVolume(auxVolume, nx, ny, nz));
    auxVolumes[auxName] = auxVolume;
  }

  app.dataset = { ...ds, resolved };
  // A mask the reader asked the network for outranks the one the recipe selects:
  // they pressed a button for it, and it is kept so clearing it can restore this.
  app.current = {
    ...app.current,
    maskU8: app.computedMask ?? maskU8,
    resolvedMask: maskU8,
    auxFlat,
    auxVolumes,
    resolution: resolved,
    meta: { ...app.current.meta, protocol_json: resolved.protocol_json },
  };
  // The Files panel states a verdict per file, and those verdicts have just
  // changed: a mask the recipe no longer selects is now an unused file.
  renderFiles();
  for (const w of resolved.warnings) status(w, "error");
  return null;
}

export async function loadModel(name) {
  status(`Loading ${name}…`, "busy");
  let meta;
  try {
    meta = await loadBundle(name);
  } catch (e) {
    status(`Could not load ${name}: ${e.message}`, "error");
    return;
  }

  // Before the volumes go: both the drawing and any computed mask are indexed
  // into the grid they define.
  resetDrawing();
  clearComputedMask();
  clearLabelNames();
  clearVolumes(nvIn);
  clearVolumes(nvOut);
  linkViewers();
  app.outputVolumes = [];
  app.shownOutput = null;
  app.lastMaps = null;
  $("output").hidden = true;
  $("output").replaceChildren();
  repaintLevels();
  $("voxel-value").textContent = "";
  $("curve-note").textContent = "";
  clearCurve();
  app.enumFields = new Map((meta.enums ?? []).map((e) => [e.key, e.values]));
  app.wheelAccum = 0;
  app.dataset = null;
  app.protocolResolved = false;
  app.overrideProtocol = false;

  // The full BIDS dataset is the real thing: fetched, resolved in the browser,
  // and fitted with the acquisition its own sidecars declare. The pre-baked
  // slice is the fallback, for no network or a host that is down.
  if (app.wasm && meta.archive) {
    // Skeletons rather than a stale list: the file count is unknown until the
    // archive is open, so guess from the volume count the payload declares.
    showLoading(`fetching ${meta.archive}…`, (meta.dims?.[3] ?? 8) * 2 + 4);
    try {
      const ok = await loadModelFromBids(name, meta);
      // A dataset that resolved is showing its volumes: land on the viewer. One
      // that resolved to nothing this model can fit has already selected Files
      // to say why, so leave that standing.
      endLoading(ok ? "viewer" : null);
      if (ok) return;
    } catch (e) {
      endLoading();
      renderFilesError(`${e.message}. Showing the pre-baked slice instead`);
      status(`${e.message}. Showing the built-in sample instead`, "error");
      // An unreachable archive host is worth interrupting for: what follows is a
      // single built-in slice, not the dataset the reader asked for, and the
      // difference matters for anything they conclude from it.
      if (e.hostUnreachable) {
        showNotice(
          "cloud-off",
          `${e.hostUnreachable} is not responding`,
          `The full example datasets are hosted on ${e.hostUnreachable} and could not be `
            + "fetched, so the playground is showing its built-in single slice instead. "
            + "Fitting still works; the data is just smaller. Try again later, or drop "
            + "your own BIDS folder onto the page.",
        );
      }
    }
  }

  setEditorText(meta.config);
  const [nx, ny, nz, nt] = meta.dims;
  let volume, maskVolume;
  try {
    volume = await nvIn.addVolumeFromUrl({
      url: `./data/${meta.files.data}`,
      name: meta.model,
      colormap: "gray",
      colorbarVisible: false,
    });
  } catch (e) {
    status(`Could not load image data: ${e.message}`, "error");
    return;
  }
  let maskU8 = null;
  if (meta.files.mask) {
    try {
      maskVolume = await NVImage.loadFromUrl({ url: `./data/${meta.files.mask}` });
      maskU8 = readMask(maskVolume, nx, ny, nz);
    } catch (e) {
      status(`Could not load the mask: ${e.message}`, "error");
      return;
    }
  }

  // `files.aux` is a general mechanism: whatever auxiliary inputs the recipe
  // resolved (none, one, or several), load every one and hand it to
  // fit_volume (a flat map, for the whole slice) and to forward() (one
  // scalar, for whichever voxel is clicked) — never keyed on a model or aux
  // name.
  //
  // Unlike the BIDS path, a failure here is terminal: this *is* the fallback, so
  // there is nowhere further to fall back to. Each is reported against the file
  // that failed and abandons the load.
  const auxFlat = {};
  const auxVolumes = {};
  for (const [auxName, filename] of Object.entries(meta.files.aux ?? {})) {
    try {
      const auxVolume = await NVImage.loadFromUrl({ url: `./data/${filename}` });
      auxFlat[auxName] = Array.from(readScalarVolume(auxVolume, nx, ny, nz));
      auxVolumes[auxName] = auxVolume;
    } catch (e) {
      status(`Could not load ${filename}: ${e.message}`, "error");
      return;
    }
  }

  resetViewers();
  setFrameUi(nt, meta.labels);
  app.current = { meta, volume, maskU8, auxFlat, auxVolumes, frame: 0 };
  $("fit").disabled = !app.wasm;
  syncMapViewControls();
  syncSegmentButton();
  status(
    app.wasm ? "Ready" : "Fitting unavailable: wasm failed to load; viewers still work",
    app.wasm ? "ok" : "error",
  );
}
