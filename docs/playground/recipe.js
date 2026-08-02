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
import { icon } from "./vendor/icons.js";
import { $ } from "./dom.js";
import { app, editor } from "./state.js";
import { mergeSurface, withProtocolComments, stripProtocolComments, readNumbers, resolvedProtocolJson, clearProtocolOverrides } from "./surface.js";
import { debounce } from "./debounce.js";
import { inlineCodeHtml } from "./inline-code.js";
import { fieldHelp, fieldLabel, fieldUnit, groupLabel } from "./labels.js";

// Knob diameter in px, mirrored in `.override-knob`; the pointer guard and the
// knob's travel both need it as a number.
const KNOB_PX = 40;

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
  setYamlPill(true);
  refreshSurface();
  updateYamlView();
  renderForm();
}

// Sets the YAML textarea's displayed text to `editor.text` (the parse/commit
// source of truth) plus the protocol-context block for the current mode, and
// repaints the highlight layer to match. The appended block is a display
// artifact only — `editor.text` never carries it — so it is regenerated here
// from the latest surface on every call rather than being part of any stored
// state. Preserves the textarea's own caret/selection across the rewrite: a
// background surface refresh lands mid-typing-pause, and resetting `.value`
// unconditionally would otherwise yank the cursor out from under a reader who
// resumes typing.
function updateYamlView() {
  const ta = $("cfg-yaml");
  const displayed =
    app.protocolResolved && app.surface
      ? withProtocolComments(editor.text, yamlLoad(app.surface.yaml), app.surface.protocol_keys)
      : editor.text;
  if (ta.value !== displayed) {
    const focused = document.activeElement === ta;
    const { selectionStart, selectionEnd } = ta;
    ta.value = displayed;
    if (focused) {
      ta.focus({ preventScroll: true });
      try {
        ta.setSelectionRange(selectionStart, selectionEnd);
      } catch {
        // A selection range invalid for the new (shorter/longer) text is not
        // worth surfacing; the caret simply lands wherever the browser puts it.
      }
    }
  }
  // The textarea's own text is transparent — the highlight layer behind it is
  // what a reader actually reads, so leaving it unpainted shows them stale YAML.
  paintYaml();
}

// The message from the last `effective_config` call that threw outright
// (as opposed to returning a surface with `.error` set) — a recipe that
// parses as YAML but does not deserialize into the config, e.g. a wrong type
// or a misspelled enum. Kept apart from `app.surface`, which becomes `null` in
// this case so `renderForm` falls back to mirroring the recipe.
let surfaceThrow = null;

// Re-reads the model's option surface for the current recipe text. Cheap — a
// parse and a serialize, no fitting — so it runs after every committed edit.
// No wasm, or invalid YAML, leaves the previous surface untouched — there is
// nothing new to read yet. A recipe that parses as YAML but fails to
// deserialize into the config (unknown model, wrong type, misspelled enum)
// clears the surface, so `renderForm` falls back to mirroring the recipe
// rather than blanking the panel, and keeps the thrown message so the fallback
// is explained rather than merely survived.
function refreshSurface() {
  if (!app.wasm || !editor.valid) return;
  try {
    app.surface = app.wasm.effective_config(editor.text, resolvedProtocolJson(app));
    surfaceThrow = null;
  } catch (e) {
    app.surface = null;
    surfaceThrow = e?.message ?? String(e);
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
  updateYamlView();
  renderForm();
}, 150);

// The dotted config path is deliberately not shown: it is the serialization's
// business, the YAML view already spells it out verbatim, and a reader of the
// form wants the quantity, not the key that stores it.
function fieldRow(path, widget) {
  const row = document.createElement("div");
  row.className = "field-row";
  const labelWrap = document.createElement("div");
  labelWrap.className = "field-labels";
  const labelSpan = document.createElement("span");
  labelSpan.className = "field-label";
  labelSpan.textContent = fieldLabel(path);
  labelWrap.append(labelSpan);
  // An option gets a hover saying what choosing it costs; an acquisition field
  // does not, because its name already is the quantity. Built as the same
  // `.info` button the panel headings use, which `wireTips` picks up by
  // delegation, so a row created after startup is covered without wiring.
  const help = fieldHelp(path);
  if (help) {
    const info = document.createElement("button");
    info.type = "button";
    info.className = "info";
    info.setAttribute("aria-label", help);
    info.dataset.tip = help;
    info.innerHTML = icon("info", 12);
    labelSpan.append(" ");
    labelSpan.append(info);
  }
  // The symbol and unit sit under the quantity rather than trailing it in
  // parentheses: the name is what a reader scans for, and a row that also
  // carries the BIDS mark was running to three pieces on one line. The second
  // line is where anything *about* the value goes, the mark included.
  const unit = fieldUnit(path);
  if (unit) {
    const meta = document.createElement("span");
    meta.className = "field-meta";
    const sym = document.createElement("span");
    sym.className = "field-unit";
    sym.textContent = unit;
    meta.append(sym);
    labelWrap.append(meta);
  }
  row.append(labelWrap, widget);
  return row;
}

