// The recipe form shows what a model actually accepts, not what the recipe
// happens to mention. That merge is the whole contract: a default the reader
// never typed must still appear (and be marked as a default), and a key the
// sidecars supply must not be presented as editable.
import test from "node:test";
import assert from "node:assert/strict";
import { mergeSurface, sameStructure } from "../../docs/playground/surface.js";

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

// `sameStructure` is what lets the form patch an edit in place instead of
// rebuilding — rebuilding a widget out from under the reader who just typed
// into it (or Tab-ed away from it) steals focus. It must say "same" whenever
// nothing that touches presence, grouping, or locking has changed, and "not
// the same" whenever anything the form must re-render around has.
test("identical shape, differing only in isSet/value, is the same structure", () => {
  const a = mergeSurface({ fit_type: "linear", repetition_time: 0.015 }, {}, {
    protocolKeys: [], readOnly: false,
  });
  const b = mergeSurface({ fit_type: "linear", repetition_time: 0.03 }, { fit_type: "nonlinear" }, {
    protocolKeys: [], readOnly: false,
  });
  assert.ok(sameStructure(a, b));
});

test("a key appearing or disappearing is not the same structure", () => {
  const a = mergeSurface({ fit_type: "linear" }, {}, { protocolKeys: [], readOnly: false });
  const b = mergeSurface({ fit_type: "linear", offset_term: false }, {}, {
    protocolKeys: [], readOnly: false,
  });
  assert.equal(sameStructure(a, b), false);
  assert.equal(sameStructure(b, a), false);
});

test("a lock flipping is not the same structure", () => {
  const surface = { flip_angles: [3, 20] };
  const locked = mergeSurface(surface, {}, { protocolKeys: ["flip_angles"], readOnly: true });
  const unlocked = mergeSurface(surface, {}, { protocolKeys: ["flip_angles"], readOnly: false });
  assert.equal(sameStructure(locked, unlocked), false);
});

test("a leaf becoming a group at the same key is not the same structure", () => {
  const a = mergeSurface({ mtw: 6 }, {}, { protocolKeys: [], readOnly: false });
  const b = mergeSurface({ mtw: { flip_angle: 6 } }, {}, { protocolKeys: [], readOnly: false });
  assert.equal(sameStructure(a, b), false);
});

test("a change nested inside a group is not the same structure", () => {
  const a = mergeSurface({ qmt_spgr: { lineshape: "gaussian" } }, {}, {
    protocolKeys: [], readOnly: false,
  });
  const b = mergeSurface({ qmt_spgr: { lineshape: "gaussian", fit: { r1f: 1 } } }, {}, {
    protocolKeys: [], readOnly: false,
  });
  assert.equal(sameStructure(a, b), false);
});
