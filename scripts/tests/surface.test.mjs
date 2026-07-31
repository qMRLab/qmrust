// The recipe form shows what a model actually accepts, not what the recipe
// happens to mention. That merge is the whole contract: a default the reader
// never typed must still appear (and be marked as a default), and a key the
// sidecars supply must not be presented as editable.
import test from "node:test";
import assert from "node:assert/strict";
import { mergeSurface, withProtocolComments, stripProtocolComments, readNumbers } from "../../docs/playground/surface.js";

const flat = (rows) => Object.fromEntries(rows.map((r) => [r.path.join("."), r]));

test("a default the recipe never states still appears, marked unset", () => {
  const rows = flat(mergeSurface(
    { flip_angles: [3, 20], repetition_time: 0.015, fit_type: "linear" },
    { flip_angles: [3, 20], repetition_time: 0.015 },
    { protocolKeys: [], readOnly: false },
  ));
  assert.equal(rows.fit_type.value, "linear");
  assert.equal(rows.fit_type.isSet, false);
  assert.equal(rows.flip_angles.isSet, true);
});

test("an override wins over the surface default", () => {
  const rows = flat(mergeSurface(
    { fit_type: "linear" },
    { fit_type: "nonlinear" },
    { protocolKeys: [], readOnly: false },
  ));
  assert.equal(rows.fit_type.value, "nonlinear");
  assert.equal(rows.fit_type.isSet, true);
});

test("protocol keys are read-only in BIDS mode and editable otherwise", () => {
  const surface = { flip_angles: [], fit_type: "linear" };
  const opts = { protocolKeys: ["flip_angles"] };
  const bids = flat(mergeSurface(surface, {}, { ...opts, readOnly: true }));
  assert.equal(bids.flip_angles.isProtocol, true);
  assert.equal(bids.flip_angles.readOnly, true);
  assert.equal(bids.fit_type.readOnly, false, "only protocol keys are locked");

  const nonBids = flat(mergeSurface(surface, {}, { ...opts, readOnly: false }));
  assert.equal(nonBids.flip_angles.readOnly, false);
});

test("nested configs keep dotted paths so enum lookup still matches", () => {
  const rows = mergeSurface(
    { qmt_spgr: { lineshape: "gaussian", fit: { r1f: 1.0 } } },
    { qmt_spgr: { lineshape: "superlorentzian" } },
    { protocolKeys: [], readOnly: false },
  );
  const group = rows.find((r) => r.key === "qmt_spgr");
  assert.ok(group.children, "a nested object yields children");
  const inner = flat(group.children);
  assert.equal(inner["qmt_spgr.lineshape"].value, "superlorentzian");
  assert.equal(inner["qmt_spgr.lineshape"].isSet, true);
  assert.equal(inner["qmt_spgr.fit"].children.length, 1);
  assert.equal(inner["qmt_spgr.fit"].children[0].path.join("."), "qmt_spgr.fit.r1f");
});

test("a nested protocol path locks, matched on its full dotted path", () => {
  // qmt_spgr's acquisition is `protocol.mtdata` under the `qmt_spgr:` subkey,
  // so matching a top-level key alone would never find it and the panel would
  // offer an editable table that ingest_protocol overwrites.
  const rows = mergeSurface(
    { qmt_spgr: { protocol: { mtdata: [[142, 443]] }, lineshape: "gaussian" } },
    {},
    { protocolKeys: ["qmt_spgr.protocol.mtdata"], readOnly: true },
  );
  const inner = flat(rows.find((r) => r.key === "qmt_spgr").children);
  const mtdata = flat(inner["qmt_spgr.protocol"].children)["qmt_spgr.protocol.mtdata"];
  assert.equal(mtdata.isProtocol, true);
  assert.equal(mtdata.readOnly, true);
  assert.equal(inner["qmt_spgr.lineshape"].readOnly, false, "only the acquisition locks");
});

test("a locked row shows the value the surface carries (ingested from the sidecars)", () => {
  // `effective_config` runs `ingest_protocol` before serializing, so the
  // surface itself already carries the resolved value — the merge must show
  // it, not blank it, on a locked row.
  const rows = mergeSurface(
    { mtw: { flip_angle: 6.0 } },
    {},
    { protocolKeys: ["mtw.flip_angle"], readOnly: true },
  );
  const leaves = flat(rows.find((r) => r.key === "mtw").children);
  assert.equal(leaves["mtw.flip_angle"].readOnly, true);
  assert.equal(leaves["mtw.flip_angle"].value, 6.0);
});

