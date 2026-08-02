#!/usr/bin/env node
// Asserts that every playground theme keeps its text readable, and that the
// theme list and the CSS agree. Run by the docs workflow; also runnable by hand:
//   node scripts/check_theme_contrast.mjs
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

// Floors per token, and which surfaces each is actually drawn on.
//
// `--muted` is deliberately 3.0 rather than AA's 4.5: a 4.5-contrast "muted"
// grey no longer reads as muted, and the shipped clinical light theme has sat
// at 3.64 since it was written.
//
// Text can land on the bare page ground as well as on a panel, so it is held to
// both. Marks — series colours, the crosshair, the active-tool fill — only ever
// appear on a panel or a canvas, so measuring them against a textured ground
// they never touch would reject good palettes for an imaginary fault.
export const FLOORS = {
  "--ink": { floor: 4.5, on: "both" },
  "--ink-2": { floor: 4.5, on: "both" },
  "--muted": { floor: 3.0, on: "both" },
  "--accent": { floor: 3.0, on: "panel" },
  "--rust": { floor: 3.0, on: "panel" },
  "--brass": { floor: 3.0, on: "panel" },
};

// Every token a theme must define.
export const REQUIRED = [
  "--bg", "--panel", "--panel-2", "--field", "--ink", "--ink-2", "--muted",
  "--line", "--rust", "--accent", "--accent-ink", "--brass", "--good", "--bad",
  "--viewer-bg", "--radius", "--border-w", "--shadow",
  "--font-ui", "--font-h", "--font-mono", "--pad-y", "--pad-x",
  "--on-accent",
];

// Ink that sits on a filled surface rather than on a panel. Each pair is
// (ink token, fill token) and is held to the 4.5 text floor: these are button
// labels, small text however bold.
//
// This pair does *not* describe the tooltip, which is a translucent panel with
// `--ink` on it, covered by the panel floors above. A tinted fill at any
// usable alpha lightens over a pale panel until its copy fails, which is why
// the tip is not one.
const ON_PAIRS = [["--on-accent", "--accent"]];

export function parseColor(str) {
  const s = str.trim();
  let m = s.match(/^#([0-9a-f]{3})$/i);
  if (m) return [...m[1]].map((c) => parseInt(c + c, 16)).concat(1);
  m = s.match(/^#([0-9a-f]{6})$/i);
  if (m) {
    const n = parseInt(m[1], 16);
    return [(n >> 16) & 255, (n >> 8) & 255, n & 255, 1];
  }
  m = s.match(/^rgba?\(([^)]+)\)$/i);
  if (m) {
    const parts = m[1].split(",").map((p) => Number(p.trim()));
    return [parts[0], parts[1], parts[2], parts.length > 3 ? parts[3] : 1];
  }
  throw new Error(`cannot parse colour: ${str}`);
}

const toLinear = (c) => {
  const v = c / 255;
  return v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
};

export function luminance(rgb) {
  const [r, g, b] = rgb;
  return 0.2126 * toLinear(r) + 0.7152 * toLinear(g) + 0.0722 * toLinear(b);
}

export function contrast(a, b) {
  const la = luminance(typeof a === "string" ? parseColor(a) : a);
  const lb = luminance(typeof b === "string" ? parseColor(b) : b);
  const [hi, lo] = la >= lb ? [la, lb] : [lb, la];
  return (hi + 0.05) / (lo + 0.05);
}

export function composite(fg, bg) {
  const a = fg[3] ?? 1;
  return [0, 1, 2].map((i) => Math.round(a * fg[i] + (1 - a) * bg[i]));
}

// Every `:root…{…}` rule, keyed "family/mode". A rule whose selector list
// includes a bare `:root` is the pre-JS default and is keyed by its attribute
// selector, so the shared block is counted once.
export function parseThemeBlocks(cssText) {
  const blocks = new Map();
  const re = /((?::root[^{}]*?,\s*)*:root[^{}]*?)\{([^}]*)\}/g;
  for (const [, selector, body] of cssText.matchAll(re)) {
    const m = selector.match(/\[data-theme="([a-z-]+)"\]\[data-mode="(light|dark)"\]/);
    if (!m) continue;
    const tokens = new Map();
    for (const [, name, value] of body.matchAll(/(--[a-z0-9-]+)\s*:\s*([^;]+);/gi)) {
      tokens.set(name, value.trim());
    }
    const key = `${m[1]}/${m[2]}`;
    blocks.set(key, new Map([...(blocks.get(key) ?? []), ...tokens]));
  }
  return blocks;
}

