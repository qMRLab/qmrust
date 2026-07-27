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

function isPlainObject(v) {
  return v !== null && typeof v === "object" && !Array.isArray(v);
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
  setYamlPill(true);
}

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
  // Fallback for shapes a compact widget can't represent (array of arrays of
  // differing lengths, array of objects, null): a small YAML textarea for
  // just that subtree.
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

function buildGroup(container, entries, path) {
  for (const [key, value] of entries) {
    // `model` at the config root is the reserved key that selects the model
    // itself — part of the config format, not a per-model field — and the
    // model dropdown above the recipe editor already owns it, so a form row
    // for it here would just be a redundant, editable duplicate.
    if (path.length === 0 && key === "model") continue;
    const childPath = [...path, key];
    if (isPlainObject(value)) {
      const group = document.createElement("div");
      group.className = "group";
      const title = document.createElement("div");
      title.className = "group-title";
      title.textContent = key;
      const fields = document.createElement("div");
      fields.className = "form-fields";
      group.append(title, fields);
      buildGroup(fields, Object.entries(value), childPath);
      container.append(group);
    } else {
      container.append(fieldRow(key, childPath, buildWidget(value, childPath)));
    }
  }
}

function renderForm() {
  const container = $("form-fields");
  container.replaceChildren();
  if (!editor.obj || typeof editor.obj !== "object") return;
  buildGroup(container, Object.entries(editor.obj), []);
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
    renderForm();
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
