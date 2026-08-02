// A config key is a serialization detail; the form shows the physical quantity
// it names, with the symbol and unit a methods section would use. The table is
// shared across models — `flip_angle` means the same thing wherever it appears.
import test from "node:test";
import assert from "node:assert/strict";
import { fieldLabel, fieldUnit, groupLabel } from "../../docs/playground/labels.js";

test("a quantity reads the same wherever it appears", () => {
  // mt_sat nests it per role, vfa_t1 has it at the top level: same label.
  for (const path of [["mtw", "flip_angle"], ["t1w", "flip_angle"], ["flip_angle"]]) {
    assert.equal(fieldLabel(path), "Flip Angle");
    assert.equal(fieldUnit(path), "FA°");
  }
});

test("labels carry the BIDS-native unit, not a converted one", () => {
  // Times are seconds throughout the recipe; a unit implying ms would be a lie.
  assert.equal(fieldUnit(["repetition_time"]), "TR [s]");
  assert.equal(fieldUnit(["echo_times"]), "TE [s]");
  assert.equal(fieldUnit(["inversion_times"]), "TI [s]");
});

test("the quantity and its symbol are held apart", () => {
  // The form puts them on separate lines, so neither may arrive with the
  // other's text baked in: a name carrying "(TI [s])" would print it twice.
  assert.equal(fieldLabel(["inversion_times"]), "Inversion Times");
  assert.equal(fieldUnit(["inversion_times"]), "TI [s]");
  for (const key of ["flip_angles", "echo_times", "offsets", "mtdata"]) {
    assert.ok(!/[(\[]/.test(fieldLabel([key])), `${key}: symbol leaked into the name`);
  }
});

test("an option with no physical dimension has no symbol line", () => {
  // `null`, not "": the renderer omits the second line entirely rather than
  // reserving an empty one.
  for (const key of ["fit_type", "export_mtr", "drop_first_echo", "some_new_option"]) {
    assert.equal(fieldUnit([key]), null, key);
  }
});

test("an exact path beats a leaf name", () => {
  // A bare `start` elsewhere is not necessarily a time.
  assert.equal(fieldLabel(["t1_range", "start"]), "T1 Range Start");
  assert.equal(fieldUnit(["t1_range", "start"]), "s");
});

test("an unlisted key stays readable rather than blank", () => {
  // A newly added option must not need a table entry to be usable.
  assert.equal(fieldLabel(["some_new_option"]), "Some New Option");
  assert.equal(fieldLabel(["fit_type"]), "Fit Type");
  assert.equal(fieldLabel([]), "");
});

test("role acronyms keep their own casing", () => {
  // `Mtw` would be wrong: these are the BIDS entities a reader already knows.
  assert.equal(groupLabel("mtw"), "MTw");
  assert.equal(groupLabel("pdw"), "PDw");
  assert.equal(groupLabel("t1w"), "T1w");
  assert.equal(groupLabel("unknown_group"), "Unknown Group");
});
