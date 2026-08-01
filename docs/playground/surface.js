// The recipe form's data model: what a model actually accepts, merged with what
// the reader's recipe pins.
//
// The *surface* (every key the model accepts, defaults filled, from
// `effective_config`) decides which rows exist for a model's own config, and
// the recipe only decides their values. `isSet` keeps that difference legible:
// a defaulted value is still what will be fitted, but it is not something the
// recipe pins. A recipe can also carry keys no model config knows about
// (`mask`, a shell-level resolution key); those still govern the fit, so a
// second pass appends a row for every override the surface walk didn't
// already cover — always editable, never protocol-sourced.

function isPlainObject(v) {
  return v !== null && typeof v === "object" && !Array.isArray(v);
}

/**
 * @param surface  every key the model accepts, at its effective value
 * @param overrides the reader's own parsed recipe (may be partial or empty)
 * @param protocolKeys dotted paths a resolved protocol supplies, already
 *   qualified by `effective_config` (e.g. `qmt_spgr.protocol.mtdata`). A path
 *   may name an object, which locks everything under it.
 * @param readOnly  true when a protocol is resolved (BIDS), locking those paths
 * @returns Row[] — nested objects carry `children` and a null `value`
 */
export function mergeSurface(surface, overrides, { protocolKeys = [], readOnly = false } = {}) {
  const protocol = new Set(protocolKeys);
  // `inherited` carries a lock down into an object's leaves: ingest_protocol
  // replaces whole weighting objects (mt_sat), so locking only the group row
  // would leave its fields editable and silently discarded.
  const walk = (surfaceNode, overrideNode, path, inherited) => {
    const rows = [];
    const fromSurface = new Set();
    for (const [key, value] of Object.entries(surfaceNode ?? {})) {
      // `model` is the reserved key selecting the model itself; the dropdown
      // above the editor owns it, so a row here would be a duplicate.
      if (path.length === 0 && key === "model") continue;
      fromSurface.add(key);
      const childPath = [...path, key];
      const has = isPlainObject(overrideNode) && key in overrideNode;
      const override = has ? overrideNode[key] : undefined;
      const isProtocol = inherited || protocol.has(childPath.join("."));
      const locked = isProtocol && readOnly;
      // A non-protocol key whose surface value is `null` is an optional the
      // reader has no way to produce (e.g. mt_sat's `b1_correction`, a
      // sequence-simulation artifact the CLI inlines from a file path) —
      // *not* a settable scalar that merely defaults to unset. An editable
      // box there can only mislead, so it is omitted while unset and
      // rendered read-only if a recipe happens to carry a value for it.
      // Known limitation: this also hides a genuine settable optional scalar
      // that serializes to `null` by default; a per-field declaration would
      // be the precise fix if one is ever added.
      if (value === null && !isProtocol) {
        if (!has) continue;
        rows.push({
          path: childPath,
          key,
          value: isPlainObject(override) ? null : override,
          isSet: true,
          isProtocol: false,
          readOnly: true,
          children: isPlainObject(override) ? walk(override, override, childPath, true) : null,
        });
        continue;
      }
      // The surface is built from `effective_config`, which runs
      // `ingest_protocol` before serializing — so a locked row's `value` is
      // already the real value resolved from the sidecars, not a struct
      // default. The BIDS recipe omits the key by design, so a locked row's
      // only source is ever the surface.
      rows.push({
        path: childPath,
        key,
        value: isPlainObject(value) ? null : has ? override : value,
        isSet: has,
        isProtocol,
        readOnly: locked,
        children: isPlainObject(value)
          ? walk(value, has ? override : undefined, childPath, isProtocol)
          : null,
      });
    }
    // A second pass over the recipe's own keys: anything the surface walk
    // above didn't already cover (no model config knows it, e.g. `mask`) still
    // governs the fit and must not silently vanish from the form. Recursed the
    // same way, from an empty surface node, so a nested override-only object
    // still renders as a group. Never protocol-sourced: the surface is the
    // only source of protocol-lockable paths.
    if (isPlainObject(overrideNode)) {
      for (const [key, value] of Object.entries(overrideNode)) {
        if (fromSurface.has(key)) continue;
        if (path.length === 0 && key === "model") continue;
        const childPath = [...path, key];
        rows.push({
          path: childPath,
          key,
          value: isPlainObject(value) ? null : value,
          isSet: true,
          isProtocol: false,
          readOnly: false,
          children: isPlainObject(value) ? walk({}, value, childPath, false) : null,
        });
      }
    }
    // Protocol first, then options. What the dataset dictates is the ground a
    // reader stands on before choosing anything, and grouping the locked rows
    // together also stops the greyed and editable controls from interleaving.
    // A stable partition, so each group keeps the config's own field order.
    return [...rows.filter((r) => r.isProtocol), ...rows.filter((r) => !r.isProtocol)];
  };
  return walk(surface, overrides, [], false);
}

