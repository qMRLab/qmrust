#!/usr/bin/env node
// Asserts that every playground theme keeps its text readable, and that the
// theme list and the CSS agree. Run by the docs workflow; also runnable by hand:
//   node scripts/check_theme_contrast.mjs
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

// Floors per token. `--muted` is deliberately 3.0 rather than AA's 4.5: a
// 4.5-contrast "muted" grey no longer reads as muted, and the shipped clinical
// light theme has sat at 3.64 since it was written.
export const FLOORS = {
  "--ink": 4.5, "--ink-2": 4.5, "--muted": 3.0,
  "--accent": 3.0, "--rust": 3.0, "--brass": 3.0,
};

// Every token a theme must define.
export const REQUIRED = [
  "--bg", "--panel", "--panel-2", "--field", "--ink", "--ink-2", "--muted",
  "--line", "--rust", "--accent", "--accent-ink", "--brass", "--good", "--bad",
  "--viewer-bg", "--radius", "--border-w", "--shadow",
  "--font-ui", "--font-h", "--font-mono", "--track", "--pad-y", "--pad-x",
];

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

// Gradient endpoints, for checking button label contrast at both ends.
function gradientStops(value) {
  return [...value.matchAll(/#[0-9a-f]{3,6}|rgba?\([^)]+\)/gi)].map((m) => m[0]);
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
    const grounds = [bg, ...gradientStops(tokens.get("--bg-image") ?? "")
      .map((s) => parseColor(s).slice(0, 3))];
    const panel = tokens.has("--panel") ? parseColor(tokens.get("--panel")) : null;

    for (const [token, floor] of Object.entries(FLOORS)) {
      if (!tokens.has(token)) continue;
      const fg = parseColor(tokens.get(token));
      let worst = Infinity;
      for (const ground of grounds) {
        const surfaces = [ground];
        if (panel) surfaces.push(composite(panel, ground));
        for (const surface of surfaces) worst = Math.min(worst, contrast(fg, surface));
      }
      if (worst < floor) {
        problems.push(`${key}: ${token} contrast ${worst.toFixed(2)} < ${floor}`);
      }
    }

    // Button labels sit on a gradient; check both ends at the 4.5 text floor.
    const grad = tokens.get("--grad-action");
    const on = tokens.get("--on-action");
    if (grad && on) {
      for (const stop of gradientStops(grad)) {
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
