// The recipe form's data model: what a model actually accepts, merged with what
// the reader's recipe pins.
//
// The form used to mirror the parsed recipe, so an option the recipe never
// mentioned was invisible and undiscoverable even though its default was what
// got fitted. Here the *surface* (every key the model accepts, defaults filled,
// from `effective_config`) decides which rows exist, and the recipe only
// decides their values. `isSet` keeps the difference legible: a defaulted value
// is still what will be fitted, but it is not something the recipe pins.

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
    for (const [key, value] of Object.entries(surfaceNode ?? {})) {
      // `model` is the reserved key selecting the model itself; the dropdown
      // above the editor owns it, so a row here would be a duplicate.
      if (path.length === 0 && key === "model") continue;
      const childPath = [...path, key];
      const has = isPlainObject(overrideNode) && key in overrideNode;
      const override = has ? overrideNode[key] : undefined;
      const isProtocol = inherited || protocol.has(childPath.join("."));
      const locked = isProtocol && readOnly;
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
    return rows;
  };
  return walk(surface, overrides, [], false);
}

/**
 * Whether two merged row trees have the same shape: the same paths, in the
 * same order, with the same group/leaf split and the same `readOnly` flags —
 * ignoring `isSet` and `value`. A row-driven form can only reuse its existing
 * DOM when this holds; if it does, no key was added or removed and nothing
 * newly locked or unlocked, so patching `isSet` in place is enough and the
 * node the reader is focused in never needs replacing.
 */
export function sameStructure(a, b) {
  if (a.length !== b.length) return false;
  return a.every((rowA, i) => {
    const rowB = b[i];
    if (rowA.path.join(".") !== rowB.path.join(".")) return false;
    if (rowA.readOnly !== rowB.readOnly) return false;
    const aHasChildren = rowA.children != null;
    const bHasChildren = rowB.children != null;
    if (aHasChildren !== bHasChildren) return false;
    return aHasChildren ? sameStructure(rowA.children, rowB.children) : true;
  });
}
