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

// The only expressions allowed to produce HTML. Each either escapes its input
// (`inlineCodeHtml`), or renders from a closed set this repo owns (`icon`), or
// is a library whose whole contract is escaping what it highlights (`hljs`,
// `highlightJson`).
const HTML_PRODUCERS = ["icon", "inlineCodeHtml", "highlightJson", "hljs.highlight"];

/**
 * `value` with every producer call, and its arguments, removed.
 *
 * The arguments go with the call: `inlineCodeHtml(message)` is safe precisely
 * because `message` passed through it. Parens are matched by counting, since a
 * regex cannot see the end of a nested call.
 */
export function stripProducerCalls(value) {
  for (const producer of HTML_PRODUCERS) {
    let at;
    while ((at = value.indexOf(`${producer}(`)) !== -1) {
      let depth = 0;
      let i = at + producer.length;
      for (; i < value.length; i++) {
        if (value[i] === "(") depth++;
        else if (value[i] === ")" && --depth === 0) break;
      }
      if (i >= value.length) return value; // unbalanced: leave it to be reported
      value = value.slice(0, at) + value.slice(i + 1);
    }
  }
  // `hljs.highlight(...).value` leaves a trailing property access behind.
  return value.replace(/^\.\w+|\.\w+/g, "");
}

/**
 * The parts of `value` that are not written-down text: everything outside a
 * literal, plus each `${...}` inside a template.
 *
 * A template's own characters are authored here and safe by construction; its
 * interpolations are not. Scanned rather than matched, because `${hljs.highlight(
 * t, { language: "yaml" }).value}` closes on its second brace, and a regex
 * stops at the first.
 */
export function dynamicParts(value) {
  const parts = [];
  let buf = "";
  for (let i = 0; i < value.length; i++) {
    const c = value[i];
    if (c === '"' || c === "'") {
      while (++i < value.length && value[i] !== c);
      continue;
    }
    if (c === "`") {
      while (++i < value.length && value[i] !== "`") {
        if (value[i] !== "$" || value[i + 1] !== "{") continue;
        let depth = 1;
        const start = (i += 2);
        while (i < value.length && depth > 0) {
          if (value[i] === "{") depth++;
          else if (value[i] === "}") depth--;
          if (depth > 0) i++;
        }
        parts.push(value.slice(start, i));
      }
      continue;
    }
    buf += c;
  }
  parts.push(buf);
  return parts;
}

/**
 * `expr` with a leading ternary condition removed: in `a ? b : c` only `b` and
 * `c` reach the DOM. Nested ternaries are left whole, which can only over-report.
 */
function withoutCondition(expr) {
  const q = expr.indexOf("?");
  return q !== -1 && expr.includes(":") ? expr.slice(q + 1) : expr;
}

/**
 * Assignments to `innerHTML` from anything but a known-escaping producer.
 *
 * The app reads datasets a stranger hands it: filenames, sidecar JSON, entity
 * values, error text quoting all three. Those reach the DOM constantly, and
 * `textContent` makes them harmless. `innerHTML` does not, so every use has to
 * be traceable to something that escapes. This check is the reason a reviewer
 * can trust that; it is not a parser, and a sufficiently indirect assignment
 * gets past it, so it guards the honest mistake rather than the determined one.
 */
export function unescapedHtmlSinks(files) {
  const problems = [];
  for (const [name, text] of files) {
    stripComments(text)
      .split("\n")
      .forEach((line, i) => {
        const m = line.match(/\.innerHTML\s*=\s*(.+)$/);
        if (!m) return;
        const value = m[1].trim();
        const dynamic = dynamicParts(withoutCondition(value))
          .map((part) => stripProducerCalls(part))
          .join("");
        // What survives is an expression no producer accounted for. Punctuation
        // and a trailing semicolon are not names.
        if (/[A-Za-z_$]/.test(dynamic)) {
          problems.push(`${name}:${i + 1}: innerHTML from an unescaped source: ${value.slice(0, 60)}`);
        }
      });
  }
  return problems;
}

/**
 * `css` with every `@media`/`@supports` wrapper removed but its rules kept
 * (so a rule inside a breakpoint is seen exactly like one outside it), and
 * every `@keyframes` block dropped whole (its `0% { ... }` steps are not
 * selector rules, and would otherwise be misread as one).
 */
export function flattenAtRules(css) {
  let out = "";
  let i = 0;
  while (i < css.length) {
    const brace = css.indexOf("{", i);
    if (brace === -1) {
      out += css.slice(i);
      break;
    }
    const prelude = css.slice(i, brace);
    if (/@keyframes/.test(prelude)) {
      let depth = 1;
      let j = brace + 1;
      while (j < css.length && depth > 0) {
        if (css[j] === "{") depth++;
        else if (css[j] === "}") depth--;
        j++;
      }
      i = j;
      continue;
    }
    if (/^\s*@(media|supports)/.test(prelude)) {
      let depth = 1;
      let j = brace + 1;
      const bodyStart = j;
      while (j < css.length && depth > 0) {
        if (css[j] === "{") depth++;
        else if (css[j] === "}") depth--;
        if (depth > 0) j++;
      }
      out += flattenAtRules(css.slice(bodyStart, j));
      i = j + 1;
      continue;
    }
    let depth = 1;
    let j = brace + 1;
    while (j < css.length && depth > 0) {
      if (css[j] === "{") depth++;
      else if (css[j] === "}") depth--;
      j++;
    }
    out += css.slice(i, j);
    i = j;
  }
  return out;
}

