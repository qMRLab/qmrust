// The boundary between NiiVue's images and `fit_volume`'s arrays, both ways:
// reading a loaded NVImage into the C-order layout the wasm API expects, and
// building a displayable NVImage back out of a fitted map. The only array
// handling this app owns.
import { NVImage } from "./vendor/niivue.js";
import { gunzipSync } from "./vendor/fflate.js";
import { roundBound } from "./dom.js";
import { percentileWindow } from "./stats.js";

// Every element of `flat` (C-order `[nx,ny,nz]`, i.e. index `(x*ny+y)*nz+z`,
// matching fit_volume's own output convention) read from `volume` via its
// public accessor, so slope/intercept and orientation are handled exactly as
// NiiVue would for display — no separate raw-buffer indexing to get wrong.
// These bundles carry no qform/sform (a single downsampled slice has no
// meaningful scanner orientation), so NiiVue's own axis order is already
// native `dim[1]`/`dim[2]` order — the same order `fit_volume`, the output
// maps, and the `probes` oracle all use.
export function readVolumeSeries(volume, nx, ny, nz, nt) {
  const flat = new Float64Array(nx * ny * nz * nt);
  for (let x = 0; x < nx; x++) {
    for (let y = 0; y < ny; y++) {
      for (let z = 0; z < nz; z++) {
        for (let t = 0; t < nt; t++) {
          flat[((x * ny + y) * nz + z) * nt + t] = volume.getValue(x, y, z, t);
        }
      }
    }
  }
  return flat;
}

// C-order `[nx,ny,nz]`, single frame — the same convention `readVolumeSeries`
// uses per timepoint, and the one `fit_volume`'s `mask`/`aux` arguments want. A
// one-frame series is exactly that layout, so this is the same read.
export function readScalarVolume(volume, nx, ny, nz) {
  return readVolumeSeries(volume, nx, ny, nz, 1);
}

export function readMask(volume, nx, ny, nz) {
  const raw = readScalarVolume(volume, nx, ny, nz);
  return Uint8Array.from(raw, (v) => (v > 0 ? 1 : 0));
}

// Where RAS voxel `(x, y, z)` lands in `vol`'s own storage buffer.
//
// Every coordinate this app handles is a RAS voxel index: `NVImage.getValue`
// takes RAS and converts, NiiVue's crosshair reports RAS, and its drawing bitmap
// is RAS (`back.dims` is `dimsRAS`). A volume's *buffer*, though, is in the order
// the file stored it, which for any dataset whose affine is not already
// RAS-aligned is a flipped and/or axis-permuted view of that — NiiVue reorders it
// with `img2RAS` when it uploads the texture.
//
// `img2RASstep`/`img2RASstart` are the mapping NiiVue derives from the affine
// itself, so this reuses its arithmetic rather than forming a second opinion on
// the header. For an already-RAS volume they are `[1, nx, nx*ny]` and `[0, 0, 0]`,
// making this the plain `x + y*nx + z*nx*ny`.
export function rasToStorage(vol, nx, ny) {
  const step = vol.img2RASstep;
  const start = vol.img2RASstart;
  if (!step || !start) return (x, y, z) => x + y * nx + z * nx * ny;
  const origin = start[0] + start[1] + start[2];
  return (x, y, z) => origin + x * step[0] + y * step[1] + z * step[2];
}

