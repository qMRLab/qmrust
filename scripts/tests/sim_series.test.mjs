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
    { value: 0.5, stats: [{ name: "T1", truth: 0.5, mean: 0.55, std: 0.05, bias: 0.05, rmse: 0.07 }] },
    { value: 1.0, stats: [{ name: "T1", truth: 1.0, mean: 0.9, std: 0.10, bias: -0.10, rmse: 0.14 }] },
  ],
};

test("a sweep is plotted against the swept value", () => {
  assert.deepEqual(sensitivitySeries(sweep).xLabels, ["0.5", "1"]);
});

test("bias is relative to the truth at each point, so every parameter shares one axis", () => {
  const [t1] = sensitivitySeries(sweep).params;
  assert.equal(t1.name, "T1");
  assert.deepEqual(t1.bias, [10, -10]);
});

test("the band spans plus and minus one std, also relative", () => {
  const [t1] = sensitivitySeries(sweep).params;
  assert.deepEqual(t1.lower, [0, -20]);
  assert.deepEqual(t1.band, [20, 20]);
});

test("a point whose truth is zero has no relative bias and leaves a gap", () => {
  // Dividing by a zero truth would report Infinity as if it were a measurement.
  const zero = {
    mode: "sensitivity",
    swept_param: "b",
    points: [{ value: 0, stats: [{ name: "b", truth: 0, mean: 0.1, std: 0.1, bias: 0.1, rmse: 0.1 }] }],
  };
  const [b] = sensitivitySeries(zero).params;
  assert.deepEqual(b.bias, [null]);
  assert.deepEqual(b.lower, [null]);
  assert.deepEqual(b.band, [null]);
});

test("every parameter the report carries gets its own line", () => {
  const two = {
    mode: "sensitivity",
    swept_param: "F",
    points: [{
      value: 0.1,
      stats: [
        { name: "F", truth: 0.1, mean: 0.1, std: 0.01, bias: 0, rmse: 0.01 },
        { name: "kr", truth: 25, mean: 26, std: 2, bias: 1, rmse: 2 },
      ],
    }],
  };
  assert.deepEqual(sensitivitySeries(two).params.map((p) => p.name), ["F", "kr"]);
});

test("the band straddles the bias line whenever the std exceeds the bias", () => {
  // A stacked-area band is drawn as a lower edge plus a height on top of it,
  // so the two must recombine to the upper edge exactly, and the lower edge
  // must be free to go negative: a rendering that cannot place a negative
  // lower edge cannot show a band straddling zero.
  const wide = {
    mode: "sensitivity",
    swept_param: "MTR",
    points: [{ value: 5, stats: [{ name: "MTR", truth: 5, mean: 4.9, std: 1.311, bias: -0.09838, rmse: 1.3 }] }],
  };
  const [mtr] = sensitivitySeries(wide).params;
  const scale = 100 / 5;
  const upper = (-0.09838 + 1.311) * scale;
  assert.ok(mtr.lower[0] < 0, "the lower edge is negative when bias - std is negative");
  assert.equal(mtr.lower[0] + mtr.band[0], upper);
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
