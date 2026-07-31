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
      // A locked row's only candidate value is the surface default — the BIDS
      // recipe omits the key by design — and displaying that default under a
      // "from sidecars" label would assert a specific dataset value that was
      // never actually resolved. `null` renders as a blank, honestly
      // unopinionated control instead.
      rows.push({
        path: childPath,
        key,
        value: isPlainObject(value) ? null : locked ? null : has ? override : value,
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
    return rows;
  };
  return walk(surface, overrides, [], false);
}
