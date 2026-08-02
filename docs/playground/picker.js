// The model picker: a two-level, collapsible tree over the same taxonomy the
// documentation gallery uses.
//
// The `<select>` in the markup remains the value. This module hides it and
// draws a tree that mirrors it, so everything that already reads or writes
// `$("model").value` — `loadModel`, and a dropped dataset choosing the model
// that matches it — keeps working untouched, and the control still functions if
// this never runs.
//
// The tree's shape is not written here. Each payload carries `family`,
// `subgroup` and `category_order` from the registry's own `Category`, so
// sorting by that order and grouping consecutive runs reproduces the taxonomy.
// Adding a model, or re-cutting the categories, changes the registry and
// nothing in this file.
import { icon } from "./vendor/icons.js";
import { $ } from "./dom.js";

/**
 * Registry reading order: `category_order` first, then title within a category.
 * Shared so the picker's tree and the `<select>` it mirrors cannot drift into
 * two different orders.
 */
export function compareByTaxonomy(a, b) {
  return (
    (a.meta.category_order ?? 0) - (b.meta.category_order ?? 0) ||
    a.meta.title.localeCompare(b.meta.title)
  );
}

/**
 * The taxonomy as a tree, from a flat list of `{name, meta}` entries.
 *
 * `[{family, groups: [{subgroup, models}]}]`, in the registry's reading order.
 * Pure, so the shape the picker draws is checkable without a DOM: sorting by
 * `category_order` and grouping consecutive runs is the whole rule, and a
 * family with no subgroups yields a single group whose `subgroup` is null.
 */
export function modelTree(entries) {
  const ordered = [...entries].sort(compareByTaxonomy);
  const tree = [];
  for (const entry of ordered) {
    const family = entry.meta.family ?? "";
    const subgroup = entry.meta.subgroup ?? null;
    let node = tree.find((f) => f.family === family);
    if (!node) tree.push((node = { family, groups: [] }));
    let group = node.groups.find((g) => g.subgroup === subgroup);
    if (!group) node.groups.push((group = { subgroup, models: [] }));
    group.models.push(entry);
  }
  return tree;
}

// Families the reader has unfolded, by name. Opening the tree clears this, so
// every visit starts as the four family headings and nothing else: the list is
// short enough to scan whole, and the button already says which model is
// loaded, so nothing is hidden that the reader has not just been told.
const expanded = new Set();

let entries = [];

/**
 * Build the tree from the loaded payloads, keyed by model name.
 * `metas` is `{name: payloadMeta}` for every model in the index.
 */
export function buildModelTree(metas, onPick) {
  const select = $("model");
  const picker = $("model-picker");
  if (!select || !picker) return;

  // The options are already in taxonomy order; `modelTree` sorts anyway, so
  // this only has to carry them across.
  entries = [...select.options]
    .map((o) => ({ name: o.value, meta: metas[o.value] }))
    .filter((e) => e.meta);
  if (entries.length === 0) return;

  // The native control keeps the value but stops being the visible one. It is
  // hidden from assistive tech too, so the tree is not announced twice.
  select.hidden = true;
  select.setAttribute("aria-hidden", "true");
  select.tabIndex = -1;
  picker.hidden = false;

  $("model-button").onclick = (e) => {
    e.stopPropagation();
    toggleTree();
  };
  // A tree that cannot be dismissed is a trap; both of these are what a reader
  // reaches for, and they mirror how the segment menu closes.
  document.addEventListener("click", (e) => {
    if ($("model-tree").hidden) return;
    if (e.target.closest("#model-tree, #model-button")) return;
    closeTree();
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && !$("model-tree").hidden) {
      closeTree();
      $("model-button").focus();
    }
  });

  // A dropped dataset sets `.value` directly, so the label follows the select
  // rather than only the tree's own clicks.
  select.addEventListener("change", () => {
    syncButton();
    renderTree(onPick);
  });

  syncButton();
  renderTree(onPick);
}

function current() {
  return entries.find((e) => e.name === $("model").value) ?? entries[0];
}

function syncButton() {
  const button = $("model-button");
  const active = current();
  if (!button || !active) return;
  // A chevron rather than the swatch: the section label above already carries
  // that glyph, and what this button needs to say is that it opens.
  button.replaceChildren();
  button.innerHTML = icon("chevron-right", 13);
  const title = document.createElement("span");
  title.className = "model-button-title";
  title.textContent = active.meta.title;
  const suffix = document.createElement("span");
  suffix.className = "model-suffix";
  suffix.textContent = active.meta.bids_suffix ?? "";
  button.append(title, suffix);
  button.title = `${active.meta.title}: click to choose another model`;
}

