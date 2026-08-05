// Run: node --test scripts/tests/*.test.mjs
import test from "node:test";
import assert from "node:assert/strict";
import {
  recipeForMode,
  SIM_MODES,
  simReads,
  statsRows,
  signalSeries,
  singleVoxelSeries,
  sensitivitySeries,
  multiVoxelScatter,
  multiVoxelErrors,
  errorHistogram,
  histogramXRange,
  sensitivityFinitePoints,
  sensitivityYExtent,
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

// A mode's form shows the settings its run reads and hides the rest, so these
// sets must match what the core's `run_*` actually consumes. Asserted as the
// distinguishing pairs rather than by restating all four lists, which would
// make the test a second copy of the table it is checking.
test("each mode reads its own settings and not another mode's", () => {
  // Every mode forwards from the ground truth, Multi-Voxel included, where it
  // supplies any parameter that has no distribution.
  for (const m of SIM_MODES) {
    assert.ok(simReads(m.id, "params"), `${m.id}: does not read the ground truth`);
  }
  // The two mode-specific blocks belong to exactly one mode each.
  assert.ok(simReads("sensitivity", "sweep"));
  assert.ok(simReads("montecarlo", "distributions"));
  for (const id of ["signal", "single-voxel", "montecarlo"]) {
    assert.ok(!simReads(id, "sweep"), `${id}: offers a sweep it does not read`);
  }
  for (const id of ["signal", "single-voxel", "sensitivity"]) {
    assert.ok(!simReads(id, "distributions"), `${id}: offers distributions it does not read`);
  }
  // Signal is the noise-free forward signal, so noise settings would do nothing.
  for (const key of ["noise", "seed", "trials"]) {
    assert.ok(!simReads("signal", key), `signal: offers ${key} it does not read`);
    assert.ok(simReads("single-voxel", key), `single-voxel: does not read ${key}`);
  }
});

test("an unknown mode is shown everything rather than an empty form", () => {
  // Hiding rows for a mode nobody declared would hide the mistake with them.
  assert.ok(simReads("not-a-mode", "sweep"));
  assert.ok(simReads("not-a-mode", "distributions"));
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

const mc = {
  mode: "montecarlo",
  trials: 4,
  stats: [
    { name: "T1", truth: 1.0, mean: 1.02, bias: 0.02, std: 0.05, rmse: 0.05 },
    { name: "M0", truth: 1000, mean: 1001, bias: 1, std: 10, rmse: 10 },
  ],
  per_trial_input: [[0.9, 990], [1.0, 1000], [1.1, 1010], [1.2, 1020]],
  per_trial_fitted: [[0.93, 995], [1.01, 1002], [1.08, 1008], [1.25, 1019]],
};

test("each parameter's scatter pairs the input a voxel was given with what was fitted", () => {
  const s = multiVoxelScatter(mc);
  assert.deepEqual(s.map((p) => p.name), ["T1", "M0"]);
  assert.deepEqual(s[0].points, [[0.9, 0.93], [1.0, 1.01], [1.1, 1.08], [1.2, 1.25]]);
  assert.deepEqual(s[1].points[0], [990, 995]);
});

test("errors are the fitted value minus the input that produced it", () => {
  const e = multiVoxelErrors(mc).find((p) => p.name === "T1");
  assert.equal(e.errors.length, 4);
  assert.ok(Math.abs(e.errors[0] - 0.03) < 1e-12);
  assert.ok(Math.abs(e.errors[3] - 0.05) < 1e-12);
});

test("a histogram bins values and reports a centre per bin", () => {
  const h = errorHistogram([0, 1, 1, 2], 2);
  assert.equal(h.counts.length, 2);
  assert.equal(h.centers.length, 2);
  assert.equal(h.counts.reduce((a, b) => a + b, 0), 4);
});

test("a histogram of identical values collapses to a single bin centred on that value", () => {
  const h = errorHistogram([3, 3, 3], 4);
  assert.deepEqual(h.centers, [3]);
  assert.deepEqual(h.counts, [3]);
});

test("an absent per-trial matrix yields nothing rather than throwing", () => {
  assert.deepEqual(multiVoxelScatter({ mode: "montecarlo", stats: [] }), []);
  assert.deepEqual(multiVoxelErrors({ mode: "montecarlo", stats: [] }), []);
});

// A diverged fit crosses `sim-worker.js`'s JSON boundary as `null`, and
// `null` arithmetics as 0: `null - input` is the finite number `-input`, so a
// check applied to the difference instead of to the fitted value would still
// accept it. These pin the fix to the operand, not the result.
const withDivergedTrial = {
  mode: "montecarlo",
  trials: 3,
  stats: [{ name: "M0", truth: 1000, mean: 1000, bias: 0, std: 1, rmse: 1 }],
  per_trial_input: [[1000], [1000], [1000]],
  per_trial_fitted: [[1001], [null], [999]],
};

test("a diverged trial (fitted null) is excluded from the scatter entirely", () => {
  const s = multiVoxelScatter(withDivergedTrial);
  assert.deepEqual(s[0].points, [[1000, 1001], [1000, 999]]);
});

test("a diverged trial (fitted null) is excluded from the error list, not turned into minus the input", () => {
  const e = multiVoxelErrors(withDivergedTrial).find((p) => p.name === "M0");
  assert.deepEqual(e.errors, [1, -1]);
  assert.ok(!e.errors.includes(-1000));
  assert.equal(e.dropped, 1);
});

test("a diverged trial's mean error is computed only from the usable trials", () => {
  const e = multiVoxelErrors(withDivergedTrial).find((p) => p.name === "M0");
  assert.equal(e.meanError, 0);
});

test("the histogram's x range is padded from the bin span", () => {
  const h = errorHistogram([1, 2, 3], 3);
  const [lo, hi] = histogramXRange(h, 2);
  assert.ok(lo < h.centers[0]);
  assert.ok(hi > h.centers[h.centers.length - 1]);
});

test("the histogram's x range widens to include zero even when every error shares a sign", () => {
  const h = errorHistogram([100, 110, 120], 3);
  const [lo] = histogramXRange(h, 110);
  assert.ok(lo <= 0);
});

test("the histogram's x range widens to include the mean error even when it falls outside the bins", () => {
  const h = errorHistogram([1, 2, 3], 3);
  const [, hi] = histogramXRange(h, 50);
  assert.ok(hi >= 50);
});

test("a sensitivity point with a non-finite mean or std is dropped rather than drawn with a fabricated zero spread", () => {
  const param = { name: "b", isSwept: false, x: [1, 2, 3], mean: [5, null, 7], std: [1, null, 1], truth: [6, 6, 6] };
  const finite = sensitivityFinitePoints(param);
  assert.deepEqual(finite.x, [1, 3]);
  assert.deepEqual(finite.mean, [5, 7]);
  assert.deepEqual(finite.std, [1, 1]);
});

test("a swept panel's y extent includes the identity diagonal's own span", () => {
  const param = { name: "T2", isSwept: true, x: [0.01, 0.2], mean: [0.05, 0.08], std: [0.01, 0.01], truth: [] };
  const finite = sensitivityFinitePoints(param);
  const [lo, hi] = sensitivityYExtent(param, finite);
  assert.ok(lo <= 0.01);
  assert.ok(hi >= 0.2);
});

test("a non-swept panel's y extent includes its constant truth", () => {
  const param = { name: "T2f", isSwept: false, x: [1, 2], mean: [0.5, 0.52], std: [0.01, 0.01], truth: [0.028, 0.028] };
  const finite = sensitivityFinitePoints(param);
  const [lo] = sensitivityYExtent(param, finite);
  assert.ok(lo <= 0.028);
});

test("a diverged sweep point does not drag the y extent to its own fabricated scale", () => {
  const param = { name: "b", isSwept: false, x: [1, 2, 3], mean: [5, -6.7e152, 7], std: [1, null, 1], truth: [6, 6, 6] };
  const finite = sensitivityFinitePoints(param);
  const [lo, hi] = sensitivityYExtent(param, finite);
  assert.ok(lo > -1e10);
  assert.ok(hi < 1e10);
});
