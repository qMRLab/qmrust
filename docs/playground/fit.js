// Fitting the loaded slice, in row-blocks so progress is real and the page stays
// responsive, then turning the returned maps into viewable volumes.
import { $, hideProgress, nextFrame, setProgress, showProgress, status } from "./dom.js";
import { app, editor, nvOut } from "./state.js";
import { buildMapVolume, readVolumeSeries } from "./nifti.js";
import { clearVolumes, linkViewers, showOutput, syncMapViewControls } from "./viewers.js";
import { plotVoxel } from "./curve.js";
import { reresolveInputs } from "./model.js";
import { showNotice } from "./modal.js";

// `fit_volume` is one synchronous call with no progress callback across the
// wasm boundary, so real (not simulated) progress requires splitting the work
// ourselves: fit contiguous row-blocks of the slice — `x` is the slowest-
// varying axis in the C-order `[nx,ny,nz,nt]`/`[nx,ny,nz]` layout `data`/
// `maskU8`/`auxFlat` already use, so a row range is a contiguous subarray —
// and stitch each block's returned maps into the full-size buffers, yielding
// to the event loop between blocks so the progress bar paints and the page
// stays responsive. Each block is an independent voxelwise fit, so the
// stitched result is exactly the single-call result (verified by `probes`).
async function fitVolumeWithProgress(nx, ny, nz, nt, data, maskU8, auxFlat) {
  const spatial = nx * ny * nz;
  const blocks = Math.min(nx, 24);
  const rowsPerBlock = Math.max(1, Math.ceil(nx / blocks));
  let maps = null;
  for (let rowStart = 0; rowStart < nx; rowStart += rowsPerBlock) {
    const rowEnd = Math.min(nx, rowStart + rowsPerBlock);
    const rows = rowEnd - rowStart;
    const lo = rowStart * ny * nz;
    const hi = rowEnd * ny * nz;
    const blockData = data.subarray(lo * nt, hi * nt);
    const blockMask = maskU8 ? maskU8.subarray(lo, hi) : undefined;
    const blockAux = {};
    for (const [k, v] of Object.entries(auxFlat)) blockAux[k] = v.slice(lo, hi);
    const raw = app.wasm.fit_volume(
      editor.text,
      blockData,
      Uint32Array.from([rows, ny, nz, nt]),
      JSON.stringify(app.current.meta.volume_ids),
      blockMask,
      JSON.stringify(blockAux),
      // The acquisition, when it was resolved from the data rather than written
      // in the recipe. Empty for a payload whose recipe carries the protocol.
      app.current.meta.protocol_json ?? "",
    );
    const blockMaps = Object.fromEntries(raw);
    if (!maps) {
      maps = {};
      for (const name of Object.keys(blockMaps)) maps[name] = new Float64Array(spatial).fill(NaN);
    }
    for (const [name, flat] of Object.entries(blockMaps)) maps[name].set(flat, lo);
    setProgress((rowEnd / nx) * 100);
    await nextFrame();
  }
  // The loop's last block already set 100%, but the caller resets the bar as soon
  // as it returns — so give that value a frame of its own to actually render.
  setProgress(100);
  await nextFrame();
  return maps ?? {};
}

export async function fitSlice() {
  if (!app.current) { status("Pick a model first", "info"); return; }
  if (!app.wasm) { status("Fitting unavailable: wasm failed to load", "error"); return; }
  if (!editor.valid) {
    status("Recipe YAML is invalid: fix it before fitting", "error");
    return;
  }
  // The recipe may have changed which files the model consumes since it was
  // loaded, not only how it fits them.
  const problem = await reresolveInputs();
  if (problem) {
    status(`Not fitted: ${problem}`, "error");
    return;
  }
  const { meta, volume, maskU8, auxFlat } = app.current;
  const [nx, ny, nz, nt] = meta.dims;
  status("Fitting…", "busy");
  showProgress();
  $("fit").disabled = true;
  const t0 = performance.now();
  const data = readVolumeSeries(volume, nx, ny, nz, nt);
  let maps;
  try {
    maps = await fitVolumeWithProgress(nx, ny, nz, nt, data, maskU8, auxFlat);
  } catch (e) {
    // The status line keeps the headline; the explanation goes where there is
    // room to read it. A model's own message names the recipe key to change,
    // which is unreadable truncated into a one-line bar.
    status("Fit failed", "error");
    showNotice("triangle-alert", "Fit failed", String(e?.message ?? e));
    hideProgress();
    $("fit").disabled = false;
    return;
  }
  app.lastMaps = maps;

  clearVolumes(nvOut);
  linkViewers();
  app.outputVolumes = [];
  for (const o of meta.outputs) {
    const flat = maps[o.name];
    if (!flat) continue;
    const vol = buildMapVolume(volume, flat, nx, ny, nz, o.name, o.unit, o.display_range);
    app.outputVolumes.push({
      name: o.name,
      unit: o.unit,
      volume: vol,
    });
  }

  const select = $("output");
  select.replaceChildren();
  for (const o of app.outputVolumes) {
    const opt = document.createElement("option");
    opt.value = o.name;
    opt.textContent = `${o.name}${o.unit ? ` [${o.unit}]` : ""}`;
    select.append(opt);
  }
  select.hidden = app.outputVolumes.length <= 1;

  // The fit itself is done; everything from here is display. A failure while
  // drawing must not leave the page reading "Fitting…" forever with its controls
  // disabled — the maps exist either way, so report the display fault and
  // release the UI.
  try {
    showOutput(app.outputVolumes[0]?.name);
    if (app.current.lastVox) {
      plotVoxel(app.current.lastVox.x, app.current.lastVox.y, app.current.lastVox.z);
    }
    // How long the fit took is the natural end of the "Fitting…" the status line
    // is already showing, so it lands there rather than in a panel of its own.
    status(`Fitted in ${((performance.now() - t0) / 1000).toFixed(1)} s`, "ok");
  } catch (e) {
    status(`Fitted, but could not display it: ${e?.message ?? e}`, "error");
  } finally {
    hideProgress();
    $("fit").disabled = false;
    syncMapViewControls();
  }
}
