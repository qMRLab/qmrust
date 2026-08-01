// A config key is a serialization detail; the form shows the physical quantity
// it names, with the symbol and unit a methods section would use. The table is
// shared across models — `flip_angle` means the same thing wherever it appears.
import test from "node:test";
import assert from "node:assert/strict";
import { fieldLabel, groupLabel } from "../../docs/playground/labels.js";

test("a quantity reads the same wherever it appears", () => {
  // mt_sat nests it per role, vfa_t1 has it at the top level: same label.
  assert.equal(fieldLabel(["mtw", "flip_angle"]), "Flip Angle (FA°)");
  assert.equal(fieldLabel(["t1w", "flip_angle"]), "Flip Angle (FA°)");
  assert.equal(fieldLabel(["flip_angle"]), "Flip Angle (FA°)");
});

test("labels carry the BIDS-native unit, not a converted one", () => {
  // Times are seconds throughout the recipe; a label implying ms would be a lie.
  assert.equal(fieldLabel(["repetition_time"]), "Repetition Time (TR [s])");
  assert.equal(fieldLabel(["echo_times"]), "Echo Times (TE [s])");
  assert.equal(fieldLabel(["inversion_times"]), "Inversion Times (TI [s])");
});

test("an exact path beats a leaf name", () => {
  // A bare `start` elsewhere is not necessarily a time.
  assert.equal(fieldLabel(["t1_range", "start"]), "T1 Range Start (s)");
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