// The YAML view's trailing context block: one commented line per
// protocol-sourced key, showing the value `effective_config` resolved for it.
// A BIDS recipe omits these keys entirely, so there is nothing in the text to
// comment out — the lines are generated for display, never parsed or
// committed. `PROTOCOL_COMMENT_HEADER` is the marker `stripProtocolComments`
// looks for; it must not occur in an ordinary recipe.
const PROTOCOL_COMMENT_HEADER = "# resolved from sidecars (read-only)";

function getByDottedPath(obj, dottedPath) {
  return dottedPath
    .split(".")
    .reduce((node, key) => (isPlainObject(node) ? node[key] : undefined), obj);
}

function formatProtocolValue(value) {
  if (Array.isArray(value)) return `[${value.join(", ")}]`;
  if (isPlainObject(value)) return JSON.stringify(value);
  return String(value);
}

/**
 * Appends the protocol-context block to `text` (the recipe as typed/dumped),
 * one line per key in `protocolKeys` that `surface` (the parsed
 * `effective_config` YAML, already ingested with the resolved protocol)
 * carries a value for. A no-op — returns `text` unchanged — when there are no
 * protocol keys to show (non-BIDS mode, or a model with no acquisition axis).
 */
export function withProtocolComments(text, surface, protocolKeys) {
  const lines = protocolKeys
    .map((key) => {
      const value = getByDottedPath(surface, key);
      return value === undefined
        ? null
        : `# ${key}: ${formatProtocolValue(value)}   # from sidecars`;
    })
    .filter((line) => line !== null);
  if (lines.length === 0) return text;
  return `${text.replace(/\s+$/, "")}\n\n${PROTOCOL_COMMENT_HEADER}\n${lines.join("\n")}\n`;
}

/**
 * Removes a `withProtocolComments` block, if present. Idempotent, and a
 * no-op on text carrying none. Must run on the textarea's content wherever it
 * is read back — the generated lines must never reach the YAML parser or the
 * committed recipe/provenance.
 */
export function stripProtocolComments(text) {
  const idx = text.indexOf(PROTOCOL_COMMENT_HEADER);
  if (idx === -1) return text;
  return `${text.slice(0, idx).replace(/\s+$/, "")}\n`;
}

/**
 * Numbers out of a free-text list, separated by commas, newlines, or both.
 *
 * The number-array widget shows one compact line at rest and one value per line
 * while editing, so both separators are live at once — a reader who pastes a
 * comma list into the expanded column, or leaves a trailing comma, gets what
 * they meant. Anything that is not a number is dropped rather than silently
 * becoming NaN in the recipe.
 */
export function readNumbers(text) {
  return String(text)
    .split(/[,\n]/)
    .map((s) => s.trim())
    .filter((s) => s.length > 0)
    .map(Number)
    .filter((n) => Number.isFinite(n));
}

/**
 * The resolved acquisition to build the option surface against, in the form
 * `effective_config` takes (`""` when nothing is resolved).
 *
 * Read from `app.dataset.resolved`, never `app.current`. Loading a dataset
 * assigns `app.dataset` and then sets the recipe text — which triggers the
 * first surface refresh — well before `app.current` exists, so sourcing this
 * from `app.current` yields `""` on that first pass and every sidecar-supplied
 * row renders the model's struct default (zeros) instead of the acquisition.
 * `app.dataset.resolved` is also the object `app.protocolResolved` is derived
 * from, so which rows lock and what those rows show cannot disagree.
 */
export function resolvedProtocolJson(app) {
  return app?.dataset?.resolved?.protocol_json ?? "";
}

/**
 * Deletes the recipe's own values for `protocolKeys`, in place, so the
 * sidecar-resolved values become the only source again.
 *
 * Re-locking the protocol has to remove what the reader typed, not merely stop
 * them typing more: a recipe that still carries an overridden flip angle keeps
 * showing it, keeps writing it into the output provenance, and no longer
 * matches the dataset it names. Parents left empty by a deletion are pruned —
 * a bare `protocol: {}` is noise nobody wrote.
 */
export function clearProtocolOverrides(obj, protocolKeys) {
  const deleteAt = (node, segments) => {
    if (!isPlainObject(node)) return;
    const [head, ...rest] = segments;
    if (rest.length === 0) {
      delete node[head];
      return;
    }
    deleteAt(node[head], rest);
    if (isPlainObject(node[head]) && Object.keys(node[head]).length === 0) delete node[head];
  };
  for (const key of protocolKeys) deleteAt(obj, key.split("."));
  return obj;
}