/**
 * Selectors that set `display` unconditionally, and selectors that set
 * `display: none` behind `[hidden]` — the two halves a card's hiding rule
 * needs, both read from the stylesheet's own rules rather than assumed.
 */
function cssDisplayRules(css) {
  const unconditional = new Set();
  const hiddenNone = new Set();
  for (const m of flattenAtRules(stripComments(css)).matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    const body = m[2];
    if (!/display\s*:/.test(body)) continue;
    const setsNone = /display\s*:\s*none\b/.test(body);
    for (const raw of m[1].split(",")) {
      const sel = raw.trim().replace(/\s+/g, " ");
      if (!sel) continue;
      if (sel.includes("[hidden]")) {
        if (setsNone) hiddenNone.add(sel.replace("[hidden]", "").trim());
      } else {
        unconditional.add(sel);
      }
    }
  }
  return { unconditional, hiddenNone };
}

/** `id -> its class list`, read from every element the markup declares both on. */
function idClassMap(htmlSources) {
  const map = new Map();
  for (const [, raw] of htmlSources) {
    const text = stripComments(raw);
    for (const tag of text.match(/<[a-zA-Z][^>]*>/g) ?? []) {
      const idM = tag.match(/\sid="([\w-]+)"/);
      if (!idM) continue;
      const classM = tag.match(/\sclass="([^"]*)"/);
      map.set(idM[1], classM ? classM[1].split(/\s+/).filter(Boolean) : []);
    }
  }
  return map;
}

/**
 * One group of CSS selectors per element the scripts toggle `.hidden` on:
 * the id itself and every class the markup puts on that same id, so that a
 * `[hidden]` override stated against any one of them still counts.
 *
 * Traced patterns: `$("id").hidden = `; a `const x = $("id")` binding
 * followed by `x.hidden = ` in the same file; a `x.className = "a b"`
 * literal followed by `x.hidden = `; and the `for (const [id, hidden] of
 * [["a", ...], ["b", ...]]) { $(id).hidden = hidden; }` shape used to hide a
 * whole set of cards together. An id assembled at runtime from a prefix
 * (`` $(`${prefix}-readout`) ``) is out of reach: the prefix and the
 * resulting element's class never appear together in any script, so nothing
 * in the source connects them.
 */
function hiddenToggleGroups(scripts, idClasses) {
  const groups = [];
  const seen = new Set();
  const addId = (id) => {
    const key = `#${id}`;
    if (seen.has(key)) return;
    seen.add(key);
    groups.push([key, ...(idClasses.get(id) ?? []).map((c) => `.${c}`)]);
  };
  const addClasses = (classes) => {
    const key = classes.join(".");
    if (seen.has(key)) return;
    seen.add(key);
    groups.push(classes.map((c) => `.${c}`));
  };
  for (const [, raw] of scripts) {
    const text = stripComments(raw);
    for (const m of text.matchAll(/\$\(\s*"([\w-]+)"\s*\)\s*\.hidden\s*=/g)) addId(m[1]);

    const idVars = new Map();
    for (const m of text.matchAll(/(?:const|let)\s+(\w+)\s*=\s*\$\(\s*"([\w-]+)"\s*\)/g)) {
      idVars.set(m[1], m[2]);
    }
    const classVars = new Map();
    for (const m of text.matchAll(/(\w+)\.className\s*=\s*"([^"$]*)"/g)) {
      classVars.set(m[1], m[2].split(/\s+/).filter(Boolean));
    }
    for (const m of text.matchAll(/\b(\w+)\.hidden\s*=/g)) {
      if (idVars.has(m[1])) addId(idVars.get(m[1]));
      else if (classVars.has(m[1])) addClasses(classVars.get(m[1]));
    }

    for (const m of text.matchAll(
      /for\s*\(\s*const\s*\[\s*(\w+)\s*,\s*\w+\s*\]\s*of\s*\[([\s\S]*?)\]\s*\)\s*\{([\s\S]*?)\}/g,
    )) {
      const [, idVar, arrText, body] = m;
      if (!new RegExp(`\\$\\(\\s*${idVar}\\s*\\)\\s*\\.hidden\\s*=`).test(body)) continue;
      for (const im of arrText.matchAll(/\[\s*"([\w-]+)"/g)) addId(im[1]);
    }
  }
  return groups;
}

/**
 * A card the scripts hide with `.hidden = true` whose stylesheet rule still
 * shows it.
 *
 * `[hidden]` is a UA rule at specificity 0,1,0: any class rule that sets
 * `display` outranks it, so setting the attribute changes nothing unless the
 * stylesheet states its own `[hidden]` case. Every element a script hides
 * this way must carry that companion rule; this check is what verifies it
 * rather than leaving it to be remembered by convention alone.
 */
export function hiddenDisplayGaps(css, htmlSources, scripts) {
  const { unconditional, hiddenNone } = cssDisplayRules(css);
  const groups = hiddenToggleGroups(scripts, idClassMap(htmlSources));
  const problems = [];
  for (const selectors of groups) {
    const shown = selectors.find((s) => unconditional.has(s));
    if (!shown) continue;
    if (selectors.some((s) => hiddenNone.has(s))) continue;
    problems.push(
      `${APP}/app.css: \`${shown}\` sets display unconditionally but has no \`${shown}[hidden]\` rule, so hiding it does nothing`,
    );
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
    ...hiddenDisplayGaps(css, markup, scripts),
    ...proseEmDashes([...scripts, ...markup]),
    ...unescapedHtmlSinks(scripts),
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
