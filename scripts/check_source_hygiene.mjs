#!/usr/bin/env node
// Mechanical hygiene checks over the playground's own sources, for the classes
// of rot a human reviewer reliably misses: a stylesheet rule for an element
// nobody builds any more, an export nothing imports, an em dash in copy a
// reader will see, and a comment that narrates the repository's history instead
// of stating what the code does.
//
// Deliberately not a linter. Each check here exists because the fault it names
// actually reached main, and each is written to have no false positives on a
// clean tree, so a red result always means work rather than tuning.
//
// Run by the docs workflow; also runnable by hand:
//   node scripts/check_source_hygiene.mjs
import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";

const APP = "docs/playground";

// Class names composed at runtime rather than written down, so no source file
// can ever mention them. highlight.js emits `hljs-<token scope>` from its
// grammars, and the stylesheet maps those onto this page's palette.
const RUNTIME_CLASS_PREFIXES = ["hljs-"];

/** Source with comments removed, so a check sees only what ships. */
export function stripComments(text) {
  return text
    .replace(/<!--[\s\S]*?-->/g, "")
    .replace(/\/\*[\s\S]*?\*\//g, "")
    // A `//` that follows a colon is a URL scheme, not a comment.
    .replace(/(^|[^:])\/\/.*$/gm, "$1");
}

/**
 * Class selectors in `css` that no markup or script ever names.
 *
 * A rule for an element nobody builds is invisible: it neither breaks a page
 * nor shows up in a diff review, and it outlives the feature it dressed. The
 * lookup is a whole-word search over every source that could add a class, so a
 * name assembled from a template (`files-row ${role.kind}`) still counts as
 * used wherever its parts are written down.
 */
export function deadCssClasses(css, sources) {
  const haystack = sources.join("\n");
  const names = new Set(
    [...css.matchAll(/\.([a-zA-Z][a-zA-Z0-9_-]*)/g)].map((m) => m[1]),
  );
  return [...names]
    .filter((name) => !RUNTIME_CLASS_PREFIXES.some((p) => name.startsWith(p)))
    .filter((name) => !new RegExp(`\\b${name}\\b`).test(haystack))
    .sort();
}

/** Every name `text` exports, covering both `export function f` and `export { a as b }`. */
export function exportedNames(text) {
  const declared = [...text.matchAll(/^export\s+(?:async\s+)?(?:function|const|let|class)\s+(\w+)/gm)]
    .map((m) => m[1]);
  const listed = [...text.matchAll(/^export\s*\{([^}]*)\}/gm)].flatMap((m) =>
    m[1]
      .split(",")
      .map((part) => part.trim())
      .filter(Boolean)
      // `a as b` publishes `b`; a bare `a` publishes itself.
      .map((part) => part.split(/\s+as\s+/).at(-1)),
  );
  return [...declared, ...listed];
}

/**
 * Exports no other module imports.
 *
 * An export is a promise that something outside needs this name. One nobody
 * imports is either dead or a private helper wearing a public label, and both
 * mislead the next reader about where a module's edges are.
 *
 * A module's own unit tests count as consumers: exporting a pure helper so it
 * can be tested directly is the point of splitting it out.
 */
export function unusedExports(modules, consumers) {
  const problems = [];
  for (const [name, text] of modules) {
    for (const symbol of exportedNames(text)) {
      const used = consumers.some(
        ([other, body]) => other !== name && new RegExp(`\\b${symbol}\\b`).test(body),
      );
      if (!used) problems.push(`${name}: exports \`${symbol}\`, which nothing imports`);
    }
  }
  return problems;
}

/**
 * The runs of a source that a reader can end up seeing: the text between an
 * HTML file's tags, and the contents of a script's string literals. Everything
 * else on those lines is markup and code, which is why a whole-line search
 * cannot tell a sentence from an attribute.
 */
export function readableRuns(name, text) {
  const body = stripComments(text);
  if (name.endsWith(".html")) return body.split(/<[^>]*>/);
  return [...body.matchAll(/"([^"\n]*)"|'([^'\n]*)'|`([^`]*)`/g)].map(
    (m) => m[1] ?? m[2] ?? m[3],
  );
}