test("locking an object locks everything under it", () => {
  // mt_sat's ingest_protocol replaces whole weighting objects, so naming the
  // object must lock its leaves — otherwise flip_angle stays editable and is
  // silently discarded.
  const rows = mergeSurface(
    { mtw: { flip_angle: 6, repetition_time: 0.028 }, export_mtr: true },
    {},
    { protocolKeys: ["mtw"], readOnly: true },
  );
  const top = flat(rows);
  assert.equal(top.mtw.readOnly, true);
  const leaves = flat(top.mtw.children);
  assert.equal(leaves["mtw.flip_angle"].readOnly, true);
  assert.equal(leaves["mtw.repetition_time"].readOnly, true);
  assert.equal(top.export_mtr.readOnly, false);
});

test("the reserved model key is never a row", () => {
  const rows = mergeSurface({ model: "vfa_t1", fit_type: "linear" }, {}, {
    protocolKeys: [], readOnly: false,
  });
  assert.deepEqual(rows.map((r) => r.key), ["fit_type"]);
});

test("a recipe key absent from the surface still produces a row", () => {
  // mt_ratio's surface is `model: mt_ratio` alone, so a BIDS recipe's `mask`
  // key names something no model config knows about — it must still render,
  // editable, or it silently stops governing the fit from the reader's view.
  const rows = flat(mergeSurface({}, { mask: { desc: "brain" } }, {
    protocolKeys: [], readOnly: true,
  }));
  assert.ok(rows.mask, "mask row missing entirely");
  assert.equal(rows.mask.isSet, true);
  assert.equal(rows.mask.readOnly, false);
});

test("a nested override-only object produces a group with its child", () => {
  const rows = mergeSurface({}, { mask: { desc: "brain" } }, {
    protocolKeys: [], readOnly: true,
  });
  const group = rows.find((r) => r.key === "mask");
  assert.ok(group.children, "mask did not render as a group");
  const inner = flat(group.children);
  assert.equal(inner["mask.desc"].value, "brain");
  assert.equal(inner["mask.desc"].isSet, true);
  assert.equal(inner["mask.desc"].readOnly, false);
});

test("surface-derived rows come first, override-only rows after", () => {
  const rows = mergeSurface(
    { fit_type: "linear" },
    { fit_type: "nonlinear", mask: { desc: "brain" } },
    { protocolKeys: [], readOnly: false },
  );
  assert.deepEqual(rows.map((r) => r.key), ["fit_type", "mask"]);
});

test("an unsettable optional (null surface value, not protocol-sourced) is omitted while unset", () => {
  const rows = flat(mergeSurface(
    { b1_correction: null, export_mtr: true },
    {},
    { protocolKeys: [], readOnly: true },
  ));
  assert.equal(rows.b1_correction, undefined, "must not render a box for an unproducible value");
  assert.ok(rows.export_mtr, "an ordinary field must still render");
});

test("an unsettable optional renders read-only when a recipe does carry a value", () => {
  const rows = flat(mergeSurface(
    { b1_correction: null },
    { b1_correction: { m0b_vs_r1: { slope: 0.05 } } },
    { protocolKeys: [], readOnly: false },
  ));
  assert.equal(rows.b1_correction.isSet, true);
  assert.equal(rows.b1_correction.readOnly, true, "still read-only even in non-BIDS mode");
});

test("a protocol-sourced null value is unaffected by the unsettable-optional rule", () => {
  // inversion_recovery's repetition_time: not producible by hand, but IS
  // protocol-sourced, so it must follow the normal locked-row treatment
  // (shown, not omitted) rather than being hidden.
  const rows = flat(mergeSurface(
    { repetition_time: null, inversion_times: [] },
    {},
    { protocolKeys: ["repetition_time"], readOnly: true },
  ));
  assert.ok(rows.repetition_time, "protocol-sourced key must still render");
  assert.equal(rows.repetition_time.readOnly, true);
});