// The BIDS mark, standing in for the words "from sidecars": this value came
// from the dataset, not from the recipe.
//
// Drawn as a background image rather than an `<img>` so the stylesheet can pick
// the artwork that suits the current mode — the mark ships as dark ink and as
// white, and only CSS knows which one the panel behind it needs. `role`/
// `aria-label` keep it an image to a screen reader, which a bare span is not.
function bidsBadge() {
  const badge = document.createElement("span");
  badge.className = "bids-badge";
  badge.setAttribute("role", "img");
  badge.setAttribute("aria-label", "BIDS");
  badge.title = "resolved from the dataset's BIDS sidecars";
  return badge;
}

// Put the BIDS mark on the label's second line, beside the symbol. A field
// with no symbol has no second line yet, so one is made for it: the mark is a
// statement about where the value came from, which belongs there either way.
function attachBadge(row) {
  const labels = row.querySelector(".field-labels");
  if (!labels) return;
  let meta = labels.querySelector(".field-meta");
  if (!meta) {
    meta = document.createElement("span");
    meta.className = "field-meta";
    labels.append(meta);
  }
  meta.prepend(bidsBadge());
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
    // A number array reads best two different ways. At rest it is one compact
    // line, so a long acquisition axis doesn't dominate the panel; while it is
    // being edited it is one value per line, where a reader can see the count,
    // scan the values against each other, and edit a single entry without
    // counting commas. Both are the same textarea, so the element identity —
    // and hence focus restoration across a re-render — survives the switch.
    const ta = document.createElement("textarea");
    ta.className = "num-array";
    const collapse = () => {
      ta.value = readNumbers(ta.value).join(", ");
      ta.rows = 1;
      ta.classList.remove("editing");
    };
    const expand = () => {
      const nums = readNumbers(ta.value);
      ta.value = nums.join("\n");
      // Cap the height so a 30-echo series scrolls rather than pushing the
      // rest of the recipe off-screen.
      ta.rows = Math.min(Math.max(nums.length, 2), 12);
      ta.classList.add("editing");
    };
    ta.value = value.join(", ");
    ta.rows = 1;
    ta.addEventListener("focus", expand);
    ta.addEventListener("blur", () => {
      const parsed = readNumbers(ta.value);
      collapse();
      // Only commit a real change: a focus-and-leave would otherwise rebuild
      // the form and rewrite the recipe for nothing.
      if (String(parsed) !== String(value)) {
        setAtPath(editor.obj, path, parsed);
        commitObjEdit();
      }
    });
    return ta;
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
    } catch {
      // Leave the underlying object untouched; the textarea keeps the
      // reader's (currently unparsable) text so they can keep fixing it.
    }
  };
  return ta;
}

// `parentLocked` is true while recursing into a group whose own row already
// carried the "from sidecars" note — mergeSurface's `inherited` flag marks
// every descendant of a locked group as protocol-sourced too (ingest_protocol
// replaces the whole object), so without this a reader would see the same
// note once on the group and again on every one of its children.
function buildRows(container, rows, parentLocked = false) {
  for (const row of rows) {
    if (row.children) {
      const group = document.createElement("div");
      group.className = "group";
      const title = document.createElement("div");
      title.className = "group-title";
      title.textContent = groupLabel(row.key);
      const fields = document.createElement("div");
      fields.className = "form-fields";
      group.append(title, fields);
      buildRows(fields, row.children, parentLocked || row.readOnly);
      container.append(group);
      if (row.readOnly) {
        group.classList.add("locked");
        if (!parentLocked) {
          title.append(bidsBadge());
        }
      }
      continue;
    }
    const widget = buildWidget(row.value, row.path);
    // The actual focusable control — the widget itself for every shape except
    // a checkbox, whose returned element is the `<label>` wrapping it. Stamped
    // once, here, rather than in every `buildWidget` branch: a rebuild is a
    // fresh set of nodes, and `renderForm` needs a way to find "the same row"
    // in the new tree that doesn't depend on the old (about to be destroyed)
    // node's identity.
    const focusable = widget.matches("input, select, textarea")
      ? widget
      : widget.querySelector("input, select, textarea");
    if (focusable) focusable.dataset.path = row.path.join(".");
    // A protocol-sourced control gets a distinct border regardless of mode —
    // editable in the non-BIDS case, disabled in the BIDS one — so a reader
    // can tell an acquisition field from an ordinary option at a glance.
    if (row.isProtocol) (focusable ?? widget).classList.add("protocol-field");
    if (row.readOnly) {
      // Disabled on the focusable control itself, not the wrapping `<label
      // class="switch">` a checkbox returns — disabling the label is a no-op:
      // the checkbox inside stays keyboard-focusable and space-togglable.
      const target = focusable ?? widget;
      target.disabled = true;
      target.title = "supplied by the dataset's sidecars";
    }
    const el = fieldRow(row.path, widget);
    if (!row.isSet) el.classList.add("unset");
    if (row.readOnly) {
      el.classList.add("locked");
      if (!parentLocked) {
        attachBadge(el);
      }
    }
    container.append(el);
  }
}

