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

// Each entry is `[quantity, symbol]`: the name a reader looks for, and the
// symbol-and-unit that names it in a methods section. They are held apart
// rather than pre-joined into one string because the form shows them on
// separate lines, and splitting a composed label back up with a regex would
// make the display format the source of truth for the data.
//
// A `null` symbol means the option has no physical dimension (`Fit Type`);
// nothing is shown beneath it.

// Exact dotted paths win over leaf names, letting a nested field disambiguate
// (`t1_range.start` is a time; a bare `start` elsewhere might not be).
const BY_PATH = new Map([
  ["t1_range.start", ["T1 Range Start", "s"]],
  ["t1_range.stop", ["T1 Range Stop", "s"]],
  ["t1_range.step", ["T1 Range Step", "s"]],
]);

const BY_KEY = new Map([
  ["flip_angle", ["Flip Angle", "FA°"]],
  ["flip_angles", ["Flip Angles", "FA°"]],
  ["repetition_time", ["Repetition Time", "TR [s]"]],
  ["repetition_times", ["Repetition Times", "TR [s]"]],
  ["echo_time", ["Echo Time", "TE [s]"]],
  ["echo_times", ["Echo Times", "TE [s]"]],
  ["inversion_time", ["Inversion Time", "TI [s]"]],
  ["inversion_times", ["Inversion Times", "TI [s]"]],
  ["saturation_time", ["Saturation Time", "s"]],
  ["offsets", ["Offset Frequencies", "Δ [Hz]"]],
  ["angles", ["Saturation Flip Angles", "°"]],
  ["mtdata", ["Saturation Angle / Offset", "° , Hz"]],
  ["b1_correction_factor", ["B1 Correction Factor", null]],
  ["b1_correction", ["B1 Correction Surface", null]],
  ["export_mtr", ["Export MTR", null]],
  ["fit_type", ["Fit Type", null]],
  ["drop_first_echo", ["Drop First Echo", null]],
  ["offset_term", ["Offset Term", null]],
]);

function titleCase(key) {
  return key
    .split("_")
    .filter((w) => w.length > 0)
    .map((w) => w[0].toUpperCase() + w.slice(1))
    .join(" ");
}

/** The `[quantity, symbol]` pair for a field, or a title-cased fallback. */
function entry(path) {
  const dotted = path.join(".");
  const key = path.at(-1) ?? "";
  return BY_PATH.get(dotted) ?? BY_KEY.get(key) ?? [titleCase(key), null];
}

/**
 * The quantity a config field names, given its dotted path segments.
 * `["mtw", "flip_angle"]` → `"Flip Angle"`.
 */
export function fieldLabel(path) {
  return entry(path)[0];
}

/**
 * Its symbol and unit as a methods section would write them, or `null` for an
 * option with no physical dimension. `["inversion_times"]` → `"TI [s]"`.
 */
export function fieldUnit(path) {
  return entry(path)[1];
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
]);

export function groupLabel(key) {
  return GROUP_TITLES.get(key) ?? titleCase(key);
}
