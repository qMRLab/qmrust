// A checker nobody has watched fail is a checker that passes for the wrong
// reason. Each case here reintroduces the exact fault its check exists for, so
// a rule that stops firing is caught here rather than by the next reviewer.
//
// Run: node --test scripts/tests/*.test.mjs
import test from "node:test";
import assert from "node:assert/strict";
import {
  deadCssClasses,
  exportedNames,
  main,
  proseEmDashes,
  readableRuns,
  stripComments,
  temporalComments,
  unusedExports,
} from "../check_source_hygiene.mjs";

test("the tree it guards is clean, so a failure is always real work", () => {
  assert.deepEqual(main(), []);
});

test("a rule for an element nobody builds is dead", () => {
  const css = ".field-key { display: none; }\n.field-label { opacity: 1; }";
  const sources = ['el.className = "field-label";'];
  assert.deepEqual(deadCssClasses(css, sources), ["field-key"]);
});

test("a class assembled from a template still counts as used", () => {
  // `files-row ${role.kind}` never spells `.volume` out next to `.files-row`,
  // but both parts are written down, so neither is dead.
  const css = ".files-row {}\n.volume {}";
  assert.deepEqual(
    deadCssClasses(css, ['row.className = `files-row ${role.kind}`;', 'case "volume":']),
    [],
  );
});

test("a class only the highlighter composes at runtime is not dead", () => {
  assert.deepEqual(deadCssClasses(".hljs-attr { color: red; }", [""]), []);
});

test("both export forms are read", () => {
  const src = [
    "export function a() {}",
    "export async function b() {}",
    "export const c = 1;",
    "export { d as e, f };",
  ].join("\n");
  assert.deepEqual(exportedNames(src), ["a", "b", "c", "e", "f"]);
});

test("an export nothing imports is reported, and its own module does not excuse it", () => {
  const modules = [["a.js", "export function helper() {}\nhelper();"]];
  assert.equal(unusedExports(modules, modules).length, 1);
});

test("a test file counts as a consumer", () => {
  const modules = [["a.js", "export function helper() {}"]];
  const consumers = [...modules, ["a.test.mjs", "import { helper } from '../a.js';"]];
  assert.deepEqual(unusedExports(modules, consumers), []);
});

test("an em dash in a status message is reported", () => {
  const problems = proseEmDashes([
    ["drop.js", 'status(`Loaded as ${pick} — also matches ${others}`);'],
  ]);
  assert.equal(problems.length, 1);
});

test("an em dash in a comment is the author's business", () => {
  assert.deepEqual(
    proseEmDashes([["a.js", "// a comment — with a dash\nconst x = 1;"]]),
    [],
  );
});

test("a dash standing alone is a placeholder, not punctuation", () => {
  assert.deepEqual(
    proseEmDashes([["index.html", '<span class="frame-label">—</span>']]),
    [],
  );
  // The same file's prose is still held to the rule.
  assert.equal(
    proseEmDashes([["index.html", "<p>The viewers work — fitting needs wasm.</p>"]]).length,
    1,
  );
});

test("an attribute is not prose, but the text beside it is", () => {
  assert.deepEqual(readableRuns("index.html", '<a href="a—b">plain</a>'), ["", "plain", ""]);
});

test("comments go, whatever their form, and a URL survives", () => {
  const src = "/* block */ const a = 1; // line\nconst u = 'https://x.example';";
  const out = stripComments(src);
  assert.ok(!out.includes("block") && !out.includes("line"));
  assert.ok(out.includes("https://x.example"));
});

test("a comment that narrates history is reported", () => {
  const problems = temporalComments([
    ["fit.js", "// This is a trap this codebase has hit before."],
    ["a.js", "// Previously we did it the other way."],
    ["ok.js", "// Bounds apply to both paths or neither."],
  ]);
  assert.equal(problems.length, 2);
  assert.ok(problems.every((p) => !p.startsWith("ok.js")));
});