// What a rebuild must not disturb: which row held focus, and — for a text
// control — where the cursor/selection sat within it. Read by dotted path
// rather than DOM identity, since the node itself is about to be replaced.
function captureFocus(container) {
  const active = document.activeElement;
  if (!active || !container.contains(active)) return null;
  const target = active.closest("[data-path]");
  if (!target) return null;
  const snapshot = { path: target.dataset.path };
  if (typeof target.selectionStart === "number") {
    snapshot.selectionStart = target.selectionStart;
    snapshot.selectionEnd = target.selectionEnd;
  }
  return snapshot;
}

// Re-applies a `captureFocus` snapshot to whichever new node now carries the
// same path, if any still does — a key the edit just removed (or locked into
// a disabled control) simply has nowhere to restore to.
function restoreFocus(container, snapshot) {
  if (!snapshot) return;
  const target = Array.from(container.querySelectorAll("[data-path]"))
    .find((el) => el.dataset.path === snapshot.path);
  if (!target) return;
  target.focus({ preventScroll: true });
  if (snapshot.selectionStart == null || typeof target.setSelectionRange !== "function") return;
  try {
    target.setSelectionRange(snapshot.selectionStart, snapshot.selectionEnd);
  } catch {
    // A control that reports a `selectionStart` but rejects a matching range
    // (e.g. a differently-typed input after a shape change) keeps its focus;
    // losing the caret position alone is not worth surfacing.
  }
}

function renderForm() {
  showConfigError(app.surface?.error ?? surfaceThrow);
  const container = $("form-fields");
  if (!editor.obj || typeof editor.obj !== "object") {
    container.replaceChildren();
    return;
  }

  // Without a surface (no wasm, or a model the registry does not know) fall
  // back to mirroring the recipe — the panel degrades, it does not vanish.
  const surface = app.surface ? yamlLoad(app.surface.yaml) : editor.obj;
  const rows = mergeSurface(surface, editor.obj, {
    protocolKeys: app.surface?.protocol_keys ?? [],
    readOnly: app.protocolResolved && !app.overrideProtocol,
  });

  // Every render rebuilds the whole subtree from the merged rows — the only
  // way every widget's displayed value, not just its `unset` styling, is
  // guaranteed to match what the recipe and the model's own surface now say.
  // What a rebuild must not do is drop keyboard focus out of the panel: a
  // text/number input's `onchange` fires at blur, just before the browser
  // finishes a Tab-driven focus move onto the next control, and this same
  // function also runs from a debounced refresh that can land after the
  // reader has since focused a different field entirely. Captured and
  // restored by path, across every rebuild, regardless of what triggered it.
  const focus = captureFocus(container);
  container.replaceChildren();
  buildRows(container, rows);
  // Only offered when there is something to unlock: a resolved protocol, and
  // at least one field it supplies.
  if (app.protocolResolved && rows.some((r) => r.isProtocol)) {
    container.append(overrideControl());
  }
  restoreFocus(container, focus);
}

