// Recipe editor: a generic walk over the parsed YAML tree. Nothing here
// branches on a model, parameter, or key name — every widget kind is chosen
// purely from the JS type of the value found at that path.
//
// YAML highlighting is by highlight.js (vendored). A highlighted <pre> sits
// behind a transparent-text textarea, the two sharing identical metrics and
// scroll offset so the painted tokens line up under the real caret. The
// textarea stays the single source of truth for text and editing, and `js-yaml`
// remains the only thing that parses meaning — the `valid`/`invalid` pill comes
// from it.
import { load as yamlLoad, dump as yamlDump } from "./vendor/js-yaml.js";
import hljs from "./vendor/highlight-core.js";
import hljsYaml from "./vendor/highlight-yaml.js";
import hljsJson from "./vendor/highlight-json.js";
import { $ } from "./dom.js";
import { app, editor } from "./state.js";
import { mergeSurface, sameStructure } from "./surface.js";
import { debounce } from "./debounce.js";

hljs.registerLanguage("yaml", hljsYaml);
hljs.registerLanguage("json", hljsJson);

// The one highlighter, so a sidecar's contents and a recipe are painted by the
// same vendored build rather than a second one.
export function highlightJson(text) {
  return hljs.highlight(text, { language: "json" }).value;
}

function setAtPath(root, path, value) {
  let node = root;
  for (const k of path.slice(0, -1)) node = node[k];
  node[path.at(-1)] = value;
}

function isNumberArray(v) {
  return Array.isArray(v) && v.every((x) => typeof x === "number");
}

// A matrix: an array of same-shaped number arrays (e.g. a protocol table of
// [angle, offset] rows). Generic on shape alone, like every other widget
// here — never on a key name.
function isNumberMatrix(v) {
  return (
    Array.isArray(v) &&
    v.length > 0 &&
    v.every((row) => isNumberArray(row) && row.length === v[0].length)
  );
}

// Re-derives `editor.text` from `editor.obj` after a form edit, and refreshes
// the (possibly hidden) YAML textarea + status pill to match.
function commitObjEdit() {
  editor.text = yamlDump(editor.obj);
  editor.valid = true;
  $("cfg-yaml").value = editor.text;
  // The textarea's own text is transparent — the highlight layer behind it is what
  // a reader actually reads, so leaving it unpainted shows them the previous YAML.
  paintYaml();
  setYamlPill(true);
  refreshSurface();
  renderForm();
}

// Re-reads the model's option surface for the current recipe text. Cheap — a
// parse and a serialize, no fitting — so it runs after every committed edit.
// A failure (no wasm, unknown model) clears the surface, and `renderForm` then
// falls back to mirroring the recipe rather than blanking the panel.
export function refreshSurface() {
  if (!app.wasm || !editor.valid) return;
  try {
    app.surface = app.wasm.effective_config(editor.text);
  } catch (e) {
    app.surface = null;
  }
}

// The YAML view's textarea fires `oninput` per keystroke, and each keystroke
// reaches here — but typing lands in the textarea itself, never in a form
// widget, so there is no focus to lose; the only cost worth avoiding is
// calling into wasm once per character. `debounce` runs the first keystroke
// of a burst immediately (so a single edit, or a model load, updates without
// delay) and coalesces the rest into one more run once typing pauses.
const scheduleSurfaceRefresh = debounce(() => {
  refreshSurface();
  renderForm();
}, 150);

function fieldRow(label, path, widget) {
  const row = document.createElement("div");
  row.className = "field-row";
  const labelWrap = document.createElement("div");
  const labelSpan = document.createElement("span");
  labelSpan.className = "field-label";
  labelSpan.textContent = label.replace(/_/g, " ");
  const keySpan = document.createElement("span");
  keySpan.className = "field-key";
  keySpan.textContent = path.join(".");
  labelWrap.append(labelSpan, keySpan);
  row.append(labelWrap, widget);
  return row;
}

