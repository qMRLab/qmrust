import { test } from "node:test";
import assert from "node:assert/strict";
import { resolveTheme, THEMES } from "../../docs/playground/themes.js";

test("THEMES lists the three shipped families in picker order", () => {
  assert.deepEqual(THEMES.map((t) => t.id), ["patina", "oxide", "clinical"]);
});

test("a stored choice wins over everything", () => {
  assert.deepEqual(
    resolveTheme({ stored: { family: "oxide", mode: "light" }, parentDark: true, prefersDark: true }),
    { family: "oxide", mode: "light" },
  );
});

test("the parent page decides mode when nothing is stored", () => {
  assert.deepEqual(
    resolveTheme({ stored: {}, parentDark: true, prefersDark: false }),
    { family: "patina", mode: "dark" },
  );
  assert.deepEqual(
    resolveTheme({ stored: {}, parentDark: false, prefersDark: true }),
    { family: "patina", mode: "light" },
  );
});

test("the OS preference decides when there is no parent", () => {
  assert.deepEqual(
    resolveTheme({ stored: {}, parentDark: null, prefersDark: true }),
    { family: "patina", mode: "dark" },
  );
});

test("family and mode resolve independently", () => {
  assert.deepEqual(
    resolveTheme({ stored: { family: "clinical" }, parentDark: true, prefersDark: false }),
    { family: "clinical", mode: "dark" },
  );
});

test("an unknown stored family falls back to the default", () => {
  assert.deepEqual(
    resolveTheme({ stored: { family: "bogus" }, parentDark: null, prefersDark: false }),
    { family: "patina", mode: "light" },
  );
});
