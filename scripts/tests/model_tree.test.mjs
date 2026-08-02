// The model picker's tree comes from the registry's taxonomy, not from a list
// written in the app: each payload carries `family`, `subgroup` and
// `category_order`, and grouping consecutive runs of that order rebuilds the
// tree. These check the rule against the taxonomy actually shipped, so a
// category added or re-cut in Rust shows up here rather than only in a browser.
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { modelTree } from "../../docs/playground/picker.js";

const DATA = "docs/playground/data";
const NOT_A_PAYLOAD = new Set(["index.json", "sources.json", "citation.json"]);

function shippedEntries() {
  return readdirSync(DATA)
    .filter((f) => f.endsWith(".json") && !NOT_A_PAYLOAD.has(f))
    .map((f) => ({
      name: f.replace(/\.json$/, ""),
      meta: JSON.parse(readFileSync(`${DATA}/${f}`, "utf8")),
    }));
}

test("every shipped payload carries the taxonomy the picker groups by", () => {
  for (const { name, meta } of shippedEntries()) {
    assert.equal(typeof meta.family, "string", `${name}: no family`);
    assert.ok(meta.family.length > 0, `${name}: empty family`);
    assert.equal(typeof meta.category_order, "number", `${name}: no category_order`);
    // `subgroup` is legitimately absent; it must be null rather than missing,
    // so grouping never conflates "no subgroup" with "not stated".
    assert.ok(
      meta.subgroup === null || typeof meta.subgroup === "string",
      `${name}: subgroup is neither null nor a string`,
    );
    assert.equal(typeof meta.bids_suffix, "string", `${name}: no bids_suffix`);
  }
});

test("the shipped models group into the intended reading order", () => {
  const tree = modelTree(shippedEntries());
  assert.deepEqual(
    tree.map((f) => f.family),
    ["T1 Relaxometry", "T2 Relaxometry", "Field Mapping", "Magnetization Transfer"],
  );

  const mt = tree.at(-1);
  assert.deepEqual(
    mt.groups.map((g) => g.subgroup),
    ["Semi-quantitative MT", "Quantitative MT"],
  );
  assert.deepEqual(
    mt.groups.map((g) => g.models.map((m) => m.name)),
    [["mt_ratio", "mt_sat"], ["qmt_spgr"]],
  );

  // A family with no subdivision still yields exactly one group, so the
  // renderer needs no special case for it.
  const t1 = tree[0];
  assert.equal(t1.groups.length, 1);
  assert.equal(t1.groups[0].subgroup, null);
  assert.deepEqual(t1.groups[0].models.map((m) => m.name), ["inversion_recovery", "vfa_t1"]);
});

test("every model in the index appears exactly once in the tree", () => {
  const index = JSON.parse(readFileSync(`${DATA}/index.json`, "utf8")).models;
  const placed = modelTree(shippedEntries())
    .flatMap((f) => f.groups)
    .flatMap((g) => g.models.map((m) => m.name));
  assert.deepEqual([...placed].sort(), [...index].sort());
  assert.equal(placed.length, new Set(placed).size, "a model was placed twice");
});

test("order comes from category_order, not from payload filename order", () => {
  // Reversed input must produce the same tree: the picker sorts by the
  // registry's order, so the order payloads happen to be read in cannot leak
  // into what the reader sees.
  const forward = modelTree(shippedEntries());
  const reversed = modelTree([...shippedEntries()].reverse());
  assert.deepEqual(
    reversed.map((f) => [f.family, f.groups.map((g) => g.models.map((m) => m.name))]),
    forward.map((f) => [f.family, f.groups.map((g) => g.models.map((m) => m.name))]),
  );
});
