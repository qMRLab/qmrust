// Icons, from Lucide (https://lucide.dev) v1.27.0 — ISC License,
// Copyright (c) 2026 Lucide Icons and Contributors.
//
// A curated subset as data rather than the whole pack or an SVG sprite:
//
//   * only the icons actually used are here, so the cost is bytes-per-icon
//     rather than a megabyte of pack;
//   * as data, one mechanism serves both static markup (`paintIcons`) and
//     buttons built at runtime (`icon`), which is what the drawing palette
//     will need;
//   * an external `<use href="sprite.svg#id">` would cost a fetch and has
//     long-standing Safari bugs.
//
// Every glyph strokes in `currentColor`, so an icon takes the colour of
// whatever theme token its context sets. To add one, copy the shape elements
// out of the upstream SVG — see docs/README.md for the recipe.

const SHAPES = {
  "circle":
    '<circle cx="12" cy="12" r="10" />',
  "cloud-off":
    '<path d="M10.94 5.274A7 7 0 0 1 15.71 10h1.79a4.5 4.5 0 0 1 4.222 6.057" /><path d="M18.796 18.81A4.5 4.5 0 0 1 17.5 19H9A7 7 0 0 1 5.79 5.78" /><path d="m2 2 20 20" />',
  "eraser":
    '<path d="M21 21H8a2 2 0 0 1-1.42-.587l-3.994-3.999a2 2 0 0 1 0-2.828l10-10a2 2 0 0 1 2.829 0l5.999 6a2 2 0 0 1 0 2.828L12.834 21" /><path d="m5.082 11.09 8.828 8.828" />',
  "moon":
    '<path d="M20.985 12.486a9 9 0 1 1-9.473-9.472c.405-.022.617.46.402.803a6 6 0 0 0 8.268 8.268c.344-.215.825-.004.803.401" />',
  "paint-bucket":
    '<path d="M11 7 6 2" /><path d="M18.992 12H2.041" /><path d="M21.145 18.38A3.34 3.34 0 0 1 20 16.5a3.3 3.3 0 0 1-1.145 1.88c-.575.46-.855 1.02-.855 1.595A2 2 0 0 0 20 22a2 2 0 0 0 2-2.025c0-.58-.285-1.13-.855-1.595" /><path d="m8.5 4.5 2.148-2.148a1.205 1.205 0 0 1 1.704 0l7.296 7.296a1.205 1.205 0 0 1 0 1.704l-7.592 7.592a3.615 3.615 0 0 1-5.112 0l-3.888-3.888a3.615 3.615 0 0 1 0-5.112L5.67 7.33" />',
  "palette":
    '<path d="M12 22a1 1 0 0 1 0-20 10 9 0 0 1 10 9 5 5 0 0 1-5 5h-2.25a1.75 1.75 0 0 0-1.4 2.8l.3.4a1.75 1.75 0 0 1-1.4 2.8z" /><circle cx="13.5" cy="6.5" r=".5" fill="currentColor" /><circle cx="17.5" cy="10.5" r=".5" fill="currentColor" /><circle cx="6.5" cy="12.5" r=".5" fill="currentColor" /><circle cx="8.5" cy="7.5" r=".5" fill="currentColor" />',
  "pencil":
    '<path d="M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z" /><path d="m15 5 4 4" />',
  "scan-box":
    '<path d="M12 12v5.5" /><path d="M17 3h2a2 2 0 012 2v2" /><path d="M21 17v2a2 2 0 01-2 2h-2" /><path d="M3 7V5a2 2 0 012-2h2" /><path d="M7 21H5a2 2 0 01-2-2v-2" /><path d="M7.264 9.252 12 12l4.737-2.748" /><path d="M7.995 8.514A2 2 0 007 10.244v3.516a2 2 0 00.996 1.73l3 1.74a2 2 0 002.008 0l3-1.74A2 2 0 0017 13.76v-3.517a2 2 0 00-.995-1.73l-3-1.742a2 2 0 00-1.892-.064z" />',
  "square":
    '<rect width="18" height="18" x="3" y="3" rx="2" />',
  "sun":
    '<circle cx="12" cy="12" r="4" /><path d="M12 2v2" /><path d="M12 20v2" /><path d="m4.93 4.93 1.41 1.41" /><path d="m17.66 17.66 1.41 1.41" /><path d="M2 12h2" /><path d="M20 12h2" /><path d="m6.34 17.66-1.41 1.41" /><path d="m19.07 4.93-1.41 1.41" />',
  "trash-2":
    '<path d="M10 11v6" /><path d="M14 11v6" /><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6" /><path d="M3 6h18" /><path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />',
  "undo-2":
    '<path d="M9 14 4 9l5-5" /><path d="M4 9h10.5a5.5 5.5 0 0 1 5.5 5.5a5.5 5.5 0 0 1-5.5 5.5H11" />',
};

// Lucide draws for 24px at stroke-width 2; below ~18px that reads heavy, so the
// stroke thins as the icon shrinks.
function strokeFor(size) {
  return size >= 20 ? 2 : size >= 16 ? 1.75 : 1.5;
}

// One icon as markup. `size` is in CSS pixels.
export function icon(name, size = 16) {
  const shapes = SHAPES[name];
  if (!shapes) throw new Error(`no such icon: ${name}`);
  return (
    `<svg class="icon" viewBox="0 0 24 24" width="${size}" height="${size}" ` +
    `fill="none" stroke="currentColor" stroke-width="${strokeFor(size)}" ` +
    `stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${shapes}</svg>`
  );
}

// Fill every `[data-icon]` placeholder under `root`, so static markup names the
// icon where it sits instead of the wiring having to know which elements have
// one. `data-icon-size` overrides the default.
export function paintIcons(root = document) {
  for (const el of root.querySelectorAll("[data-icon]")) {
    el.innerHTML = icon(el.dataset.icon, Number(el.dataset.iconSize) || 16);
  }
}
