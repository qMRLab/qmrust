import { test } from "node:test";
import assert from "node:assert/strict";
import {
  parseColor, luminance, contrast, composite, parseThemeBlocks,
} from "../check_theme_contrast.mjs";

test("parseColor handles 6-digit, 3-digit and rgba", () => {
  assert.deepEqual(parseColor("#ffffff"), [255, 255, 255, 1]);
  assert.deepEqual(parseColor("#fff"), [255, 255, 255, 1]);
  assert.deepEqual(parseColor("#0c8290"), [12, 130, 144, 1]);
  assert.deepEqual(parseColor("rgba(15,26,25,.80)"), [15, 26, 25, 0.8]);
  assert.deepEqual(parseColor("rgb(1, 2, 3)"), [1, 2, 3, 1]);
});

test("luminance anchors at black and white", () => {
  assert.equal(luminance([0, 0, 0]), 0);
  assert.equal(luminance([255, 255, 255]), 1);
});

test("contrast is 21 for black on white and 1 for a colour on itself", () => {
  assert.equal(Math.round(contrast("#000000", "#ffffff")), 21);
  assert.equal(contrast("#7cc3b5", "#7cc3b5"), 1);
});

test("contrast matches a known measured pair", () => {
  // clinical light --accent after correction, against --bg
  assert.equal(contrast("#0c8290", "#eef1f4").toFixed(2), "4.02");
});

test("composite blends an alpha colour over its background", () => {
  assert.deepEqual(composite([0, 0, 0, 0.5], [255, 255, 255]), [128, 128, 128]);
  // oxide dark panel over the greenest gradient stop
  assert.deepEqual(composite(parseColor("rgba(15,26,25,.80)"), parseColor("#28564b")), [20, 38, 35]);
});

test("parseThemeBlocks keys blocks by family/mode and handles the shared default", () => {
  const css = `
:root,
:root[data-theme="patina"][data-mode="dark"] { --ink:#ece5da; --panel:#2c323a; }
:root[data-theme="clinical"][data-mode="light"] { --ink:#161b21; --panel:#fff; }
`;
  const blocks = parseThemeBlocks(css);
  assert.deepEqual([...blocks.keys()].sort(), ["clinical/light", "patina/dark"]);
  assert.equal(blocks.get("patina/dark").get("--ink"), "#ece5da");
  assert.equal(blocks.get("clinical/light").get("--panel"), "#fff");
});
