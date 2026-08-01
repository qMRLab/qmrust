// The archives this check guards live on a Zenodo record, so the interesting
// paths are the ones that only happen when something is already broken. Each is
// exercised here against the pure half of the check, which is why that half is
// split out from the fetching.
//
// Run: node --test scripts/tests/*.test.mjs
import test from "node:test";
import assert from "node:assert/strict";
import { readdirSync } from "node:fs";
import {
  classify,
  missingArchives,
  usesTheApiForm,
  wantedArchives,
} from "../check_dataset_archives.mjs";

const entries = (...keys) => keys.map((key) => ({ key }));

test("an archive the record does not hold is reported, by model", () => {
  const missing = missingArchives(entries("ds-irt1.zip"), [
    ["inversion_recovery", "ds-irt1.zip"],
    ["vfa_t1", "ds-vfa.zip"],
  ]);
  assert.deepEqual(missing, [["vfa_t1", "ds-vfa.zip"]]);
});

test("a record holding more than we ask for is fine", () => {
  // `ds-mts-b1.zip` sits on the record with nothing referencing it.
  assert.deepEqual(
    missingArchives(entries("ds-mts.zip", "ds-mts-b1.zip"), [["mt_sat", "ds-mts.zip"]]),
    [],
  );
});

test("only the /api/ form is accepted, since the other is CORS-blocked", () => {
  assert.ok(usesTheApiForm("https://zenodo.org/api/records/21696048/files"));
  // The human-facing form. It serves the same bytes and the browser refuses it.
  assert.ok(!usesTheApiForm("https://zenodo.org/records/21696048/files"));
  // A concept id would 404 on this route, but it is still shaped like the API
  // form; the record fetch is what catches that, not this.
  assert.ok(usesTheApiForm("https://zenodo.org/api/records/1/files"));
});

test("Zenodo's health is not this repository's problem, but a wrong id is", () => {
  assert.equal(classify(200), "read");
  assert.equal(classify(404), "fail", "a stale record id must fail loudly");
  assert.equal(classify(410), "fail");
  assert.equal(classify(500), "skip");
  assert.equal(classify(429), "skip");
  assert.equal(classify(503), "skip");
});

test("payloads are read for their archive, and non-payloads are left out", () => {
  const wanted = wantedArchives("docs/playground/data", [
    "vfa_t1.json",
    "sources.json",
    "citation.json",
    "index.json",
    "vfa_t1.nii.gz",
  ]);
  assert.deepEqual(wanted, [["vfa_t1", "ds-vfa.zip"]]);
});

test("every shipped payload names an archive, so none is silently unchecked", () => {
  // `wantedArchives` drops a payload with no `archive` key. If one ever lost
  // it, the check above would quietly stop covering that model.
  const dir = "docs/playground/data";
  const names = readdirSync(dir);
  const payloads = names.filter(
    (n) => n.endsWith(".json") && !["index.json", "sources.json", "citation.json"].includes(n),
  );
  assert.equal(wantedArchives(dir, names).length, payloads.length, payloads.join(", "));
});
