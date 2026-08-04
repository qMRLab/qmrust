// Run: node --test scripts/tests/*.test.mjs
import test from "node:test";
import assert from "node:assert/strict";
import {
  recipeForMode,
  SIM_MODES,
  statsRows,
  signalSeries,
  singleVoxelSeries,
  sensitivitySeries,
  montecarloBoxes,
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

const sweep = {
  mode: "sensitivity",
  swept_param: "T1",
  points: [
    { value: 0.5, stats: [
      { name: "T1", truth: 0.5, mean: 0.55, std: 0.05, bias: 0.05, rmse: 0.07 },
      { name: "a", truth: 500, mean: 498, std: 12, bias: -2, rmse: 12 },
    ] },
    { value: 1.0, stats: [
      { name: "T1", truth: 1.0, mean: 0.90, std: 0.10, bias: -0.10, rmse: 0.14 },
      { name: "a", truth: 500, mean: 502, std: 11, bias: 2, rmse: 11 },
    ] },
  ],
};

test("a sweep is plotted as fitted against the input that produced it", () => {
  const s = sensitivitySeries(sweep);
  assert.equal(s.swept, "T1");
  const t1 = s.params.find((p) => p.name === "T1");
  assert.deepEqual(t1.x, [0.5, 1.0]);
  assert.deepEqual(t1.mean, [0.55, 0.90]);
  assert.deepEqual(t1.std, [0.05, 0.10]);
});

test("the swept parameter is marked so its panel can carry the identity line", () => {
  const s = sensitivitySeries(sweep);
  assert.equal(s.params.find((p) => p.name === "T1").isSwept, true);
  assert.equal(s.params.find((p) => p.name === "a").isSwept, false);
});

test("a parameter that is not swept carries its constant truth for a reference line", () => {
  const a = sensitivitySeries(sweep).params.find((p) => p.name === "a");
  assert.deepEqual(a.truth, [500, 500]);
  assert.deepEqual(a.mean, [498, 502]);
});

test("every reported parameter gets a panel, in the report's own order", () => {
  assert.deepEqual(sensitivitySeries(sweep).params.map((p) => p.name), ["T1", "a"]);
});

test("a report with no points yields no panels rather than throwing", () => {
  assert.deepEqual(sensitivitySeries({ mode: "sensitivity", points: [] }).params, []);
});

test("a montecarlo report becomes one box per reported parameter", () => {
  const report = {
    mode: "montecarlo",
    trials: 5,
    stats: [
      { name: "T1", truth: 1, mean: 1, bias: 0, std: 0.1, rmse: 0.1 },
      { name: "M0", truth: 1000, mean: 1000, bias: 0, std: 10, rmse: 10 },
    ],
    per_trial_input: [
      [0, 0], [0, 0], [0, 0], [0, 0], [0, 0],
    ],
    per_trial_fitted: [
      [1, 10], [2, 20], [3, 30], [4, 40], [5, 50],
    ],
  };
  const boxes = montecarloBoxes(report);
  assert.deepEqual(boxes.map((b) => b.name), ["T1", "M0"]);
  // Five sorted errors (fitted - input): min 1, median 3, max 5.
  assert.equal(boxes[0].box[0], 1);
  assert.equal(boxes[0].box[2], 3);
  assert.equal(boxes[0].box[4], 5);
});

test("a report with no per-trial pairs yields no boxes rather than throwing", () => {
  assert.deepEqual(
    montecarloBoxes({ mode: "montecarlo", stats: [], per_trial_input: [], per_trial_fitted: [] }),
    [],
  );
});
