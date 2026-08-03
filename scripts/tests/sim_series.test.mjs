// Run: node --test scripts/tests/*.test.mjs
import test from "node:test";
import assert from "node:assert/strict";
import { recipeForMode, SIM_MODES, statsRows } from "../../docs/playground/sim-series.js";

test("each page mode edits its own recipe", () => {
  const texts = { dataText: "model: a", simText: "model: a\nsim: {}" };
  assert.equal(recipeForMode("data", texts), "model: a");
  assert.equal(recipeForMode("sim", texts), "model: a\nsim: {}");
});

test("an unknown mode falls back to the data recipe rather than blanking the editor", () => {
  const texts = { dataText: "model: a", simText: "model: a\nsim: {}" };
  assert.equal(recipeForMode("", texts), "model: a");
});

test("the four modes the core implements are offered, in the order they answer questions", () => {
  assert.deepEqual(
    SIM_MODES.map((m) => m.id),
    ["signal", "single-voxel", "sensitivity", "montecarlo"],
  );
});

test("stats rows come straight from the report's own stats", () => {
  const report = {
    mode: "montecarlo",
    stats: [
      { name: "T1", truth: 0.9, mean: 0.902, std: 0.03, bias: 0.002, rmse: 0.031 },
    ],
  };
  assert.deepEqual(statsRows(report), [
    { name: "T1", truth: 0.9, mean: 0.902, bias: 0.002, std: 0.03, rmse: 0.031 },
  ]);
});

test("a report with no stats yields no rows rather than throwing", () => {
  assert.deepEqual(statsRows({ mode: "signal", signal: [1, 2] }), []);
});
