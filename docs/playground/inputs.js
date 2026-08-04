// The Inputs card: the acquired image, the file list behind it, and the frame
// slider they share. The card shows either the image or where the image came
// from, and a filename and the image it produced are one click apart — which is
// why the frame UI and the file tree live together.
//
// Every verdict in the file list arrives as data from `resolve_bids`; this only
// chooses how to draw it, and branches on the verdict's shape alone — never on a
// model, suffix, or parameter name.
import { icon } from "./vendor/icons.js";
import { $, identityLabel } from "./dom.js";
import { app, nvIn } from "./state.js";
import { hideDownloadProgress } from "./dataset.js";
import { openFileModal, openJsonModal } from "./modal.js";
import { sizeViewers } from "./viewers.js";

// The track's coloured portion, as a percentage. A single-volume series has no
// range to travel, so it reads as full rather than empty.
export function syncFrameFill() {
  const slider = $("frame");
  const max = Number(slider.max);
  const pct = max > 0 ? (Number(slider.value) / max) * 100 : 100;
  slider.style.setProperty("--fill", `${pct}%`);
}

export function setFrameUi(nt, labels) {
  const slider = $("frame");
  slider.max = String(Math.max(0, nt - 1));
  slider.value = "0";
  slider.disabled = nt <= 1;
  $("frame-label").textContent = labels[0] ?? "";
  syncFrameFill();
}

export function onFrameChange() {
  if (!app.current) return;
  const t = Number($("frame").value);
  app.current.frame = t;
  nvIn.setFrame4D(app.current.volume.id, t);
  $("frame-label").textContent = app.current.meta.labels[t] ?? "";
  syncFrameFill();
  syncFilesHighlight();
}

// Jumps to frame `t` (NiiVue's own `setFrame4D`, the same call `onFrameChange`
// makes from the slider) and syncs the slider + label to match, so the wheel
// and the slider always agree on where they are.
export function goToFrame(t) {
  $("frame").value = String(t);
  onFrameChange();
}

// The Inputs card shows either the image or where the image came from.
export function showInputsTab(tab) {
  const isViewer = tab === "viewer";
  $("tab-viewer").classList.toggle("active", isViewer);
  $("tab-files").classList.toggle("active", !isViewer);
  $("tab-viewer").setAttribute("aria-selected", String(isViewer));
  $("tab-files").setAttribute("aria-selected", String(!isViewer));
  $("viewer-in").hidden = !isViewer;
  $("files-view").hidden = isViewer;
  // The canvas was display:none while hidden, so its GL viewport is stale.
  // Skip while loading: the skeleton is over it and there is nothing to resize.
  if (isViewer && !app.loading) sizeViewers();
}

// While a dataset is being fetched, extracted and resolved there is nothing
// truthful to list, so both views show the shape of what is coming rather than a
// stale list or an empty black canvas. `expect` is the file count if known.
export function showLoading(note, expect = 12) {
  app.loading = true;
  // A dataset load only fills the Inputs panel, so only its skeleton appears;
  // the fitted map gets its own skeleton from `fitSlice`, while a fit runs.
  // The overlay sits inside its viewer, so the card does not change size.
  $("skel-in").hidden = false;
  const tree = $("files-tree");
  tree.replaceChildren();
  $("files-summary").textContent = note;
  const dir = document.createElement("div");
  dir.className = "skel skel-dir";
  tree.append(dir);
  for (let i = 0; i < expect; i++) {
    const row = document.createElement("div");
    row.className = "skel-row";
    const name = document.createElement("div");
    // Vary the width a little so the block reads as a list of names rather
    // than a grid.
    name.className = "skel skel-name";
    name.style.maxWidth = `${55 + ((i * 37) % 40)}%`;
    const role = document.createElement("div");
    role.className = "skel skel-role";
    row.append(name, role);
    tree.append(row);
  }
}

// Reveals the real content behind the skeletons. `prefer` names the tab to land
// on: a dataset that loaded is showing its image, so the viewer is what the
// reader wants, whatever tab they happened to leave selected — being stranded on
// a file list after a successful load reads as if nothing arrived. Callers that
// pass nothing keep the reader where they are, which is what the failure paths
// want: they have already selected Files to explain themselves.
export function endLoading(prefer = null) {
  app.loading = false;
  hideDownloadProgress();
  $("skel-in").hidden = true;
  $("skel-out").hidden = true;
  if (prefer) {
    showInputsTab(prefer);
    return;
  }
  const onFiles = $("tab-files").classList.contains("active");
  showInputsTab(onFiles ? "files" : "viewer");
}

function roleLabel(role) {
  switch (role.kind) {
    case "volume": {
      const detail = identityLabel(role.identity);
      const n = `volume ${role.index + 1} of ${role.total}`;
      return detail === "volume" ? n : `${n} · ${detail}`;
    }
    case "sidecar":
      // A count only says something when inheritance spread it wider than the
      // volume it sits beside.
      return role.applies_to.length > 1
        ? `sidecar → ${role.applies_to.length} volumes`
        : "sidecar";
    case "mask":
      return "mask";
    case "aux":
      return role.input;
    case "dataset_metadata":
      return "dataset metadata";
    case "unused":
      return role.reason;
    default:
      return role.kind;
  }
}

