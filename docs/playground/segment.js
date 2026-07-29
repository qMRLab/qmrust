// Brain extraction: a segmentation network over the volume on screen, turned into
// the mask the fit uses to skip background.
//
// The network and its settings are brainchop's (neuroneural/brainchop, MIT) — a
// MeshNet of dilated 3×3×3 convolutions, 21 KB of weights. It is fully
// convolutional, so it takes any grid, but it was trained on 1 mm isotropic
// volumes and its dilations reach 16 voxels; a 1.5 mm grid would put its
// receptive field over half again as much anatomy as it learned. So the input is
// conformed to brainchop's own 256³ 1 mm grid first, and the labels are sampled
// back onto the acquisition's grid afterwards.
//
// This module owns the imperative half — the runtime, the resampling, the tensor
// work and the button. The index arithmetic is `volume.js`, which is pure.
import { NVImage } from "./vendor/niivue.js";
import { icon } from "./vendor/icons.js";
import { $, hideProgress, setProgress, showProgress, status } from "./dom.js";
import { app, nvIn } from "./state.js";
import { rasToStorage } from "./nifti.js";
import {
  argmaxChannels,
  boundingBox,
  cropReversed,
  foregroundMask,
  normalizeToWhiteMatter,
  placeReversed,
} from "./volume.js";

// One entry per method, in the order the menu offers them. Each carries how it is
// run, so adding one is an entry here and nothing else — nothing downstream asks
// what kind of method it is.
//
// A method's `run` returns `{ mask, overlay }`: the mask on the acquisition's own
// grid in `fit_volume`'s layout — `(x*ny + y)*nz + z`, the same one `readMask`
// produces for a mask that came from the dataset — and an NVImage to draw over the
// image. It throws, with a sentence a reader can act on, when it finds nothing. The
// shared half owns the provenance, the overlay and the reporting.
//
// For the networks, `threshold`/`padding`/`classes` are upstream's own inference
// settings (`niivue/brainchop-models`, `<model>/settings.json`): they describe how
// those weights were trained to be fed, not something for us to tune. `threshold`
// and `padding` size the box handed to the network; `classes` is the final layer's
// channel count, and since the label sets name only background and brain, any
// non-zero class is brain.
const METHODS = [
  {
    id: "brain-extract-full",
    // What the menu says. The expected contrast is part of the name because it is
    // part of the contract: these weights were trained on T1-weighted anatomy, and
    // a network handed something else does not fail, it quietly finds less.
    menu: "Brain Segment T1w (brainchop)",
    icon: "brain",
    label: "brain",
    run: runNetwork,
    // 11 channels over the whole resampled field of view, which is the accuracy
    // and the memory both: ~800 MB of GPU, against the light model's ~400.
    threshold: 0,
    padding: 0,
    classes: 3,
  },
  {
    id: "brain-extract-light",
    // Kept as the one to fall back to. Nothing here can know how much memory a
    // reader's GPU has, and 5 channels over a tightly cropped box is the version
    // that still runs when the full one cannot.
    menu: "Brain Segment T1w, light (brainchop)",
    icon: "brain",
    label: "brain",
    run: runNetwork,
    threshold: 0.02,
    padding: 18,
    classes: 3,
  },
  {
    id: "otsu-largest",
    // No weights, no runtime, no GPU and no opinion about the contrast — so it is
    // the only entry that works on a single slice, which is what four of the five
    // example datasets are.
    menu: "Head Segment any contrast (Otsu)",
    icon: "fold-vertical",
    label: "head",
    run: runThreshold,
  },
];

// The grid brainchop's weights were trained on, and the axis order `conform` must
// put a volume into for them to mean anything (NiiVue's `permRAS`: which signed
// input axis each output axis came from).
const CONFORMED = [256, 256, 256];
const ORIENTATION = [-1, 3, -2];