function toggleTree() {
  const tree = $("model-tree");
  tree.hidden ? openTree() : closeTree();
}

function openTree() {
  // Collapsed every time, not merely the first: a picker that remembered last
  // visit's folds would open differently depending on history.
  expanded.clear();
  $("model-tree").hidden = false;
  $("model-button").setAttribute("aria-expanded", "true");
  renderTree(lastPick);
  $("model-tree").querySelector(".model-family")?.focus();
}

function closeTree() {
  $("model-tree").hidden = true;
  $("model-button").setAttribute("aria-expanded", "false");
}

let lastPick = null;

function renderTree(onPick) {
  lastPick = onPick ?? lastPick;
  const tree = $("model-tree");
  if (!tree) return;
  tree.replaceChildren();
  const active = current();

  for (const node of modelTree(entries)) {
    const count = node.groups.reduce((n, g) => n + g.models.length, 0);
    const isOpen = expanded.has(node.family);
    const holdsActive = node.groups.some((g) => g.models.some((m) => m.name === active?.name));
    tree.append(
      familyRow(node.family, node.groups[0]?.models[0]?.meta.family_icon, isOpen, count, holdsActive),
    );
    if (!isOpen) continue;

    const body = document.createElement("div");
    body.className = "model-family-body";
    // A `treeitem`'s children must sit in a `group` for the tree to be
    // navigable; without it the rows read as siblings of their family.
    body.setAttribute("role", "group");
    for (const group of node.groups) {
      if (!group.subgroup) {
        // No subdivision: the models sit directly on the family's own level.
        for (const entry of group.models) {
          body.append(optionRow(entry, entry.name === active?.name));
        }
        continue;
      }
      const head = document.createElement("div");
      head.className = "model-subgroup";
      head.textContent = group.subgroup;
      body.append(head);
      // A third level, so it gets its own indent step and guide line.
      const nested = document.createElement("div");
      nested.className = "model-subgroup-body";
      nested.setAttribute("role", "group");
      for (const entry of group.models) {
        nested.append(optionRow(entry, entry.name === active?.name));
      }
      body.append(nested);
    }
    tree.append(body);
  }
}

function familyRow(family, familyIcon, isOpen, count, holdsActive) {
  const row = document.createElement("button");
  row.type = "button";
  // Folded shut, the tree would otherwise say nothing about where the loaded
  // model sits; marking its family keeps that visible without unfolding.
  row.className = [
    "model-family",
    isOpen ? "open" : "",
    holdsActive ? "holds-active" : "",
  ].filter(Boolean).join(" ");
  row.setAttribute("role", "treeitem");
  row.setAttribute("aria-expanded", String(isOpen));
  // Fold chevron, then the family's own glyph: the first says what the row
  // does, the second says what it holds.
  row.innerHTML = icon("chevron-right", 13) + icon(familyIcon ?? "circle", 14);
  const label = document.createElement("span");
  label.textContent = family;
  const badge = document.createElement("span");
  badge.className = "model-count";
  badge.textContent = String(count);
  row.append(label, badge);
  row.onclick = (e) => {
    e.stopPropagation();
    if (expanded.has(family)) expanded.delete(family);
    else expanded.add(family);
    renderTree(lastPick);
  };
  return row;
}

function optionRow(entry, isActive) {
  const row = document.createElement("button");
  row.type = "button";
  row.className = isActive ? "model-option active" : "model-option";
  row.setAttribute("role", "treeitem");
  row.setAttribute("aria-selected", String(isActive));
  const title = document.createElement("span");
  title.textContent = entry.meta.title;
  const suffix = document.createElement("span");
  suffix.className = "model-suffix";
  suffix.textContent = entry.meta.bids_suffix ?? "";
  row.append(title, suffix);
  row.onclick = (e) => {
    e.stopPropagation();
    closeTree();
    if (entry.name === $("model").value) return;
    // Route the choice through the select so there is one path a model change
    // can take, whoever initiated it.
    $("model").value = entry.name;
    syncButton();
    lastPick?.(entry.name);
  };
  return row;
}