function buildWidget(value, path) {
  const enumValues = app.enumFields.get(path.join("."));
  if (enumValues && typeof value === "string") {
    const select = document.createElement("select");
    for (const opt of enumValues) {
      const o = document.createElement("option");
      o.value = opt;
      o.textContent = opt;
      select.append(o);
    }
    select.value = value;
    select.onchange = () => {
      setAtPath(editor.obj, path, select.value);
      commitObjEdit();
    };
    return select;
  }
  if (typeof value === "boolean") {
    const label = document.createElement("label");
    label.className = "switch";
    const input = document.createElement("input");
    input.type = "checkbox";
    input.checked = value;
    input.onchange = () => {
      setAtPath(editor.obj, path, input.checked);
      commitObjEdit();
    };
    const slider = document.createElement("span");
    slider.className = "slider";
    label.append(input, slider);
    return label;
  }
  if (typeof value === "number") {
    const input = document.createElement("input");
    input.type = "number";
    input.step = "any";
    input.value = String(value);
    input.onchange = () => {
      const v = Number(input.value);
      setAtPath(editor.obj, path, Number.isFinite(v) ? v : value);
      commitObjEdit();
    };
    return input;
  }
  if (isNumberArray(value)) {
    const input = document.createElement("input");
    input.type = "text";
    input.value = value.join(", ");
    input.onchange = () => {
      const parsed = input.value
        .split(",")
        .map((s) => Number(s.trim()))
        .filter((n) => !Number.isNaN(n));
      setAtPath(editor.obj, path, parsed);
      commitObjEdit();
    };
    return input;
  }
  if (typeof value === "string") {
    const input = document.createElement("input");
    input.type = "text";
    input.value = value;
    input.onchange = () => {
      setAtPath(editor.obj, path, input.value);
      commitObjEdit();
    };
    return input;
  }
  if (isNumberMatrix(value)) {
    // One row per line ("a, b, c"), the same compact notation the plain
    // number-array widget above uses — far more readable than the nested
    // block-YAML (`- - a\n  - b`) the generic fallback below would otherwise
    // produce for an array of arrays.
    const ta = document.createElement("textarea");
    ta.value = value.map((row) => row.join(", ")).join("\n");
    ta.onchange = () => {
      const rows = ta.value
        .split("\n")
        .map((line) => line.trim())
        .filter((line) => line.length > 0)
        .map((line) => line.split(",").map((s) => Number(s.trim())));
      if (rows.every((row) => row.every((n) => Number.isFinite(n)))) {
        setAtPath(editor.obj, path, rows);
        commitObjEdit();
      }
    };
    return ta;
  }
  if (value === null) {
    // An unset optional: a plain input whose text is read as YAML, so "0.015"
    // becomes a number, a word stays a string, and empty stays unset — the same
    // reading the recipe itself would get.
    const input = document.createElement("input");
    input.type = "text";
    input.value = "";
    input.onchange = () => {
      const text = input.value.trim();
      setAtPath(editor.obj, path, text === "" ? null : yamlLoad(text));
      commitObjEdit();
    };
    return input;
  }
  // Fallback for shapes a compact widget can't represent (array of arrays of
  // differing lengths, array of objects): a small YAML textarea for just
  // that subtree.
  const ta = document.createElement("textarea");
  ta.value = value == null ? "" : yamlDump(value).trimEnd();
  ta.onchange = () => {
    try {
      const parsed = ta.value.trim() === "" ? null : yamlLoad(ta.value);
      setAtPath(editor.obj, path, parsed);
      commitObjEdit();
    } catch (e) {
      // Leave the underlying object untouched; the textarea keeps the
      // reader's (currently unparsable) text so they can keep fixing it.
    }
  };
  return ta;
}

function buildRows(container, rows) {
  for (const row of rows) {
    if (row.children) {
      const group = document.createElement("div");
      group.className = "group";
      const title = document.createElement("div");
      title.className = "group-title";
      title.textContent = row.key;
      const fields = document.createElement("div");
      fields.className = "form-fields";
      group.append(title, fields);
      buildRows(fields, row.children);
      container.append(group);
      continue;
    }
    const widget = buildWidget(row.value, row.path);
    if (row.readOnly) {
      widget.disabled = true;
      widget.title = "supplied by the dataset's sidecars";
    }
    const el = fieldRow(row.key, row.path, widget);
    if (!row.isSet) el.classList.add("unset");
    if (row.readOnly) {
      el.classList.add("locked");
      const note = document.createElement("span");
      note.className = "field-source";
      note.textContent = "from sidecars";
      el.querySelector(".field-key")?.after(note);
    }
    container.append(el);
  }
}