/**
 * Em dashes in text a reader sees.
 *
 * House style: never in user-facing copy. Comments keep theirs, so the check
 * reads each source with its comments stripped. A dash standing alone is a
 * placeholder glyph rather than punctuation in a sentence, so a run is reported
 * only when the dash shares it with words.
 */
export function proseEmDashes(files) {
  const problems = [];
  for (const [name, text] of files) {
    for (const run of readableRuns(name, text)) {
      if (run.includes("—") && /[A-Za-z]/.test(run)) {
        problems.push(`${name}: em dash in user-facing text: ${run.trim().slice(0, 70)}`);
      }
    }
  }
  return problems;
}

// Phrasing that dates a comment to the moment it was written. A reader who did
// not watch the change cannot use any of it, and it decays into a claim about
// code that is no longer there.
const TEMPORAL = [
  /\bpreviously\b/i, /\bused to be\b/i, /\bformerly\b/i, /\bwe now\b/i,
  /\bthis (change|commit|PR)\b/i, /\brenamed from\b/i, /\bwas renamed\b/i,
  /\bin the old\b/i, /\bhas hit before\b/i, /\bno longer needed\b/i,
];

/** Comments that narrate history rather than state the current contract. */
export function temporalComments(files) {
  const problems = [];
  for (const [name, text] of files) {
    text.split("\n").forEach((line, i) => {
      const hit = TEMPORAL.find((re) => re.test(line));
      if (hit) problems.push(`${name}:${i + 1}: comment dates itself (${hit.source}): ${line.trim().slice(0, 70)}`);
    });
  }
  return problems;
}

function read(dir, pattern) {
  return readdirSync(dir, { withFileTypes: true })
    .filter((e) => e.isFile() && pattern.test(e.name))
    .map((e) => [`${dir}/${e.name}`, readFileSync(`${dir}/${e.name}`, "utf8")]);
}

// Build output, vendored code and generated bindings: not ours to hold to a
// house style.
const SKIP_DIRS = new Set(["target", "pkg", "vendor", "node_modules", "_build"]);

// The one file whose job is to contain the phrasing every other file must not.
const FIXTURES = "scripts/tests/source_hygiene.test.mjs";

function readTree(dir, pattern) {
  const out = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = `${dir}/${entry.name}`;
    if (entry.isDirectory()) {
      if (!SKIP_DIRS.has(entry.name)) out.push(...readTree(path, pattern));
    } else if (pattern.test(entry.name) && path !== FIXTURES) {
      out.push([path, readFileSync(path, "utf8")]);
    }
  }
  return out;
}

export function main() {
  const scripts = read(APP, /\.js$/);
  const markup = read(APP, /\.html$/);
  const tests = read("scripts/tests", /\.test\.mjs$/);
  const css = readFileSync(`${APP}/app.css`, "utf8");
  const sources = [...scripts, ...markup, ...read(`${APP}/vendor`, /\.js$/)];

  return [
    ...deadCssClasses(css, sources.map(([, body]) => body)).map(
      (c) => `${APP}/app.css: \`.${c}\` styles nothing any source builds`,
    ),
    ...unusedExports(scripts, [...scripts, ...markup, ...tests]),
    ...proseEmDashes([...scripts, ...markup]),
    // Comments are held to this everywhere, not just in the app: the rule is
    // about what a comment is for, and Rust is where most of them live.
    ...temporalComments([
      ...scripts,
      ...markup,
      ...readTree("crates", /\.rs$/),
      ...readTree("scripts", /\.(mjs|py)$/),
      ...readTree("ci", /\.(sh|py)$/),
    ]),
  ];
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const problems = main();
  if (problems.length) {
    console.error(`${problems.length} hygiene problem(s):`);
    for (const p of problems) console.error(" -", p);
    process.exit(1);
  }
  console.log("source hygiene ok: no dead rules, unused exports, stray em dashes or dated comments");
}