// Build a displayable NVImage for one output map from `flat` (C-order
// `[nx,ny,nz]`, NaN outside the fit), reusing `template`'s header/orientation
// (NVImage.zerosLike) rather than hand-rolling a NIfTI header.
export function buildMapVolume(template, flat, nx, ny, nz, name, unit, displayRange) {
  const vol = NVImage.zerosLike(template, "float32");
  vol.hdr.dims = template.hdr.dims.slice();
  vol.hdr.dims[0] = 3;
  vol.hdr.dims[4] = 1;
  vol.nFrame4D = 1;
  // The model's declared window for this quantity when it has one: a window
  // derived from the data is only as good as the data, and an unmasked fit puts
  // background noise in the same histogram as tissue. Falls back to a
  // percentile window for outputs whose scale genuinely belongs to the data
  // (an arbitrary-unit amplitude, say).
  const [lo, hi] = displayRange ?? percentileWindow(flat);
  // Unfitted voxels (NaN — outside the mask, or a failed fit) must not read
  // as the *brightest* colour in the map, which is what an unclamped NaN
  // texel does here. Park them well below the display window so they clamp
  // to the colormap's dark end instead — the GPU equivalent of
  // `docsfig.style`'s `cm.set_bad(BG)`.
  const sentinel = lo - Math.max(1, hi - lo);
  const img = new Float32Array(nx * ny * nz);
  // `flat` is indexed by the RAS voxel the fit ran on; `img` is the template's
  // storage order. Writing one as if it were the other mirrors the map against
  // the image it was fitted from.
  const at = rasToStorage(template, nx, ny);
  for (let x = 0; x < nx; x++) {
    for (let y = 0; y < ny; y++) {
      for (let z = 0; z < nz; z++) {
        const v = flat[(x * ny + y) * nz + z];
        img[at(x, y, z)] = Number.isFinite(v) ? v : sentinel;
      }
    }
  }
  vol.img = img;
  vol.name = `${name}${unit ? ` [${unit}]` : ""}`;
  // Trust the percentile window computed above rather than have NiiVue
  // recompute its own min/max from the raw buffer — which would include the
  // dark-end sentinel just written in for NaN and pull the window back out
  // to it.
  vol.trustCalMinMax = true;
  // The colorbar is a custom HTML/CSS element beside the viewer (see the level
  // control), not NiiVue's own on-canvas one — this vendored build only draws
  // that horizontally, along the canvas's bottom edge, with no vertical/
  // right-side option.
  vol.colorbarVisible = false;
  vol.calculateRAS();
  // `NVImage.zerosLike` clones `template` (the acquired image) before
  // zeroing it, and `clone()` runs the library's own `calMinMax()` over the
  // *template's* data as a side effect — so `hdr.cal_min`/`hdr.cal_max` start
  // out describing the wrong image entirely. Set both the plain property and
  // its header twin to our percentile window, so whichever one a later
  // render/add-volume step reads back gets the fitted map's own range.
  vol.cal_min = roundBound(lo);
  vol.cal_max = roundBound(hi);
  vol.hdr.cal_min = vol.cal_min;
  vol.hdr.cal_max = vol.cal_max;
  return vol;
}

// In BIDS one file is one 3D volume, while the viewer, the frame slider and
// `readVolumeSeries` all expect a single 4D image.
//
// Rather than assemble an NVImage by hand — which means setting NiiVue's
// internal 4D bookkeeping correctly and keeping it correct — concatenate the
// volumes into a real 4D NIfTI and let NiiVue parse it, the same path the
// pre-baked 4D bundles take. Reading the few header fields needed to do that is
// the only NIfTI field access this app performs; decoding is still NiiVue's.
//
// `parts` are the per-volume file bytes in the order resolution put them, which
// is the order the model consumes them.
export function buildSeriesNifti(parts) {
  const raw = parts.map((b) => (b[0] === 0x1f && b[1] === 0x8b ? gunzipSync(b) : b));
  const head = new DataView(raw[0].buffer, raw[0].byteOffset, raw[0].byteLength);
  // dim[0] (the dimensionality) is 1..7 in the file's own byte order; anything
  // else means the header is big-endian, which nothing this project writes.
  const ndim = head.getInt16(40, true);
  if (ndim < 1 || ndim > 7) {
    throw new Error("unsupported NIfTI byte order (big-endian header)");
  }
  const voxOffset = head.getFloat32(108, true);
  const bitpix = head.getInt16(72, true);
  const [nx, ny, nz] = [1, 2, 3].map((i) => head.getInt16(40 + i * 2, true));
  const frameBytes = nx * ny * nz * (bitpix / 8);
  const nt = raw.length;

  const header = raw[0].slice(0, voxOffset);
  const hv = new DataView(header.buffer, header.byteOffset, header.byteLength);
  hv.setInt16(40, 4, true); // dim[0]: now four-dimensional
  hv.setInt16(48, nt, true); // dim[4]: this many volumes

  const out = new Uint8Array(voxOffset + frameBytes * nt);
  out.set(header, 0);
  for (let t = 0; t < nt; t++) {
    const data = raw[t].subarray(voxOffset, voxOffset + frameBytes);
    if (data.length !== frameBytes) {
      throw new Error(
        `volume ${t + 1} of ${nt} holds ${data.length} data bytes, expected ${frameBytes} ` +
          "(the volumes of one collection must share a shape and datatype)",
      );
    }
    out.set(data, voxOffset + frameBytes * t);
  }
  return { bytes: out, nx, ny, nz, nt };
}

// Load one file's bytes out of the extracted archive as an NVImage. NiiVue
// infers the format from the name, so the basename must survive.
export async function volumeFromDataset(files, path, extra = {}) {
  const bytes = files.get(path);
  if (!bytes) throw new Error(`resolution named "${path}", which the archive does not hold`);
  const name = path.split("/").pop();
  return NVImage.loadFromFile({ file: new File([bytes], name), name, ...extra });
}
