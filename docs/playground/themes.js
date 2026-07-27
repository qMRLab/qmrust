// Which theme applies, and applying it. A theme is data — the values live in
// app.css, keyed by `data-theme`/`data-mode`. This module owns the list, the
// resolution order, and persistence.
//
// It imports nothing, deliberately. What must repaint when the theme changes is
// the wiring module's knowledge, delivered through `onThemeChange`; and an import
// of `state.js` would drag a WebGL context into a pure resolver, which would make
// this module impossible to unit-test outside a browser.

// Values live in app.css; this is the list, so the two cannot disagree about a
// token. `scripts/check_theme_contrast.mjs` asserts they agree both ways.
export const THEMES = [
  { id: "patina", label: "Patina" },
  { id: "oxide", label: "Oxide" },
  { id: "clinical", label: "Clinical" },
];

const DEFAULT_FAMILY = "patina";
// Dark unless something says otherwise. The images are the point of this page
// and they are read on a dark ground, so dark is the right resting state — not
// whatever the operating system happens to prefer.
const DEFAULT_MODE = "dark";
const KEY_FAMILY = "qmrust-theme-family";
const KEY_MODE = "qmrust-theme-mode";

// Pure: no DOM, no storage, no clock. `parentDark` is null when there is no
// reachable parent page — the standalone /app/index.html case — which is
// different from a parent that is light.
export function resolveTheme({ stored = {}, parentDark = null }) {
  const known = THEMES.some((t) => t.id === stored.family);
  const family = known ? stored.family : DEFAULT_FAMILY;
  const mode = stored.mode === "light" || stored.mode === "dark"
    ? stored.mode
    : (parentDark ?? DEFAULT_MODE === "dark") ? "dark" : "light";
  return { family, mode };
}

// The MyST book theme uses Tailwind's class strategy, so dark is a `dark` class
// on the parent's <html>. Same-origin, therefore readable — but a cross-origin
// or absent parent must yield "no opinion" rather than throwing or reading as
// light.
function readParentDark() {
  try {
    if (window.parent === window) return null;
    return window.parent.document.documentElement.classList.contains("dark");
  } catch {
    return null;
  }
}

// Storage can throw outright in some privacy modes, so every access is guarded
// and the app degrades to a session-only choice rather than failing to start.
function readStored() {
  try {
    return {
      family: localStorage.getItem(KEY_FAMILY) ?? undefined,
      mode: localStorage.getItem(KEY_MODE) ?? undefined,
    };
  } catch {
    return {};
  }
}

// What must repaint once the attributes change. Registered by the wiring module,
// so this one needs no knowledge of viewers, charts or level controls.
const listeners = [];
export function onThemeChange(fn) {
  listeners.push(fn);
}

export function applyTheme({ family, mode }) {
  const root = document.documentElement;
  root.dataset.theme = family;
  root.dataset.mode = mode;
  for (const fn of listeners) fn();
}

function current() {
  return resolveTheme({ stored: readStored(), parentDark: readParentDark() });
}

export function setFamily(id) {
  try {
    localStorage.setItem(KEY_FAMILY, id);
  } catch {
    // Private mode: the choice lasts for this session only.
  }
  applyTheme({ ...current(), family: id });
}

export function setMode(mode) {
  try {
    if (mode === "auto") localStorage.removeItem(KEY_MODE);
    else localStorage.setItem(KEY_MODE, mode);
  } catch {
    // Private mode: the choice lasts for this session only.
  }
  applyTheme(mode === "auto" ? current() : { ...current(), mode });
}

export function initTheme() {
  applyTheme(current());
  // Follow the docs page's own light/dark toggle, but only until the reader
  // states a preference here — after that, theirs wins.
  try {
    if (window.parent === window) return;
    new MutationObserver(() => {
      if (readStored().mode) return;
      applyTheme(current());
    }).observe(window.parent.document.documentElement, {
      attributes: true,
      attributeFilter: ["class"],
    });
  } catch {
    // Cross-origin parent: nothing to follow.
  }
}
