// Run: node --test scripts/tests/*.test.mjs
import test from "node:test";
import assert from "node:assert/strict";
import {
  recipeForMode,
  SIM_MODES,
  statsRows,
  signalSeries,
  singleVoxelSeries,
} from "../../docs/playground/sim-series.js";

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

test("a signal report plots its own vector", () => {
  const s = signalSeries({ mode: "signal", signal: [3, 2, 1] });
  assert.deepEqual(s.values, [3, 2, 1]);
  assert.deepEqual(s.labels, ["1", "2", "3"]);
});

test("a signal report labels its points from its own identities, not a loaded dataset's", () => {
  // The report's identities describe the acquisition it was actually
  // simulated from (a Series model's per-sample parameter row), never
  // whatever dataset happened to be loaded before the recipe was edited.
  const s = signalSeries({
    mode: "signal",
    signal: [403, 410],
    identities: [{ InversionTime: 2.1 }, { InversionTime: 2.2 }],
  });
  assert.deepEqual(s.labels, ["InversionTime=2.1", "InversionTime=2.2"]);
});

test("a Named measurement's role identities format by role name", () => {
  const s = signalSeries({
    mode: "signal",
    signal: [1, 2, 3],
    identities: ["MTw", "PDw", "T1w"],
  });
  assert.deepEqual(s.labels, ["MTw", "PDw", "T1w"]);
});

test("identities shorter than the signal fall back to positions for the rest", () => {
  // A Series model whose fit excludes a volume returns fewer samples than the
  // acquisition has, so the two lengths genuinely differ.
  const s = signalSeries({
    mode: "signal",
    signal: [3, 2, 1],
    identities: [{ InversionTime: 0.35 }],
  });
  assert.deepEqual(s.labels, ["InversionTime=0.35", "2", "3"]);
});

test("single-voxel plots the three curves a recovery reading needs", () => {
  const s = singleVoxelSeries({
    mode: "single-voxel",
    clean_signal: [10, 8, 6],
    noisy_signal: [10.2, 7.7, 6.1],
    fitted_signal: [10.1, 7.9, 6.05],
    stats: [{ name: "T1", truth: 1, mean: 1, bias: 0, std: 0, rmse: 0 }],
  });
  assert.deepEqual(s.clean, [10, 8, 6]);
  assert.deepEqual(s.noisy, [10.2, 7.7, 6.1]);
  assert.deepEqual(s.fitted, [10.1, 7.9, 6.05]);
});

test("a single-voxel report missing a curve yields an empty one rather than throwing", () => {
  const s = singleVoxelSeries({ mode: "single-voxel", noisy_signal: [1, 2] });
  assert.deepEqual(s.noisy, [1, 2]);
  assert.deepEqual(s.clean, []);
  assert.deepEqual(s.fitted, []);
});

test("single-voxel labels its points from the report's own identities", () => {
  const s = singleVoxelSeries({
    mode: "single-voxel",
    clean_signal: [10, 8],
    noisy_signal: [10.2, 7.7],
    fitted_signal: [10.1, 7.9],
    identities: [{ InversionTime: 0.35 }, { InversionTime: 0.5 }],
  });
  assert.deepEqual(s.labels, ["InversionTime=0.35", "InversionTime=0.5"]);
});