// The runtime is 1.4 MB and most readers never press this button, so it is
// fetched on first use rather than at load. It is the published UMD bundle,
// vendored byte-for-byte and loaded as a classic script: the ESM build imports a
// dependency graph from a CDN, and this page vendors everything it runs.
let runtime = null;

async function loadRuntime() {
  if (runtime) return runtime;
  await new Promise((resolve, reject) => {
    const tag = document.createElement("script");
    tag.src = "./vendor/tf.js";
    tag.onload = resolve;
    tag.onerror = () => reject(new Error("the TensorFlow.js runtime could not be loaded"));
    document.head.append(tag);
  });
  const tf = globalThis.tf;
  if (!tf) throw new Error("the TensorFlow.js runtime loaded but published nothing");
  await tf.setBackend("webgl");
  await tf.ready();
  if (tf.getBackend() !== "webgl") {
    throw new Error("this browser exposes no WebGL backend, and the network needs a GPU");
  }
  runtime = tf;
  return tf;
}

// The frame on screen as a standalone 3D image. Which contrast to segment is a
// question about the acquisition rather than about the model, so it is whichever
// frame the reader is looking at — no model, suffix or role name appears here.
//
// The values are copied raw: `conform` applies `scl_slope`/`scl_inter` itself, and
// the header this clones already carries them.
function frameVolume() {
  const { volume, frame } = app.current;
  const [, dx, dy, dz] = volume.hdr.dims;
  const spatial = dx * dy * dz;
  const vol = NVImage.zerosLike(volume, "float32");
  vol.hdr.dims = volume.hdr.dims.slice();
  vol.hdr.dims[0] = 3;
  vol.hdr.dims[4] = 1;
  vol.nFrame4D = 1;
  vol.img = Float32Array.from(volume.img.subarray(frame * spatial, (frame + 1) * spatial));
  vol.calculateRAS();
  // The clone carried the whole series' intensity range. `conform` scales its
  // output against this window, so it has to describe the one frame now in it.
  vol.calMinMax();
  return vol;
}

// The composite map from a voxel of `from`'s grid to a voxel of `to`'s.
//
// Both grids are affine, so the composite is affine and four evaluations pin it
// down exactly: the origin, and one unit step along each axis. Deriving it this
// way keeps every matrix NiiVue's own — this holds no opinion about the header —
// and turns the per-voxel work into nine multiplies instead of two matrix
// inversions.
function gridToGrid(from, to) {
  const at = (vox) => to.mm2vox(Array.from(from.vox2mm(vox, from.matRAS)), true);
  const origin = at([0, 0, 0]);
  const axes = [at([1, 0, 0]), at([0, 1, 0]), at([0, 0, 1])].map((c) =>
    [0, 1, 2].map((d) => c[d] - origin[d]),
  );
  return (x, y, z) =>
    [0, 1, 2].map((d) =>
      Math.round(origin[d] + x * axes[0][d] + y * axes[1][d] + z * axes[2][d]),
    );
}

// Run the network's layers in sequence rather than through `predict`, so each one
// can report progress and release its input immediately: the activations are the
// memory cost here, not the 21 KB of weights.
async function inferLabels(tf, net, data, size) {
  const [sx, sy, sz] = size;
  // Axis-reversed, matching `cropReversed`.
  const shape = [1, sz, sy, sx, 1];
  net.layers[0].batchInputShape = [1, sz, sy, sx, 1];
  let tensor = tf.tensor(data, shape);
  for (let i = 1; i < net.layers.length; i++) {
    const next = tf.tidy(() => net.layers[i].apply(tensor));
    tensor.dispose();
    tensor = next;
    setProgress((i / net.layers.length) * 100);
    // One value read back forces the queued GPU work to finish, which is what
    // makes the progress above describe the network rather than the queue.
    const probe = tensor.slice([0, 0, 0, 0, 0], [1, 1, 1, 1, 1]);
    await probe.data();
    probe.dispose();
    await new Promise((resolve) => requestAnimationFrame(resolve));
  }
  const scores = await tensor.data();
  tensor.dispose();
  return scores;
}

