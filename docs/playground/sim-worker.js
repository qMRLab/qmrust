// A simulation runs the same wasm call the CLI runs, and a sweep of a few
// thousand fits takes tens of seconds. On the main thread that is a frozen tab,
// and the call cannot be split: the RNG stream is consumed sequentially across
// trials and sweep points, so per-chunk calls with the same seed would reuse
// the same noise and stop matching the CLI. So it runs here instead, whole,
// with its own wasm instance.
let wasm = null;

async function ready() {
  if (!wasm) {
    const mod = await import("./pkg/qmrust_wasm.js");
    await mod.default();
    wasm = mod;
  }
  return wasm;
}

self.onmessage = async (event) => {
  const { id, mode, yaml } = event.data;
  try {
    const mod = await ready();
    self.postMessage({ id, ok: true, report: JSON.parse(mod.sim(mode, yaml)) });
  } catch (e) {
    self.postMessage({ id, ok: false, error: String(e?.message ?? e) });
  }
};
