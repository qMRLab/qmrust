// Page-level helpers every region shares: element lookup, the navbar status
// line, display formatting, and reading a CSS custom property as a colour.
// Depends on nothing else in the app, so any module may import it.

export const $ = (id) => document.getElementById(id);

// The navbar status line. `kind` colours and weights it, because the two states
// a reader actually watches for — work in progress, and done — should be
// distinguishable at a glance rather than uniform grey.
//   "ok"    finished and usable      (green)
//   "busy"  something is happening   (rust)
//   "error" it did not work          (red)
//   "info"  neutral aside            (muted)
export const status = (m, kind = "info") => {
  const el = $("status");
  el.textContent = m;
  el.classList.remove("ok", "busy", "error");
  if (kind !== "info") el.classList.add(kind);
};

// Compact number for a label: drops trailing zeros so 0.35 reads as 0.35 and
// 90.0 as 90.
const fmt = (v) => (typeof v === "number" ? String(Number(v.toPrecision(6))) : String(v));

// A volume's label, from whatever identity it resolved to — a role name for a
// named measurement, its parameter values for a series one. Never keyed on a
// model or parameter name.
export function identityLabel(id) {
  if (!id) return "volume";
  if (id.role) return id.role;
  const parts = Object.entries(id.params ?? {}).map(([k, v]) => `${k}=${fmt(v)}`);
  return parts.length ? parts.join(", ") : "volume";
}

// Rounds a colour-scale bound to ~3 significant figures for display in the
// min/max inputs — full float precision is not a meaningful "default", and
// doesn't fit the input box either.
export function roundBound(v) {
  if (!Number.isFinite(v) || v === 0) return 0;
  const digits = Math.max(0, 2 - Math.floor(Math.log10(Math.abs(v))));
  return Number(v.toFixed(Math.min(6, digits)));
}

// A CSS custom property as NiiVue's 0..1 RGBA. Reading the token rather than
// hardcoding a hex keeps the viewers on the same palette as the charts, and
// following light/dark with everything else.
export function cssColorToRgba(token, alpha = 1) {
  const raw = getComputedStyle(document.documentElement).getPropertyValue(token).trim();
  // A CSS-resolved colour comes back as `rgb(...)`/`rgba(...)`; a raw hex token
  // does not, so handle both.
  const hex = raw.match(/^#([0-9a-f]{6})$/i);
  if (hex) {
    const n = parseInt(hex[1], 16);
    return [((n >> 16) & 255) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255, alpha];
  }
  const nums = raw.match(/[\d.]+/g);
  if (!nums || nums.length < 3) return [1, 0, 0, alpha];
  return [nums[0] / 255, nums[1] / 255, nums[2] / 255, alpha];
}

// The fitting progress track: a permanent fixture above the Fit button (an empty
// track at idle, not an element that appears and reflows the page); only its fill
// width and the "active" stripe animation change. Anything that takes long enough
// to need a bar drives this one — there is only ever one long computation running.
export function showProgress() {
  $("progress-bar").classList.add("active");
  setProgress(0);
}

export function setProgress(pct) {
  $("progress-bar").style.width = `${Math.max(0, Math.min(100, pct))}%`;
}

export function hideProgress() {
  $("progress-bar").classList.remove("active");
  setProgress(0);
}

// Lets the browser paint, and stay responsive to input, between chunks of a long
// computation — a plain microtask or `setTimeout(0)` does not reliably yield before
// the next paint in every engine, but a rAF round-trip does.
export function nextFrame() {
  return new Promise((resolve) => requestAnimationFrame(() => setTimeout(resolve, 0)));
}