// The mask, on the acquisition's own grid and in the C-order layout `fit_volume`
// takes — the same convention `readMask` produces for a mask that came from the
// dataset, so nothing downstream can tell the two apart.
function maskOnNativeGrid(labels, conformed, native, dims) {
  const [nx, ny, nz] = dims;
  const toStorage = rasToStorage(conformed, CONFORMED[0], CONFORMED[1]);
  const toConformed = gridToGrid(native, conformed);
  const mask = new Uint8Array(nx * ny * nz);
  let inside = 0;
  for (let x = 0; x < nx; x++) {
    for (let y = 0; y < ny; y++) {
      for (let z = 0; z < nz; z++) {
        const [cx, cy, cz] = toConformed(x, y, z);
        if (cx < 0 || cy < 0 || cz < 0) continue;
        if (cx >= CONFORMED[0] || cy >= CONFORMED[1] || cz >= CONFORMED[2]) continue;
        if (labels[toStorage(cx, cy, cz)] === 0) continue;
        mask[(x * ny + y) * nz + z] = 1;
        inside++;
      }
    }
  }
  return { mask, inside };
}

// The mask over the image it was derived from, as a second volume rather than as
// a drawing: a drawing is the reader's, and this is the machine's.
const OVERLAY_NAME = "computed mask";

// `labels` in `template`'s own storage order, as a displayable volume on
// `template`'s grid — which may be the conformed one or the acquisition's.
function labelVolume(template, labels) {
  const vol = NVImage.zerosLike(template, "uint8");
  vol.hdr.dims = template.hdr.dims.slice();
  vol.hdr.dims[0] = 3;
  vol.hdr.dims[4] = 1;
  vol.nFrame4D = 1;
  vol.img = Uint8Array.from(labels);
  vol.name = OVERLAY_NAME;
  vol.colorbarVisible = false;
  vol.calculateRAS();
  // A binary mask, so the window is 0..1 whatever class numbers it holds.
  vol.cal_min = 0;
  vol.cal_max = 1;
  vol.hdr.cal_min = 0;
  vol.hdr.cal_max = 1;
  vol.trustCalMinMax = true;
  return vol;
}

function showOverlay(vol) {
  hideOverlay();
  nvIn.addVolume(vol);
  nvIn.setColormap(vol.id, "red");
  // NiiVue's `red` already carries alpha — 0 at the bottom of the window, 0.5 at
  // the top — so background is transparent without a threshold of ours, and this
  // only tempers what is left.
  nvIn.setOpacity(nvIn.volumes.length - 1, 0.7);
  nvIn.drawScene();
}

function hideOverlay() {
  for (const v of [...nvIn.volumes]) {
    if (v.name === OVERLAY_NAME) nvIn.removeVolume(v);
  }
}

// The mask belongs to the grid it was computed on, so a model switch drops it the
// same way a drawing is dropped. `reresolveInputs` reads `app.computedMask` on every
// fit, so clearing it here is what hands the recipe's own mask back.
export function clearComputedMask() {
  app.computedMask = null;
  hideOverlay();
  if (app.current) app.current.maskU8 = app.current.resolvedMask ?? null;
  syncButton();
}

function syncButton() {
  const btn = $("segment");
  if (!btn) return;
  btn.innerHTML = `${icon("sparkles", 13)}<span>Segment</span>`;
  btn.classList.toggle("active", Boolean(app.computedMask));
  btn.disabled = !app.current;
  btn.title = app.computedMask
    ? "A computed mask is in use — choose another network, or clear it"
    : "Find a structure in the volume on screen with a segmentation network";
}