test("withProtocolComments appends one commented line per resolved protocol key", () => {
  const text = "model: vfa_t1\nfit_type: linear\n";
  const surface = { flip_angles: [3, 20], repetition_time: 0.015, fit_type: "linear" };
  const out = withProtocolComments(text, surface, ["flip_angles", "repetition_time"]);
  assert.ok(out.startsWith(text.trimEnd()), "original text is preserved");
  assert.match(out, /# flip_angles: \[3, 20\]\s+# from sidecars/);
  assert.match(out, /# repetition_time: 0\.015\s+# from sidecars/);
});

test("withProtocolComments is a no-op with no protocol keys", () => {
  const text = "model: mt_ratio\n";
  assert.equal(withProtocolComments(text, {}, []), text);
});

test("withProtocolComments skips a declared key absent from the surface", () => {
  const text = "model: vfa_t1\n";
  const out = withProtocolComments(text, { flip_angles: [3, 20] }, ["flip_angles", "nonexistent"]);
  assert.match(out, /flip_angles/);
  assert.doesNotMatch(out, /nonexistent/);
});

test("stripProtocolComments removes an appended block and is idempotent", () => {
  const text = "model: vfa_t1\nfit_type: linear\n";
  const withComments = withProtocolComments(text, { flip_angles: [3, 20] }, ["flip_angles"]);
  const stripped = stripProtocolComments(withComments);
  assert.equal(stripped, text);
  assert.equal(stripProtocolComments(stripped), stripped, "stripping twice is a no-op");
});

test("stripProtocolComments is a no-op on text with no comment block", () => {
  const text = "model: vfa_t1\nfit_type: linear\n";
  assert.equal(stripProtocolComments(text), text);
});

test("isSet is true for an override that is explicitly false, 0, or null", () => {
  const rows = flat(mergeSurface(
    { a: true, b: 1, c: "x" },
    { a: false, b: 0, c: null },
    { protocolKeys: [], readOnly: false },
  ));
  assert.equal(rows.a.isSet, true);
  assert.equal(rows.a.value, false);
  assert.equal(rows.b.isSet, true);
  assert.equal(rows.b.value, 0);
  assert.equal(rows.c.isSet, true);
  assert.equal(rows.c.value, null);
});

test("protocol rows come before options, each keeping config order", () => {
  // What the dataset dictates is the ground a reader stands on before choosing
  // anything; interleaving locked and editable controls also reads as noise.
  const rows = mergeSurface(
    { fit_type: "linear", flip_angles: [3, 20], drop_first: false, repetition_time: 0.015 },
    { mask: { desc: "brain" } },
    { protocolKeys: ["flip_angles", "repetition_time"], readOnly: true },
  );
  assert.deepEqual(rows.map((r) => r.key), [
    "flip_angles", "repetition_time",   // protocol, in config order
    "fit_type", "drop_first",           // options, in config order
    "mask",                             // override-only, last
  ]);
});

test("ordering holds when nothing is protocol-sourced", () => {
  const rows = mergeSurface(
    { fit_type: "linear", flip_angles: [3, 20] },
    {},
    { protocolKeys: [], readOnly: false },
  );
  assert.deepEqual(rows.map((r) => r.key), ["fit_type", "flip_angles"]);
});

test("readNumbers accepts commas, newlines, and both together", () => {
  // The widget shows a comma line at rest and a column while editing, so both
  // separators are live at once — and a pasted comma list must survive.
  assert.deepEqual(readNumbers("0.35, 0.5, 0.65"), [0.35, 0.5, 0.65]);
  assert.deepEqual(readNumbers("0.35\n0.5\n0.65"), [0.35, 0.5, 0.65]);
  assert.deepEqual(readNumbers("0.35, 0.5\n0.65,"), [0.35, 0.5, 0.65]);
  assert.deepEqual(readNumbers("  3 ,\n\n 20 \n"), [3, 20]);
});

test("readNumbers drops what is not a number rather than yielding NaN", () => {
  // A NaN reaching the recipe would serialize as `.nan` and fail the fit later,
  // far from the typo that caused it.
  assert.deepEqual(readNumbers("3, oops, 20"), [3, 20]);
  assert.deepEqual(readNumbers(""), []);
  assert.deepEqual(readNumbers("Infinity, 5"), [5]);
});
