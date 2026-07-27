// Summary statistics over a map's values. One quantile convention, shared: the
// display window and the ROI table are read side by side, so a reader comparing
// them must not be seeing two different definitions of a percentile.

// The `p`th percentile of an already-ascending array, interpolating linearly
// between the two neighbouring samples — the convention numpy's `percentile`
// and `scripts/docsfig/style.py` both use.
function quantile(sorted, p) {
  const idx = (p / 100) * (sorted.length - 1);
  const lo = Math.floor(idx);
  const hi = Math.ceil(idx);
  return lo === hi ? sorted[lo] : sorted[lo] + (sorted[hi] - sorted[lo]) * (idx - lo);
}

// Fixed 2nd/98th-percentile window over the finite (in-mask, fitted) values —
// the same rule `scripts/docsfig/style.py`'s `window()` uses for the
// committed figures, so the browser and the published figures agree, and a
// few boundary-stuck fit failures (e.g. a grid search pinned at its T1 upper
// bound) cannot single-handedly drag the whole map's range out to them the
// way a plain min/max would.
export function percentileWindow(values) {
  const finite = [];
  for (let i = 0; i < values.length; i++) {
    const v = values[i];
    if (Number.isFinite(v)) finite.push(v);
  }
  if (finite.length === 0) return [0, 1];
  finite.sort((a, b) => a - b);
  const lo = quantile(finite, 2);
  const hi = quantile(finite, 98);
  return hi > lo ? [lo, hi] : [lo, lo + 1];
}

// Summary statistics of `values`, which must already be the in-ROI finite set.
export function describeValues(values) {
  const v = Float64Array.from(values).sort();
  const n = v.length;
  const mean = v.reduce((a, b) => a + b, 0) / n;
  // Sample standard deviation (n-1): these voxels are a sample of a region, not
  // the whole population of it.
  const sd = n > 1 ? Math.sqrt(v.reduce((a, b) => a + (b - mean) ** 2, 0) / (n - 1)) : 0;
  const q1 = quantile(v, 25);
  const q3 = quantile(v, 75);
  return {
    n,
    mean,
    sd,
    min: v[0],
    max: v[n - 1],
    median: quantile(v, 50),
    q1,
    q3,
    iqr: q3 - q1,
  };
}