// Build a directory tree from the flat path list, preserving path order within
// each directory. A nested structure is what a reader recognises as a dataset —
// the flat "one heading per directory" form repeated the whole prefix each time.
function buildTree(files) {
  const root = { dirs: new Map(), files: [] };
  for (const [path, role] of files) {
    const parts = path.split("/");
    const name = parts.pop();
    let node = root;
    for (const dir of parts) {
      if (!node.dirs.has(dir)) node.dirs.set(dir, { dirs: new Map(), files: [] });
      node = node.dirs.get(dir);
    }
    node.files.push({ path, name, role });
  }
  return root;
}

const VIEWABLE = ["volume", "mask", "aux"];

// An icon per file kind, chosen from the name rather than from the resolved role:
// what a reader recognises in a listing is the extension, and an unrecognised file
// still deserves a mark so the rows line up.
function fileIcon(name) {
  const lower = name.toLowerCase();
  if (lower.endsWith(".json")) return "file-braces-corner";
  if (lower.endsWith(".tsv") || lower.endsWith(".csv")) return "file-spreadsheet";
  if (lower.endsWith(".nii") || lower.endsWith(".nii.gz")) return "file-box";
  return "file";
}

// One file's row. `kind` colouring distinguishes image data from its metadata,
// the two roles the BIDS layout is built around.
function fileRow({ path, name, role }) {
  const row = document.createElement("div");
  row.className = `files-row ${role.kind}`;
  row.dataset.path = path;
  const isJson = name.endsWith(".json");
  const isImage = VIEWABLE.includes(role.kind) && !isJson;
  const held = app.dataset?.files?.has(path);
  if (role.kind === "volume") {
    row.dataset.frame = String(role.index);
    row.classList.add("clickable");
    row.title = "Click to show this volume in the viewer";
  }
  const glyph = document.createElement("span");
  glyph.className = "files-icon";
  glyph.innerHTML = icon(fileIcon(name), 13);
  const label = document.createElement("span");
  label.className = `files-name${isImage ? " is-nii" : ""}${isJson ? " is-json" : ""}`;
  label.textContent = name;
  const kind = document.createElement("span");
  kind.className = "files-role";
  kind.textContent = roleLabel(role);
  row.append(glyph, label, kind);

  // The filename is the "open this file" affordance for both kinds: a sidecar
  // opens as text, an image in a viewer. One click either way — the row's own
  // click still steps the inputs viewer to that volume, so both actions are
  // reachable without a modifier or a second click.
  if (held && (isJson || isImage)) {
    row.classList.add("is-openable");
    label.title = isJson ? "Show this file's contents" : "Open this image in a viewer";
    label.addEventListener("click", (e) => {
      e.stopPropagation();
      if (isJson) openJsonModal(path);
      else openFileModal(path);
    });
  }
  return row;
}

// Render one tree level: its files, then its subdirectories, each indented.
function renderNode(node, parent) {
  for (const entry of node.files) parent.append(fileRow(entry));
  for (const [dir, child] of node.dirs) {
    const box = document.createElement("div");
    box.className = "files-node";
    const head = document.createElement("div");
    head.className = "files-dir";
    head.textContent = dir;
    box.append(head);
    renderNode(child, box);
    parent.append(box);
  }
}

export function renderFiles() {
  const tree = $("files-tree");
  tree.replaceChildren();
  if (!app.dataset?.resolved?.files) {
    $("files-summary").textContent = app.dataset
      ? "No per-file verdicts for this dataset."
      : "Showing the pre-baked demo slice, no BIDS dataset loaded.";
    return;
  }
  const entries = app.dataset.resolved.files;
  const counts = new Map();
  for (const [, role] of entries) counts.set(role.kind, (counts.get(role.kind) ?? 0) + 1);
  const parts = [...counts].map(([k, n]) => `${n} ${k.replace(/_/g, " ")}`);
  $("files-summary").textContent =
    `${app.dataset.archive} · ${entries.length} files · ${parts.join(" · ")}`;

  const root = document.createElement("div");
  root.className = "files-node";
  renderNode(buildTree(entries), root);
  tree.append(root);
  syncFilesHighlight();
}

// The row and the frame slider drive the same state, so a filename and the
// image it produced are one click apart.
export function onFilesClick(event) {
  const row = event.target.closest(".files-row");
  if (!row?.dataset.frame) return;
  goToFrame(Number(row.dataset.frame));
}

export function syncFilesHighlight() {
  const frame = String(app.current?.frame ?? 0);
  for (const row of $("files-tree").querySelectorAll(".files-row")) {
    row.classList.toggle("current", row.dataset.frame === frame);
  }
}

// Failures show in the panel as well as the status bar: this is where a reader
// looks for why nothing appeared.
export function renderFilesError(message) {
  $("files-tree").replaceChildren();
  $("files-summary").textContent = message;
  showInputsTab("files");
}