// The knob's glyph points the way the sweep goes: right to override, left to
// come back. Direction is the whole instruction, so it beats a padlock, which
// only says which state you are already in.
function arrowIcon(pointsLeft) {
  const NS = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(NS, "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("aria-hidden", "true");
  const chevron = document.createElementNS(NS, "path");
  chevron.setAttribute("d", pointsLeft ? "M14 6 L8 12 L14 18" : "M10 6 L16 12 L10 18");
  chevron.setAttribute("fill", "none");
  chevron.setAttribute("stroke", "currentColor");
  chevron.setAttribute("stroke-width", "2.5");
  chevron.setAttribute("stroke-linecap", "round");
  chevron.setAttribute("stroke-linejoin", "round");
  svg.append(chevron);
  return svg;
}

// Slide to override: a deliberate sweep rather than a click, because unlocking
// these fields lets the recipe contradict the dataset it is fitted against —
// and a value typed here is recorded in the output provenance as if the
// acquisition said so. The same control slides back to re-lock, so the gesture
// reads the same in both directions.
//
// A range input carries the interaction, so the control is keyboard- and
// screen-reader-operable without a hand-rolled drag; the visible knob and
// track are drawn separately, since a native thumb cannot hold an icon.
function overrideControl() {
  const unlocked = app.overrideProtocol;
  const wrap = document.createElement("div");
  wrap.className = unlocked ? "override unlocked" : "override";

  const slider = document.createElement("input");
  slider.type = "range";
  slider.className = "override-slider";
  slider.min = "0";
  slider.max = "100";
  slider.value = unlocked ? "100" : "0";
  slider.setAttribute(
    "aria-label",
    unlocked ? "Slide to lock the protocol" : "Slide to edit the protocol",
  );

  const knob = document.createElement("span");
  knob.className = "override-knob";
  knob.append(arrowIcon(unlocked));

  const label = document.createElement("span");
  label.className = "override-label";
  label.textContent = unlocked ? "Slide to Lock Protocol" : "Slide to Edit Protocol";

  const setProgress = (v) => {
    // Travel drives the knob's position and fades the prompt, so a partial
    // sweep reads as progress rather than as a control ignoring the drag.
    wrap.style.setProperty("--p", String(v / 100));
    const reached = unlocked ? 1 - v / 100 : v / 100;
    label.style.opacity = String(Math.max(0, 1 - reached * 1.6));
  };
  setProgress(Number(slider.value));

  const commit = () => {
    const v = Number(slider.value);
    const done = unlocked ? v <= 0 : v >= 100;
    if (done) {
      app.overrideProtocol = !unlocked;
      if (unlocked) {
        // Coming back to the dataset: drop whatever the recipe overrode, so
        // the sidecar values are the only source again and the provenance
        // stops carrying an acquisition the dataset never stated.
        clearProtocolOverrides(editor.obj, app.surface?.protocol_keys ?? []);
        commitObjEdit();
        return;
      }
      refreshSurface();
      updateYamlView();
      renderForm();
      return;
    }
    // Abandoned mid-sweep: spring back to where it started.
    slider.value = unlocked ? "100" : "0";
    setProgress(Number(slider.value));
  };

  // A range input jumps to wherever the track is clicked, which would turn a
  // safety gesture into a single click at the far end. Only a press that
  // starts on the knob may move it; keyboard operation is untouched.
  slider.addEventListener("pointerdown", (e) => {
    const r = slider.getBoundingClientRect();
    const travel = r.width - KNOB_PX;
    const knobCentre = r.left + KNOB_PX / 2 + (Number(slider.value) / 100) * travel;
    if (Math.abs(e.clientX - knobCentre) > KNOB_PX / 2 + 6) e.preventDefault();
  });
  slider.addEventListener("input", () => {
    setProgress(Number(slider.value));
    const v = Number(slider.value);
    if (unlocked ? v <= 0 : v >= 100) commit();
  });
  slider.addEventListener("change", commit);

  wrap.append(slider, knob, label);
  return wrap;
}

// Backticked terms are set as `<code>` here exactly as they are in the notice
// dialog: the same messages reach both surfaces, and a config key that reads as
// code in one and as stray punctuation in the other is the app disagreeing with
// itself about what it just said.
function showConfigError(message) {
  const box = $("cfg-error");
  box.innerHTML = message ? inlineCodeHtml(message) : "";
  box.hidden = !message;
}

function setYamlPill(valid) {
  const pill = $("yaml-pill");
  pill.textContent = valid ? "valid" : "invalid";
  pill.classList.toggle("valid", valid);
  pill.classList.toggle("invalid", !valid);
}

// Sets the editor from a fresh string (model load, or typed into the YAML
// view, already stripped of any protocol-comment block — see
// `stripProtocolComments`). Rebuilds the form only when the text parses, so a
// reader mid-typo in the YAML view doesn't have the form yanked out from
// under them. `scheduleSurfaceRefresh` (on its synchronous leading call, e.g.
// a model load or the first keystroke of a burst) is what actually rewrites
// the textarea via `updateYamlView`; this still repaints the highlight layer
// on every call so it never lags behind what the reader is looking at.
export function setEditorText(text) {
  editor.text = text;
  try {
    editor.obj = yamlLoad(text);
    editor.valid = true;
    setYamlPill(true);
    scheduleSurfaceRefresh();
  } catch {
    editor.valid = false;
    setYamlPill(false);
  }
  paintYaml();
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
