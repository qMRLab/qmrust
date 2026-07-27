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
