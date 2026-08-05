// An icon is named as a string, and `icon()` throws on a name the vendored set
// does not carry. Nothing type-checks those names, so a typo reaches the reader
// as a blank control or a thrown render — which is what these cover, by resolving
// every name the app actually asks for rather than a list kept alongside.
//
// Run: node --test scripts/tests/*.test.mjs
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { icon, iconPaths } from "../../docs/playground/vendor/icons.js";
import { SIM_MODES } from "../../docs/playground/sim-series.js";

test("every simulation mode's tab glyph is one the vendored set carries", () => {
  for (const m of SIM_MODES) {
    assert.doesNotThrow(() => icon(m.icon, 14), `${m.id}: icon "${m.icon}" is not vendored`);
  }
});

test("every data-icon in the markup names a vendored glyph", () => {
  // `paintIcons` resolves these at startup, so one bad name throws before the
  // page finishes wiring.
  const html = readFileSync(new URL("../../docs/playground/index.html", import.meta.url), "utf8");
  const names = new Set([...html.matchAll(/data-icon="([\w-]+)"/g)].map((m) => m[1]));
  assert.ok(names.size > 0, "the markup declares no icons at all, so this proves nothing");
  for (const name of names) {
    assert.doesNotThrow(() => icon(name), `markup asks for icon "${name}", which is not vendored`);
  }
});

test("a glyph the skeleton hit-tests carries path data, not only rects", () => {
  // The umbrella void is cut by testing `Path2D`s built from these. A glyph
  // drawn only from `<rect>`/`<circle>` yields no path data, and would cut
  // nothing at all rather than failing loudly.
  assert.ok(iconPaths("umbrella").length > 0);
});

test("an unknown name fails loudly rather than rendering an empty glyph", () => {
  assert.throws(() => icon("not-an-icon"), /no such icon/);
  assert.throws(() => iconPaths("not-an-icon"), /no such icon/);
});
