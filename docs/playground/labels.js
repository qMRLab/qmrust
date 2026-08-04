// Human labels for recipe fields.
//
// A config key is a snake_case identifier; a reader wants the physical quantity
// it names, with the symbol and unit they would find in a methods section. This
// table is a vocabulary of qMRI quantities keyed by config path — shared by
// every model, never per-model: `flip_angle` means the same thing in mt_sat as
// in vfa_t1, so it is spelled the same way in both.
//
// Units are BIDS-native (seconds, degrees, hertz, tesla), matching what the
// recipe actually carries, so a label never implies a conversion the fit does
// not do.
//
// A key with no entry falls back to title case, so an unlisted or newly added
// option is still readable rather than blank.

// Each entry is `[quantity, unit]`: the name a reader looks for, and the line
// beneath it. They are held apart rather than pre-joined into one string
// because the form shows them on separate lines, and splitting a composed
// label back up with a regex would make the display format the source of truth
// for the data.
//
// The unit line always ends in a parenthesised abbreviation, and reads as a
// phrase on its own: `TI (s)` where the quantity has a conventional symbol,
// `seconds (s)` where it does not. A bare `s` under `T1 Range Start` says less
// than the space it takes.
//
// A `null` unit means the option has no physical dimension (`Fit Type`);
// nothing is shown beneath it.

// Exact dotted paths win over leaf names, letting a nested field disambiguate
// (`t1_range.start` is a time; a bare `start` elsewhere might not be).
const BY_PATH = new Map([
  ["t1_range.start", ["T1 Range Start", "seconds (s)"]],
  ["t1_range.stop", ["T1 Range Stop", "seconds (s)"]],
  ["t1_range.step", ["T1 Range Step", "seconds (s)"]],
  ["sim.noise.type", ["Noise Type", null]],
  // The sweep's range carries the swept parameter's own unit, which changes
  // with the parameter chosen, so no unit is stated: a fixed "seconds (s)"
  // would be a lie the moment the sweep moves off a relaxation time.
  ["sim.sweep.param", ["Swept Parameter", null]],
  ["sim.sweep.start", ["Sweep Start", null]],
  ["sim.sweep.stop", ["Sweep Stop", null]],
  ["sim.sweep.steps", ["Sweep Steps", null]],
]);

const BY_KEY = new Map([
  ["flip_angle", ["Flip Angle", "FA (°)"]],
  ["flip_angles", ["Flip Angles", "FA (°)"]],
  ["repetition_time", ["Repetition Time", "TR (s)"]],
  ["repetition_times", ["Repetition Times", "TR (s)"]],
  ["echo_time", ["Echo Time", "TE (s)"]],
  ["echo_times", ["Echo Times", "TE (s)"]],
  ["inversion_time", ["Inversion Time", "TI (s)"]],
  ["inversion_times", ["Inversion Times", "TI (s)"]],
  ["saturation_time", ["Saturation Time", "seconds (s)"]],
  ["offsets", ["Offset Frequencies", "Δ (Hz)"]],
  ["angles", ["Saturation Flip Angles", "degrees (°)"]],
  ["mtdata", ["Saturation Angle / Offset", "degrees (°), hertz (Hz)"]],
  ["b1_correction_factor", ["B1 Correction Factor", null]],
  ["b1_correction", ["B1 Correction Surface", null]],
  ["export_mtr", ["Export MTR", null]],
  ["fit_type", ["Fit Type", null]],
  ["drop_first_echo", ["Drop First Echo", null]],
  ["offset_term", ["Offset Term", null]],
  // An acronym, not a word: title case would render it `Snr`. Dimensionless,
  // being a ratio of a signal to a standard deviation in the same units.
  ["snr", ["SNR", null]],
  // Spelled out, since `Std` is an abbreviation a reader has to expand. Both
  // carry the distributed parameter's own unit, which differs per parameter,
  // so neither states one.
  ["mean", ["Mean", null]],
  ["std", ["Standard Deviation", null]],
]);

