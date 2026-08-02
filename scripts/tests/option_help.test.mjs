// Every fitting option the recipe form shows must say what it does.
//
// The hover text lives in `labels.js`, away from the Rust config that defines
// the option, which is the risk this file exists to cover: a developer adding
// an option has no reason to look in the playground. So coverage is checked
// against the option surface the models actually publish, not against a list
// maintained beside the text. A new option arrives here as a failure naming it.
//
// The surface comes from `qmrust catalog`, whose `effective_config` is the
// fully-resolved config each model is fitted with — the same thing the form is
// built from. CI generates it before this runs (see `.github/workflows/
// docs.yml`); locally it appears after `qmrust catalog > catalog.json`.
import test from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { load as yamlLoad } from "../../docs/playground/vendor/js-yaml.js";
import { fieldHelp } from "../../docs/playground/labels.js";

const CATALOG = "catalog.json";

// Acquisition axes are named by the quantity they carry, so they carry no
// hover: a note on every protocol row is noise. Taken from each model's own
// `protocol_schema` rather than listed here, plus `model`, which names the
// model rather than configuring it.
function acquisitionKeys(models) {
  const keys = new Set(["model"]);
  for (const m of models) {
    for (const p of m.protocol_schema) keys.add(p.key || p.name);
  }
  // The schema names sidecar fields (`InversionTime`); the config names the
  // same axis in snake_case (`inversion_times`). Both spellings are excluded.
  for (const k of [...keys]) {
    const snake = k.replace(/([a-z])([A-Z])/g, "$1_$2").toLowerCase();
    keys.add(snake);
    keys.add(`${snake}s`);
  }
  return keys;
}

/** Every leaf config path in a model's resolved config, dotted. */
function leafPaths(node, prefix = "") {
  const out = [];
  for (const [key, value] of Object.entries(node ?? {})) {
    const path = prefix ? `${prefix}.${key}` : key;
    if (value && typeof value === "object" && !Array.isArray(value)) {
      out.push(...leafPaths(value, path));
    } else {
      out.push(path);
    }
  }
  return out;
}

test("every fitting option the form shows explains itself", (t) => {
  if (!existsSync(CATALOG)) {
    t.skip(`${CATALOG} not present; run: qmrust catalog > ${CATALOG}`);
    return;
  }
  const parsed = JSON.parse(readFileSync(CATALOG, "utf8"));
  const models = Array.isArray(parsed) ? parsed : parsed.models;
  const acquisition = acquisitionKeys(models);

  const missing = [];
  for (const model of models) {
    for (const path of leafPaths(yamlLoad(model.effective_config))) {
      const leaf = path.split(".").at(-1);
      if (acquisition.has(leaf) || acquisition.has(path)) continue;
      // A model's own protocol block is acquisition too, however it is nested.
      if (path.includes(".protocol.")) continue;
      if (!fieldHelp(path.split("."))) missing.push(`${model.name}: ${path}`);
    }
  }
  assert.deepEqual(
    missing,
    [],
    `options with no hover text (add one to HELP in labels.js):\n  ${missing.join("\n  ")}`,
  );
});