// The menu is built from the registry rather than written out, so adding a network
// is one entry in `NETWORKS` and nothing here.
function buildMenu() {
  const box = $("segment-menu");
  box.replaceChildren();
  for (const method of METHODS) {
    const item = document.createElement("button");
    item.type = "button";
    item.className = "menu-item";
    item.setAttribute("role", "menuitem");
    item.innerHTML = `${icon(method.icon, 15)}<span>${method.menu}</span>`;
    item.onclick = () => {
      closeMenu();
      choose(method);
    };
    box.append(item);
  }
  if (app.computedMask) {
    const clear = document.createElement("button");
    clear.type = "button";
    clear.className = "menu-item";
    clear.setAttribute("role", "menuitem");
    clear.innerHTML = `${icon("eraser", 15)}<span>Clear the mask</span>`;
    clear.onclick = () => {
      closeMenu();
      clearAiMask();
      status("Mask cleared — the next fit uses the recipe's own", "ok");
    };
    box.append(clear);
  }
}

// Anchored above the button, not below it: the button sits at the bottom of its
// card, and a menu opening downwards would hang off the page.
function openMenu() {
  const btn = $("segment");
  const box = $("segment-menu");
  buildMenu();
  box.hidden = false;
  const anchor = btn.getBoundingClientRect();
  const width = box.offsetWidth;
  box.style.left = `${Math.max(6, Math.min(anchor.right - width, window.innerWidth - width - 6))}px`;
  box.style.top = `${anchor.top - box.offsetHeight - 6}px`;
  btn.setAttribute("aria-expanded", "true");
}

function closeMenu() {
  $("segment-menu").hidden = true;
  $("segment").setAttribute("aria-expanded", "false");
}

function toggleMenu() {
  if ($("segment-menu").hidden) openMenu();
  else closeMenu();
}

let running = false;

async function runNetwork(network) {
  status(`Loading the ${network.label} network…`, "busy");
  const tf = await loadRuntime();

  status(`Conforming the volume to ${CONFORMED.join("×")} at 1 mm…`, "busy");
  // `rawFloat32` (the last argument): resampled values are kept as they are rather
  // than mapped through this volume's display window into 8 bits.
  // `normalizeToWhiteMatter` sets the intensity scale the network needs, and it
  // needs real values to find the tissue mode in.
  const conformed = await nvIn.conform(
    frameVolume(), false, true, false, false, CONFORMED, 1, true,
  );
  // The network was trained on volumes in the orientation `conform` produces, and
  // nothing downstream can tell a wrongly-oriented volume from a correctly oriented
  // one — it just returns a worse mask. So the orientation is checked rather than
  // assumed: upstream asserts this exact axis order before feeding its own network
  // (brainchop's `ensureConformed`).
  if (String(conformed.permRAS) !== String(ORIENTATION)) {
    throw new Error(
      `conforming gave axes [${conformed.permRAS}] where the network expects ` +
        `[${ORIENTATION}] — the mask would not mean anything`,
    );
  }

  const values = normalizeToWhiteMatter(conformed.img);
  const { lo, size } = boundingBox(values, CONFORMED, network.threshold, network.padding);
  const cropped = cropReversed(values, CONFORMED, lo, size);

  status(`Finding the brain (${size.join("×")})…`, "busy");
  const net = await tf.loadLayersModel(`./models/${network.id}/model.json`);
  const scores = await inferLabels(tf, net, cropped, size);
  net.dispose();
  tf.engine().disposeVariables();

  const labels = placeReversed(argmaxChannels(scores, network.classes), CONFORMED, lo, size);
  const found = labels.reduce((n, v) => n + (v > 0 ? 1 : 0), 0);
  const { mask, inside } = maskOnNativeGrid(
    labels, conformed, app.current.volume, app.current.meta.dims,
  );
  // These two failures look identical from the outside and have nothing to do with
  // each other: one is the network declining, the other is our own coordinate round
  // trip missing. Reporting them apart is what makes the next one diagnosable.
  if (found === 0) {
    const frame = app.current.meta.labels[app.current.frame] ?? "this volume";
    throw new Error(`the network found no brain in ${frame} — try a frame with more `
      + "anatomical contrast");
  }
  if (inside === 0) {
    throw new Error(`the network found ${found.toLocaleString()} brain voxels, but none `
      + "of them landed on this volume's grid — the coordinate mapping is wrong");
  }
  return { mask, overlay: labelVolume(conformed, labels) };
}