// The last tree actually rendered, and the error rendered alongside it — the
// baseline `renderForm` diffs the next render against to decide whether an
// edit can be patched in place. `null` means the next render must build from
// scratch (nothing to compare yet, or the last render held no rows).
let lastRows = null;
let lastError;

function renderForm() {
  const error = app.surface?.error ?? null;
  showConfigError(error);
  const container = $("form-fields");
  if (!editor.obj || typeof editor.obj !== "object") {
    container.replaceChildren();
    lastRows = null;
    lastError = undefined;
    return;
  }

  // Without a surface (no wasm, or a model the registry does not know) fall
  // back to mirroring the recipe — the panel degrades, it does not vanish.
  const surface = app.surface ? yamlLoad(app.surface.yaml) : editor.obj;
  const rows = mergeSurface(surface, editor.obj, {
    protocolKeys: app.surface?.protocol_keys ?? [],
    readOnly: app.protocolResolved,
  });

  // Editing one field normally changes neither the key set, the read-only
  // flags, nor the model's own error — so in the common case nothing needs
  // rebuilding: patch the `unset` class row by row and leave every DOM node
  // (including whichever one still holds focus, e.g. a select or checkbox
  // that fires `onchange` immediately, or a text/number input whose blur
  // fires it just before the browser finishes a Tab-driven focus move) right
  // where it is. A structural change — a key appearing or disappearing
  // because e.g. `fit_type` changed what else is valid, a lock flipping, or
  // the model's own error changing — still gets a full rebuild, which is the
  // only way any of those show up at all.
  if (lastRows && error === lastError && sameStructure(lastRows, rows)) {
    patchRows(container, rows);
  } else {
    container.replaceChildren();
    buildRows(container, rows);
  }
  lastRows = rows;
  lastError = error;
}

// Walks `rows` against the DOM `buildRows` produced for the previous render of
// the same shape (guaranteed by `sameStructure`), toggling only the `unset`
// class — never touching a widget's value, its listeners, or its identity.
function patchRows(container, rows) {
  const nodes = container.children;
  rows.forEach((row, i) => {
    const el = nodes[i];
    if (row.children) {
      patchRows(el.querySelector(":scope > .form-fields"), row.children);
      return;
    }
    el.classList.toggle("unset", !row.isSet);
  });
}

function showConfigError(message) {
  const box = $("cfg-error");
  box.textContent = message ?? "";
  box.hidden = !message;
}

function setYamlPill(valid) {
  const pill = $("yaml-pill");
  pill.textContent = valid ? "valid" : "invalid";
  pill.classList.toggle("valid", valid);
  pill.classList.toggle("invalid", !valid);
}

// Sets the editor from a fresh string (model load, or typed into the YAML
// view). Rebuilds the form only when the text parses, so a reader mid-typo
// in the YAML view doesn't have the form yanked out from under them.
export function setEditorText(text) {
  editor.text = text;
  $("cfg-yaml").value = text;
  paintYaml();
  try {
    editor.obj = yamlLoad(text);
    editor.valid = true;
    setYamlPill(true);
    scheduleSurfaceRefresh();
  } catch (e) {
    editor.valid = false;
    setYamlPill(false);
  }
}

// Repaint the layer behind the textarea and keep the two scrolled together.
function paintYaml() {
  const ta = $("cfg-yaml");
  // A trailing newline would collapse in the <pre>, shifting the last line.
  $("yaml-hl").innerHTML = `${hljs.highlight(ta.value, { language: "yaml" }).value}\n`;
  syncYamlScroll();
}

export function syncYamlScroll() {
  const ta = $("cfg-yaml");
  const hl = $("yaml-hl");
  hl.scrollTop = ta.scrollTop;
  hl.scrollLeft = ta.scrollLeft;
}

export function showTab(tab) {
  const isForm = tab === "form";
  $("tab-form").classList.toggle("active", isForm);
  $("tab-yaml").classList.toggle("active", !isForm);
  $("tab-form").setAttribute("aria-selected", String(isForm));
  $("tab-yaml").setAttribute("aria-selected", String(!isForm));
  $("form-view").hidden = !isForm;
  $("yaml-view").hidden = isForm;
  if (isForm && editor.valid) renderForm();
}
