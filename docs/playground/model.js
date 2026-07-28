// Loading a model's data: a real BIDS dataset when the payload names one,
// the pre-baked slice otherwise. Both paths end in the same place — `app.current`
// holding the volume, mask and auxiliary inputs a fit needs — so everything
// downstream is blind to where the data came from.
import { NVImage } from "./vendor/niivue.js";
import { $, status } from "./dom.js";
import { app, nvIn, nvOut } from "./state.js";
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
import { clearVolumes, sizeViewers, syncMapViewControls } from "./viewers.js";
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
    status(`No ${meta.title} data in that dataset — see Files for why`, "error");
    return false;
  }

  const resolved = ds.collections[0];
  app.dataset = { ...ds, resolved };
  // The recipe for BIDS input carries options only; the acquisition comes from
  // the sidecars, via `resolved.protocol_json`.
  setEditorText(meta.config_bids);

  stage(`Loading ${resolved.data_files.length} volumes…`);
  const parts = resolved.data_files.map((path) => {
    const bytes = ds.files.get(path);
    if (!bytes) throw new Error(`resolution named "${path}", which the archive does not hold`);
    return bytes;
  });
  const { bytes, nx, ny, nz, nt } = buildSeriesNifti(parts);
  const volume = await NVImage.loadFromFile({
    file: new File([bytes], `${meta.model}.nii`),
    name: meta.model,
    colormap: "gray",
    colorbarVisible: false,
  });
  nvIn.addVolume(volume);

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
  app.current = { meta: bidsMeta, volume, maskU8, auxFlat, auxVolumes, frame: 0 };
  renderFiles();
  syncMapViewControls();
  $("fit").disabled = false;
  for (const w of resolved.warnings) status(w, "error");
  $("model-info").textContent =
    `${meta.title}: ${nt} volumes, ${nx}×${ny}${nz > 1 ? `×${nz}` : ""} (${resolved.subject})`;
  status("Ready", "ok");
  return true;
}

export async function loadModel(name) {
  status(`Loading ${name}…`, "busy");
  let meta;
  try {
    meta = await loadBundle(name);
  } catch (e) {
    status(`Could not load ${name} — ${e.message}`, "error");
    return;
  }

  clearVolumes(nvIn);
  clearVolumes(nvOut);
  app.outputVolumes = [];
  app.shownOutput = null;
  app.lastMaps = null;
  $("output").hidden = true;
  $("output").replaceChildren();
  repaintLevels();
  $("voxel-value").textContent = "";
  $("model-info").textContent = "";
  $("fit-timing").textContent = "";
  $("curve-note").textContent = "";
  clearCurve();
  app.enumFields = new Map((meta.enums ?? []).map((e) => [e.key, e.values]));
  app.wheelAccum = 0;
  app.dataset = null;

  // The full BIDS dataset is the real thing: fetched, resolved in the browser,
  // and fitted with the acquisition its own sidecars declare. The pre-baked
  // slice is the fallback, for no network or a host that is down.
  if (app.wasm && meta.archive) {
    // Skeletons rather than a stale list: the file count is unknown until the
    // archive is open, so guess from the volume count the payload declares.
    showLoading(`fetching ${meta.archive}…`, (meta.dims?.[3] ?? 8) * 2 + 4);
    try {
      const ok = await loadModelFromBids(name, meta);
      endLoading();
      if (ok) return;
    } catch (e) {
      endLoading();
      renderFilesError(`${e.message} — showing the pre-baked slice instead`);
      status(`${e.message} — showing the built-in sample instead`, "error");
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
    status(`Could not load image data — ${e.message}`, "error");
    return;
  }
  let maskU8 = null;
  if (meta.files.mask) {
    try {
      maskVolume = await NVImage.loadFromUrl({ url: `./data/${meta.files.mask}` });
      maskU8 = readMask(maskVolume, nx, ny, nz);
    } catch (e) {
      status(`Could not load the mask — ${e.message}`, "error");
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
      status(`Could not load ${filename} — ${e.message}`, "error");
      return;
    }
  }

  resetViewers();
  setFrameUi(nt, meta.labels);
  app.current = { meta, volume, maskU8, auxFlat, auxVolumes, frame: 0 };
  $("fit").disabled = !app.wasm;
  // The model/volume-count/dims summary lives beside the frame note, not the
  // navbar — the navbar's `#status` is for transient app state (loading,
  // errors, "ready"), not a per-model fact that stays true until the next
  // model switch.
  $("model-info").textContent = `${meta.title}: ${nt} volumes, ${nx}×${ny}`;
  syncMapViewControls();
  status(
    app.wasm ? "Ready" : "Fitting unavailable — wasm failed to load; viewers still work",
    app.wasm ? "ok" : "error",
  );
}
