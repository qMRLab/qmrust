// Full BIDS datasets: fetched, unzipped, and resolved in the browser through
// the same `rust-bids` code the CLI's `--bids-dir` path runs. The acquisition
// comes from the dataset's own JSON sidecars, so the recipe for this path
// carries options only — see `meta.config_bids`.
import { unzipSync } from "./vendor/fflate.js";
import { $, fmt, status } from "./dom.js";
import { app } from "./state.js";
import { fetchOrThrow } from "./bundles.js";

const datasetCache = {};

// data/sources.json, fetched once.
let sourcesCache = null;

// The download ring, on the fitted-map skeleton — the one panel with nothing
// else to say while bytes arrive. `fraction` is null when the server sent no
// `Content-Length`, in which case the ring stays hidden rather than animating a
// number it cannot know.
const RING_CIRCUMFERENCE = 2 * Math.PI * 36; // r=36, matching the SVG

// Three states, because a download has three phases a reader can distinguish:
// connecting (no total known yet — spin), transferring (show the percentage),
// and done (gone).
function showDownloadPending() {
  const box = $("navbar-dl");
  box.hidden = false;
  box.classList.add("pending");
  // The indeterminate half of the pair: the ring reports a percentage when there
  // is one to report, while the navbar's sweep says "still working" through the
  // phases that have no measurable total — extracting an archive, resolving it.
  $("navbar").classList.add("loading");
  $("dl-pct").hidden = true;
  $("dl-arc").style.strokeDashoffset = "";
}

function setDownloadProgress(fraction) {
  const box = $("navbar-dl");
  if (fraction === null) {
    box.hidden = true;
    box.classList.remove("pending");
    $("navbar").classList.remove("loading");
    return;
  }
  box.hidden = false;
  box.classList.remove("pending");
  $("dl-pct").hidden = false;
  const clamped = Math.max(0, Math.min(1, fraction));
  $("dl-arc").style.strokeDashoffset = String(RING_CIRCUMFERENCE * (1 - clamped));
  $("dl-pct").textContent = `${Math.round(clamped * 100)}%`;
}

export function hideDownloadProgress() {
  setDownloadProgress(null);
}

// How long a fetch may take before it is called a failure rather than left
// hanging. Generous, because these archives are tens of megabytes on a slow
// link — but finite, because an unresponsive host must eventually say so.
const FETCH_DEADLINE_MS = 60_000;

// What to say while waiting, and when. Silence for the first few seconds (most
// fetches finish there and a warning would be noise), then progressively more
// candid — naming the host once it is clearly the host that is slow, since that
// tells a reader the fault is not theirs and not ours.
const SLOW_STEPS = [
  [6_000, (h) => `Still fetching the example dataset from ${h}…`],
  [15_000, (h) => `${h} is taking longer than usual…`],
  [30_000, (h) => `${h} is slow to respond — still trying…`],
];

function watchSlow(host) {
  const timers = SLOW_STEPS.map(([at, text]) => setTimeout(() => stage(text(host)), at));
  return () => timers.forEach(clearTimeout);
}

