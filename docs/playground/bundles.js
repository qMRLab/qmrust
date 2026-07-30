// The wasm module and the per-model payload JSON, each loaded once.
import { $, status } from "./dom.js";

// Per-model payload JSON (`docs/playground/data/<model>.json`), fetched once.
const bundleCache = {};

export async function loadWasm() {
  try {
    const mod = await import("./pkg/qmrust_wasm.js");
    await mod.default();
    return mod;
  } catch (e) {
    $("fallback").hidden = false;
    status("Fitting unavailable — wasm failed to load; viewers still work", "error");
    return null;
  }
}

export async function fetchOrThrow(url) {
  const r = await fetch(url);
  if (!r.ok) throw new Error(`${url}: ${r.status}`);
  return r;
}

export async function loadBundle(name) {
  if (bundleCache[name]) return bundleCache[name];
  const meta = await (await fetchOrThrow(`./data/${name}.json`)).json();
  bundleCache[name] = meta;
  return meta;
}