// The classical route. It needs no resampling — the mask it finds is already on the
// grid the fit runs on — and no runtime, no weights and no GPU.
//
// It averages *every* acquired volume rather than reading the one on screen. That is
// not a shortcut: a single frame can be a poor place to look for the subject (an
// inversion-recovery frame near its null has almost no signal anywhere), while the
// subject is present in all of them, so the mean has the best contrast available and
// the answer does not depend on where the reader left the slider.
async function runThreshold(method) {
  const [nx, ny, nz, nt] = app.current.meta.dims;
  const { volume } = app.current;
  status(`Thresholding ${nx}×${ny}${nz > 1 ? `×${nz}` : ""} over ${nt} volumes…`, "busy");
  // `volume.js` indexes storage-style throughout, so the read is built that way and
  // re-indexed into the fit's layout once, at the end.
  const values = new Float32Array(nx * ny * nz);
  for (let x = 0; x < nx; x++) {
    for (let y = 0; y < ny; y++) {
      for (let z = 0; z < nz; z++) {
        let sum = 0;
        for (let t = 0; t < nt; t++) sum += volume.getValue(x, y, z, t);
        values[x + y * nx + z * nx * ny] = sum / nt;
      }
    }
  }
  // A frame's worth of work, so let the progress bar and the status line paint.
  setProgress(50);
  await new Promise((resolve) => requestAnimationFrame(resolve));

  const found = foregroundMask(values, [nx, ny, nz]);
  const mask = new Uint8Array(nx * ny * nz);
  const overlay = new Uint8Array(nx * ny * nz);
  const at = rasToStorage(volume, nx, ny);
  let inside = 0;
  for (let x = 0; x < nx; x++) {
    for (let y = 0; y < ny; y++) {
      for (let z = 0; z < nz; z++) {
        if (!found[x + y * nx + z * nx * ny]) continue;
        mask[(x * ny + y) * nz + z] = 1;
        overlay[at(x, y, z)] = 1;
        inside++;
      }
    }
  }
  if (inside === 0) {
    throw new Error("thresholding found nothing above background in this dataset");
  }
  return { mask, overlay: labelVolume(volume, overlay) };
}

// The shared half: the flags, the progress bar, the provenance and the reporting.
// A method only has to produce a mask and something to show for it.
async function choose(method) {
  if (running || !app.current) return;
  running = true;
  $("segment").disabled = true;
  showProgress();
  const started = performance.now();
  try {
    const { mask, overlay } = await method.run(method);
    // The recipe's own mask is kept, so clearing this one restores it without
    // having to resolve the dataset again.
    app.current.resolvedMask ??= app.current.maskU8;
    app.computedMask = mask;
    app.current.maskU8 = mask;
    showOverlay(overlay);
    const inside = mask.reduce((n, v) => n + v, 0);
    const share = ((inside / mask.length) * 100).toFixed(1);
    status(
      `${method.label} mask: ${inside.toLocaleString()} voxels (${share}% of the volume) ` +
        `in ${((performance.now() - started) / 1000).toFixed(1)} s — fit to use it`,
      "ok",
    );
  } catch (e) {
    status(`Could not segment — ${e?.message ?? e}`, "error");
  } finally {
    running = false;
    hideProgress();
    syncButton();
  }
}

export function wireSegment() {
  $("segment").onclick = toggleMenu;
  // A menu that cannot be dismissed is a trap, and both of these are what a reader
  // reaches for first.
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape") closeMenu();
  });
  document.addEventListener("pointerdown", (e) => {
    if ($("segment-menu").hidden) return;
    if (e.target.closest("#segment-menu, #segment")) return;
    closeMenu();
  });
  syncButton();
}

// Called whenever a model finishes loading, so the button reflects the new volume:
// enabled, and with no mask of its own yet.
export { syncButton as syncSegmentButton };