// Read the body as a stream so progress is real rather than simulated. A
// server that sends no length (or a browser without streams) still works — it
// just gets no ring.
//
// A failure to reach the host is marked `hostUnreachable`, so the caller can
// tell "the archive host is down" from "the archive was unreadable" and say the
// right thing.
async function fetchWithProgress(url) {
  // Spin from the moment the request goes out: waiting for headers (redirects,
  // time-to-first-byte) is often the longest part, and leaving the ring hidden
  // until the first chunk made the whole transfer look instantaneous.
  showDownloadPending();
  const host = new URL(url, location.href).hostname;
  const stopWatching = watchSlow(host);
  const abort = new AbortController();
  const deadline = setTimeout(() => abort.abort(), FETCH_DEADLINE_MS);
  let res;
  try {
    res = await fetch(url, { signal: abort.signal });
  } catch (e) {
    const err = new Error(
      abort.signal.aborted
        ? `${host} did not respond within ${FETCH_DEADLINE_MS / 1000} s`
        : `${host} could not be reached (${e.message})`,
    );
    err.hostUnreachable = host;
    throw err;
  } finally {
    clearTimeout(deadline);
    stopWatching();
  }
  if (!res.ok) {
    const err = new Error(`${host} refused the request (HTTP ${res.status})`);
    err.hostUnreachable = host;
    throw err;
  }
  const total = Number(res.headers.get("content-length")) || 0;
  if (!res.body?.getReader || !total) {
    // No length to measure against, so keep spinning rather than inventing one.
    return new Uint8Array(await res.arrayBuffer());
  }
  const reader = res.body.getReader();
  const chunks = [];
  let received = 0;
  setDownloadProgress(0);
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    chunks.push(value);
    received += value.length;
    setDownloadProgress(received / total);
  }
  const out = new Uint8Array(received);
  let at = 0;
  for (const c of chunks) {
    out.set(c, at);
    at += c.length;
  }
  return out;
}

// One loading stage, reported in both places a reader might be looking: the
// navbar status and whichever skeleton is on screen.
export function stage(message) {
  status(message, "busy");
  // The files panel's own header doubles as the loading caption; the viewers
  // carry no text, only their shimmer.
  if (app.loading) $("files-summary").textContent = message;
  // Every stage other than the transfer itself is indeterminate — extracting a
  // few MB and resolving it take real time but report no total — so the ring
  // spins through them rather than parking at 100% until the very end.
  if (app.loading) showDownloadPending();
}

async function loadSources() {
  if (!sourcesCache) {
    sourcesCache = await (await fetchOrThrow("./data/sources.json")).json();
  }
  return sourcesCache;
}

// Unzip into the dataset-root-relative `path -> bytes` shape `resolve_bids`
// requires. The archives wrap their dataset in a `ds-<slug>/` directory; that
// wrapper is the caller's to strip, since only the caller knows where its root
// is (a dropped folder answers differently).
export function unzipDataset(buf) {
  const files = new Map();
  for (const [path, bytes] of Object.entries(unzipSync(buf))) {
    if (path.endsWith("/") || bytes.length === 0) continue;
    const cut = path.indexOf("/");
    files.set(cut === -1 ? path : path.slice(cut + 1), bytes);
  }
  return files;
}

// A volume's label, from whatever identity it resolved to — a role name for a
// named measurement, its parameter values for a series one. Never keyed on a
// model or parameter name.
export function identityLabel(id) {
  if (!id) return "volume";
  if (id.role) return id.role;
  const parts = Object.entries(id.params ?? {}).map(([k, v]) => `${k}=${fmt(v)}`);
  return parts.length ? parts.join(", ") : "volume";
}

export async function fetchDataset(name, meta) {
  // A dataset the reader dropped takes precedence over the hosted one, and is
  // consumed once: from here on it is indistinguishable from a fetched dataset.
  if (app.droppedFiles) {
    const files = app.droppedFiles;
    app.droppedFiles = null;
    stage(`Resolving ${files.size} files as BIDS…`);
    return {
      archive: "your dataset",
      files,
      collections: app.wasm.resolve_bids(files, meta.config_bids, ""),
    };
  }
  if (datasetCache[name]) return datasetCache[name];
  const src = await loadSources();
  const url = `${src.base}/${meta.archive}${src.suffix}`;
  stage("Downloading example data…");
  const buf = await fetchWithProgress(url);

  stage(`Extracting ${(buf.length / 1e6).toFixed(1)} MB…`);
  let files;
  try {
    files = unzipDataset(buf);
  } catch (e) {
    throw new Error(`could not read ${meta.archive} (${buf.length} bytes): ${e.message}`);
  }

  stage(`resolving ${files.size} files as BIDS…`);
  const collections = app.wasm.resolve_bids(files, meta.config_bids, "");
  const dataset = { archive: meta.archive, files, collections };
  datasetCache[name] = dataset;
  return dataset;
}