// One level of `var(--x)` indirection, so a flat theme's `--grad-action:var(--rust)`
// is checked as the colour it actually paints rather than skipped as unparseable.
function deref(value, tokens) {
  const m = value.trim().match(/^var\(\s*(--[a-z0-9-]+)\s*\)$/i);
  return m && tokens.has(m[1]) ? tokens.get(m[1]).trim() : value;
}

// Every colour a fill paints: a gradient's stops, or the single colour of a
// flat fill. This is what a label has to stay readable against.
function fillColours(value, tokens) {
  const resolved = deref(value, tokens);
  const stops = [...resolved.matchAll(/#[0-9a-f]{3,6}|rgba?\([^)]+\)/gi)].map((m) => m[0]);
  return stops;
}

export function main(cssPath, themesPath) {
  const css = readFileSync(cssPath, "utf8");
  const blocks = parseThemeBlocks(css);
  const problems = [];

  const listed = [...readFileSync(themesPath, "utf8")
    .matchAll(/id:\s*"([a-z-]+)"/g)].map((m) => m[1]);
  for (const family of listed) {
    for (const mode of ["light", "dark"]) {
      if (!blocks.has(`${family}/${mode}`)) {
        problems.push(`themes.js lists "${family}" but app.css has no ${mode} block for it`);
      }
    }
  }
  for (const key of blocks.keys()) {
    const family = key.split("/")[0];
    if (!listed.includes(family)) {
      problems.push(`app.css defines "${key}" but themes.js does not list "${family}"`);
    }
  }

  for (const [key, tokens] of blocks) {
    for (const name of REQUIRED) {
      if (!tokens.has(name)) problems.push(`${key}: missing ${name}`);
    }
    const bgRaw = tokens.get("--bg");
    if (!bgRaw) continue;
    const bg = parseColor(bgRaw).slice(0, 3);
    // Where --bg-image is a gradient, measure against each of its stops too:
    // panels are translucent and the worst case may be any stop.
    const grounds = [bg, ...fillColours(tokens.get("--bg-image") ?? "", tokens)
      .map((s) => parseColor(s).slice(0, 3))];
    const panel = tokens.has("--panel") ? parseColor(tokens.get("--panel")) : null;

    for (const [token, { floor, on }] of Object.entries(FLOORS)) {
      if (!tokens.has(token)) continue;
      const fg = parseColor(tokens.get(token));
      let worst = Infinity;
      for (const ground of grounds) {
        // A translucent panel must be composited over the ground before it can
        // be measured, or every alpha theme reports a reassuring wrong number.
        if (panel) worst = Math.min(worst, contrast(fg, composite(panel, ground)));
        if (on === "both") worst = Math.min(worst, contrast(fg, ground));
      }
      if (worst < floor) {
        problems.push(`${key}: ${token} contrast ${worst.toFixed(2)} < ${floor} (on ${on})`);
      }
    }

    // Ink on a filled surface: the primary button puts its label straight onto
    // --accent, where a light-on-light pairing is easy to miss.
    for (const [inkToken, fillToken] of ON_PAIRS) {
      if (!tokens.has(inkToken) || !tokens.has(fillToken)) continue;
      const r = contrast(parseColor(tokens.get(inkToken)), parseColor(tokens.get(fillToken)));
      if (r < 4.5) {
        problems.push(`${key}: ${inkToken} on ${fillToken} is ${r.toFixed(2)} < 4.5`);
      }
    }

    // The Fit button's label sits on --grad-action, which is a ramp in a textured
    // theme and a single colour in a flat one. Every colour it paints is checked
    // at the 4.5 text floor, middle stops included — a gradient carries the label
    // across its whole length, not only at its ends.
    const grad = tokens.get("--grad-action");
    const on = tokens.get("--on-action");
    if (grad && on) {
      for (const stop of fillColours(grad, tokens)) {
        const r = contrast(parseColor(on), parseColor(stop));
        if (r < 4.5) {
          problems.push(`${key}: --on-action on gradient stop ${stop} is ${r.toFixed(2)} < 4.5`);
        }
      }
    }
  }
  return { problems };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const { problems } = main("docs/playground/app.css", "docs/playground/themes.js");
  if (problems.length) {
    console.error(`${problems.length} theme problem(s):`);
    for (const p of problems) console.error(" -", p);
    process.exit(1);
  }
  console.log("themes ok — contrast floors met, token contract complete, list agrees with CSS");
}
