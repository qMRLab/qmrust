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
  stripProducerCalls,
  temporalComments,
  unescapedHtmlSinks,
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

// The app renders filenames, sidecar contents and entity values out of a
// dataset a stranger supplied, so an `innerHTML` that does not escape is the
// one XSS this code can plausibly grow. Both columns matter equally: a check
// that flags the safe forms gets switched off, and one that misses the unsafe
// forms was never protecting anything.
const SAFE = [
  ["escaped by a producer", 'x.innerHTML = inlineCodeHtml(msg);'],
  ["a ternary whose branches are both safe", 'box.innerHTML = m ? inlineCodeHtml(m) : "";'],
  ["a glyph beside written-down text", 'b.innerHTML = `${icon("eye", 16)}<span>Segment</span>`;'],
  // The interpolation closes on its second brace; the object literal's is the first.
  ["a highlighter call carrying an options object", 'h.innerHTML = `${hljs.highlight(t, { language: "yaml" }).value}\\n`;'],
  ["a written-down string", 'x.innerHTML = "<b>ready</b>";'],
];

const UNSAFE = [
  ["a bare variable", "x.innerHTML = filename;"],
  ["dataset text interpolated into markup", "x.innerHTML = `<b>${role.reason}</b>`;"],
  ["string concatenation", 'x.innerHTML = "<b>" + name + "</b>";'],
  ["a function that merely sounds like it escapes", "x.innerHTML = escapeIsh(name);"],
  ["an unsafe branch hiding behind a safe one", 'x.innerHTML = m ? inlineCodeHtml(m) : raw;'],
  ["a safe producer's output concatenated with raw text", 'x.innerHTML = icon("eye", 16) + name;'],
];

for (const [label, src] of SAFE) {
  test(`innerHTML allows ${label}`, () => {
    assert.deepEqual(unescapedHtmlSinks([["t.js", src]]), []);
  });
}

for (const [label, src] of UNSAFE) {
  test(`innerHTML flags ${label}`, () => {
    assert.equal(unescapedHtmlSinks([["t.js", src]]).length, 1, src);
  });
}

test("a producer's arguments go with it, since they passed through the escaping", () => {
  assert.match(stripProducerCalls("inlineCodeHtml(message)"), /^\s*$/);
  // Only the listed producers earn that; a lookalike keeps its argument.
  assert.match(stripProducerCalls("inlineCodeHtmlish(message)"), /message/);
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