// What an option does, for the hover beside its label.
//
// Only options carry one. An acquisition field does not: `Echo Times` is the
// echo times, and a note saying so is noise on every protocol row. An option is
// a choice about *how* to fit or *how* to simulate, and nothing on screen says
// what choosing it costs, so each entry answers that in a sentence: what it
// changes, and when a reader would want it.
//
// A simulated ground-truth value is not an option either. `T1` under Ground
// Truth is the T1 being simulated, so it is named by the quantity like any
// acquisition field; the settings around it (how much noise, how many trials,
// what to sweep) are the choices.
//
// Keyed the same way as the labels, exact path before leaf name, so a nested
// option (`zoom.points`) is described without colliding with a bare `points`
// elsewhere.
const HELP = new Map([
  ["method", "Whether the inversion-recovery data is complex (signed) or "
    + "magnitude. Magnitude data has lost the sign at the null, so the fit "
    + "restores polarity before solving."],
  ["t1_range.start", "Lower end of the T1 grid the search starts from. "
    + "Narrowing it speeds the fit; a true T1 outside it cannot be found."],
  ["t1_range.stop", "Upper end of the T1 grid. A tissue T1 above this is "
    + "clipped to the boundary rather than fitted."],
  ["t1_range.step", "Spacing of the initial T1 grid. The zoom refinement below "
    + "resolves finer than this, so it trades startup cost, not accuracy."],
  ["zoom.iterations", "How many times the search narrows around its best T1. "
    + "Each pass refines the estimate between the two neighbouring grid points."],
  ["zoom.points", "How many T1 values each zoom pass tries. More is finer but "
    + "costs a full grid evaluation per voxel."],
  ["fit_type", "Linear log-transforms the signal and solves in closed form: "
    + "fast, but it weights the noise unevenly. Nonlinear fits the signal "
    + "itself, which is slower and unbiased at low SNR."],
  ["drop_first_echo", "Discard the shortest echo, whose refocusing is "
    + "imperfect and biases T2 low. Costs one point from an already short "
    + "series."],
  ["offset_term", "Fit an additive constant alongside the decay, absorbing "
    + "residual signal that does not decay. Rejected with the linear fit, "
    + "which has no term to put it in."],
  ["b1_correction_factor", "Empirical scaling of the transmit correction "
    + "(Helms 2015). Applied only when the dataset supplies a B1 map; ignored "
    + "otherwise."],
  ["b1_correction", "A calibration surface from `qmrust mtsat-b1`, correcting "
    + "MTsat for transmit inhomogeneity. Without it the plain Helms "
    + "correction is used."],
  ["export_mtr", "Also write an MTR map from the same volumes. Needs the MT "
    + "and PD volumes to share a repetition time, since MTR is their ratio."],
  ["mask.desc", "Which mask in the dataset to fit inside, by its `desc-` "
    + "label. Left blank, any single mask present is used; a dataset with "
    + "none is fitted whole."],
  ["qmt_spgr.model", "Which two-pool approximation to fit. Ramani is a "
    + "closed-form steady-state solution; SledPikeRP integrates the "
    + "saturation pulse."],
  ["qmt_spgr.lineshape", "Absorption lineshape of the bound pool. "
    + "SuperLorentzian is standard for white matter; Gaussian and Lorentzian "
    + "suit other tissue."],
  ["qmt_spgr.read_pulse_alpha", "Flip angle of the imaging readout, which "
    + "saturates the free pool alongside the MT pulse."],
  ["qmt_spgr.pulse.shape", "Envelope of the off-resonance saturation pulse. "
    + "It sets the power deposited at each offset."],
  ["qmt_spgr.pulse.bandwidth", "Bandwidth of that saturation pulse, in hertz."],
  ["qmt_spgr.fitting.st", "Starting values for the six fitted parameters. A "
    + "poor start can settle the optimiser in a local minimum."],
  ["qmt_spgr.fitting.lb", "Lower bounds. A parameter pinned to its bound in "
    + "the output map means the data did not constrain it."],
  ["qmt_spgr.fitting.ub", "Upper bounds, read the same way as the lower ones."],
  ["qmt_spgr.fitting.fx", "Which parameters are held at their starting value "
    + "instead of fitted, one flag each."],
  ["qmt_spgr.fitting.fix_r1f_t2f", "Hold the free-pool R1*T2 product fixed "
    + "rather than fitting T2f, which the data constrains only weakly."],
  ["qmt_spgr.fitting.r1f_t2f", "The value that product is held at when fixed."],
  ["qmt_spgr.fitting.fix_r1r_eq_r1f", "Assume both pools relax at the same "
    + "rate, removing one poorly-determined parameter."],
  ["qmt_spgr.fitting.use_r1map_to_constrain_r1f", "Take R1f from a supplied "
    + "R1 map rather than fitting it, when the dataset provides one."],
  ["sim.noise.type", "Rician matches magnitude images, where noise no longer "
    + "averages out at low signal. Gaussian suits complex data; none simulates "
    + "a noise-free acquisition."],
  ["snr", "Peak clean signal over the noise standard deviation. Lower values "
    + "widen the fitted spread in every mode."],
  ["seed", "Fixes the random draw, so a run repeats exactly. Change it to see "
    + "another noise realisation of the same truth."],
  ["trials", "How many noisy repetitions to simulate and fit. More tightens "
    + "the reported statistics, at one fit each."],
  ["sim.sweep.param", "Which parameter Sensitivity varies. The others stay at "
    + "their ground truth."],
  ["sim.sweep.start", "Low end of the swept range, in the swept parameter's "
    + "own units."],
  ["sim.sweep.stop", "High end of that range. Widening it past what the "
    + "protocol can resolve is where bias appears."],
  ["sim.sweep.steps", "How many values across the range. Each one costs a full "
    + "set of trials."],
  ["mean", "Centre of the Gaussian each Multi-Voxel trial draws this parameter "
    + "from."],
  ["std", "Spread of that draw. Zero pins every trial to the mean, collapsing "
    + "this parameter's scatter to a line."],
]);

