import { test } from "node:test";
import assert from "node:assert/strict";
import { nextMode, resolveTheme, THEMES } from "../../docs/playground/themes.js";

test("THEMES lists the three shipped families in picker order", () => {
  assert.deepEqual(THEMES.map((t) => t.id), ["patina", "oxide", "clinical"]);
});

test("a stored choice wins over everything", () => {
  assert.deepEqual(
    resolveTheme({ stored: { family: "oxide", mode: "light" }, parentDark: true }),
    { family: "oxide", mode: "light" },
  );
});

test("the parent page decides mode when nothing is stored", () => {
  assert.deepEqual(
    resolveTheme({ stored: {}, parentDark: true }),
    { family: "patina", mode: "dark" },
  );
  assert.deepEqual(
    resolveTheme({ stored: {}, parentDark: false }),
    { family: "patina", mode: "light" },
  );
});

test("with nothing stored and no parent, the default is Patina dark", () => {
  assert.deepEqual(resolveTheme({}), { family: "patina", mode: "dark" });
  assert.deepEqual(
    resolveTheme({ stored: {}, parentDark: null }),
    { family: "patina", mode: "dark" },
  );
});

test("family and mode resolve independently", () => {
  assert.deepEqual(
    resolveTheme({ stored: { family: "clinical" }, parentDark: true }),
    { family: "clinical", mode: "dark" },
  );
  assert.deepEqual(
    resolveTheme({ stored: { mode: "light" }, parentDark: true }),
    { family: "patina", mode: "light" },
  );
});

test("an unknown stored family falls back to the default", () => {
  assert.deepEqual(
    resolveTheme({ stored: { family: "bogus" }, parentDark: false }),
    { family: "patina", mode: "light" },
  );
});

test("the mode switch flips from either side", () => {
  // The half you click is not a choice of destination, it is the switch.
  // Clicking the lit half used to do nothing, which reads as a dead control.
  assert.equal(nextMode("dark"), "light");
  assert.equal(nextMode("light"), "dark");
  // Never a no-op, whatever it is handed.
  for (const from of ["dark", "light", undefined, "", "sepia"]) {
    assert.notEqual(nextMode(from), from);
    assert.ok(["dark", "light"].includes(nextMode(from)), `${from} -> off the two modes`);
  }
});
