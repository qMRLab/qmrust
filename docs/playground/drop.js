// A reader's own BIDS data, dropped onto the page. Everything past "turn this
// into a path -> bytes map" is the fetched path's code, unchanged: the same
// `resolve_bids`, the same loading, the same fit. That identity is the point —
// there is no separate code path for "our demo data" and "your data".
import { $, status } from "./dom.js";
import { app } from "./state.js";
import { loadBundle } from "./bundles.js";
import { stage, unzipDataset } from "./dataset.js";
import {
  endLoading,
  renderFiles,
  renderFilesError,
  showInputsTab,
  showLoading,
} from "./inputs.js";
import { loadModel } from "./model.js";

// Walk a dropped directory entry into `[path, File]` pairs. The File System
// Entry API is callback-based and its `readEntries` returns at most 100 entries
// per call, so it must be drained in a loop.
async function walkEntry(entry, prefix = "") {
  const path = prefix ? `${prefix}/${entry.name}` : entry.name;
  if (entry.isFile) {
    const file = await new Promise((res, rej) => entry.file(res, rej));
    return [[path, file]];
  }
  const reader = entry.createReader();
  const children = [];
  for (;;) {
    const batch = await new Promise((res, rej) => reader.readEntries(res, rej));
    if (batch.length === 0) break;
    children.push(...batch);
  }
  const nested = await Promise.all(children.map((c) => walkEntry(c, path)));
  return nested.flat();
}

// The dataset root is wherever `dataset_description.json` sits — the file BIDS
// defines the root by. Paths are rewritten relative to it, so a folder dropped
// from anywhere resolves the same as an unzipped archive.
function rootRelative(pairs) {
  const marker = pairs
    .map(([p]) => p)
    .filter((p) => p.endsWith("dataset_description.json"))
    .sort((a, b) => a.length - b.length)[0];
  if (!marker) {
    throw new Error(
      "no dataset_description.json found: a BIDS dataset root must contain one",
    );
  }
  const root = marker.slice(0, marker.length - "dataset_description.json".length);
  return pairs
    .filter(([p]) => p.startsWith(root))
    .map(([p, f]) => [p.slice(root.length), f]);
}

// `entries` must be captured synchronously from the drop event: a
// `DataTransfer`'s items are neutered once the handler returns.
async function filesFromDrop(entries) {
  // A single dropped archive is the fetched path's own input, so reuse it.
  if (entries.length === 1 && entries[0].isFile && /\.zip$/i.test(entries[0].name)) {
    const file = await new Promise((res, rej) => entries[0].file(res, rej));
    return unzipDataset(new Uint8Array(await file.arrayBuffer()));
  }

  const pairs = (await Promise.all(entries.map((e) => walkEntry(e)))).flat();
  if (pairs.length === 0) throw new Error("nothing readable in that drop");
  const files = new Map();
  for (const [path, file] of rootRelative(pairs)) {
    files.set(path, new Uint8Array(await file.arrayBuffer()));
  }
  return files;
}

// Which registered models can fit this dataset. Asking every model, rather than
// assuming the selected one, is what lets a reader drop a folder and be told
// what it is — including when it is nothing this playground knows.
async function modelsMatching(files) {
  const matches = [];
  for (const name of app.modelNames) {
    const meta = await loadBundle(name);
    try {
      if (app.wasm.resolve_bids(files, meta.config_bids, "").length > 0) matches.push(name);
    } catch {
      // A model whose resolution throws simply does not match this dataset.
    }
  }
  return matches;
}

// A reader's dataset, however it arrived: find which models can fit it, load it
// through the ordinary path, or explain per file why nothing matched.
async function useDataset(files) {
  stage(`Resolving your ${files.size} files as BIDS…`);
  const matches = await modelsMatching(files);
  if (matches.length === 0) {
    // Explain it against the model currently selected, since that is the one
    // the reader was looking at when they handed the dataset over.
    const meta = await loadBundle($("model").value);
    const verdicts = app.wasm.annotate_non_matching(files, meta.config_bids);
    app.dataset = { archive: "your dataset", files, resolved: { files: verdicts } };
    endLoading();
    renderFiles();
    $("files-summary").textContent =
      `your dataset · ${verdicts.length} files · no model here can fit this`;
    showInputsTab("files");
    status("No model matches that dataset: see Files for why", "error");
    return;
  }

  // Pinning the dataset lets the ordinary load path pick it up instead of
  // fetching, so nothing downstream knows where it came from.
  const pick = matches.includes($("model").value) ? $("model").value : matches[0];
  app.droppedFiles = files;
  $("model").value = pick;
  endLoading();
  await loadModel(pick);
  const others = matches.filter((m) => m !== pick);
  if (others.length) {
    status(`Loaded as ${pick}, which also matches ${others.join(", ")}`, "info");
  }
}

// However the reader handed the dataset over, the shape is the same: show the
// skeletons, read the files, load them — and report the same way in both the
// panel and the status line when the read fails.
async function readAndUse(read) {
  showLoading("Reading your dataset…", 16);
  try {
    await useDataset(await read());
  } catch (e) {
    endLoading();
    renderFilesError(`Could not read that dataset: ${e.message}`);
    status(`Could not read that dataset: ${e.message}`, "error");
  }
}

// Turn `<input type="file">` output into the same path -> bytes map a drop
// produces. A directory picker reports each file's `webkitRelativePath`, which
// is exactly the relative path resolution needs; a picked `.zip` goes through
// the same extractor a fetched archive does.
async function filesFromInput(fileList) {
  const picked = [...fileList];
  if (picked.length === 0) throw new Error("nothing selected");
  if (picked.length === 1 && /\.zip$/i.test(picked[0].name)) {
    return unzipDataset(new Uint8Array(await picked[0].arrayBuffer()));
  }
  const pairs = picked.map((f) => [f.webkitRelativePath || f.name, f]);
  const files = new Map();
  for (const [path, file] of rootRelative(pairs)) {
    files.set(path, new Uint8Array(await file.arrayBuffer()));
  }
  return files;
}

export async function onBrowse(fileList) {
  if (!app.wasm) {
    status("Fitting unavailable: wasm failed to load", "error");
    return;
  }
  readAndUse(() => filesFromInput(fileList));
}

export function onDrop(event) {
  event.preventDefault();
  $("drop-wrap").classList.remove("drop-over");
  if (!app.wasm) {
    status("Fitting unavailable: wasm failed to load", "error");
    return;
  }
  // Capture the entries here, synchronously: the DataTransfer is neutered as
  // soon as this handler returns, so nothing async may touch it.
  const entries = [...event.dataTransfer.items]
    .map((i) => (i.webkitGetAsEntry ? i.webkitGetAsEntry() : null))
    .filter(Boolean);
  if (entries.length === 0) {
    status("That drop carried no files", "error");
    return;
  }
  readAndUse(() => filesFromDrop(entries));
}