function titleCase(key) {
  return key
    .split("_")
    .filter((w) => w.length > 0)
    .map((w) => w[0].toUpperCase() + w.slice(1))
    .join(" ");
}

/**
 * The `[quantity, symbol]` pair for a field, or a title-cased fallback.
 *
 * `paramNames` is the loaded model's own parameter list (`param_names()`): a
 * key that matches one of them verbatim is a symbol, not a word, so it must
 * print exactly as the model spells it. `a` and `A`, or `kr`, are meaningless
 * once title-cased, and two models can give the same-looking key different
 * casing for different quantities (`mt_sat`'s `A` is not `inversion_recovery`'s
 * `a`).
 */
function entry(path, paramNames = []) {
  const dotted = path.join(".");
  const key = path.at(-1) ?? "";
  if (paramNames.includes(key)) return [key, BY_PATH.get(dotted)?.[1] ?? BY_KEY.get(key)?.[1] ?? null];
  return BY_PATH.get(dotted) ?? BY_KEY.get(key) ?? [titleCase(key), null];
}

/**
 * The quantity a config field names, given its dotted path segments.
 * `["mtw", "flip_angle"]` → `"Flip Angle"`. `paramNames` is the loaded model's
 * own parameter list, so a leaf that names one of them prints as that exact
 * symbol.
 */
export function fieldLabel(path, paramNames) {
  return entry(path, paramNames)[0];
}

/**
 * Its symbol and unit as a methods section would write them, or `null` for an
 * option with no physical dimension. `["inversion_times"]` → `"TI (s)"`.
 */
export function fieldUnit(path, paramNames) {
  return entry(path, paramNames)[1];
}

/**
 * One sentence on what an option does, or `null` for a field that needs none.
 * Acquisition fields have no entry: they are named by the quantity they carry.
 */
export function fieldHelp(path) {
  return HELP.get(path.join(".")) ?? HELP.get(path.at(-1) ?? "") ?? null;
}

/**
 * Every field path these tables name, dotted.
 *
 * Exported so a check over "all of them" iterates the tables themselves rather
 * than a list someone maintains alongside: a new entry is covered the moment it
 * is added, which is the only way such a check stays true.
 */
export function labelledPaths() {
  return [...new Set([...BY_PATH.keys(), ...BY_KEY.keys(), ...HELP.keys()])];
}

/**
 * The heading for a group of fields (`mtw` → `MTw`). Role names are acronyms a
 * reader already knows from the BIDS entities, so they keep their own casing
 * rather than being title-cased into `Mtw`.
 */
const GROUP_TITLES = new Map([
  ["mtw", "MTw"],
  ["pdw", "PDw"],
  ["t1w", "T1w"],
  ["t1_range", "T1 Range"],
  ["mask", "Mask"],
  ["zoom", "Zoom"],
  ["protocol", "Protocol"],
  ["pulse", "Saturation Pulse"],
  // `Sim` is an abbreviation and `Params` is jargon that does not say these
  // values are the truth being simulated. `sweep` is named for the mode that
  // reads it, so the heading and the mode picker agree. `noise` and
  // `distributions` title-case correctly and need no entry.
  ["sim", "Simulation"],
  ["params", "Ground Truth"],
  ["sweep", "Sensitivity Sweep"],
]);

export function groupLabel(key) {
  return GROUP_TITLES.get(key) ?? titleCase(key);
}
