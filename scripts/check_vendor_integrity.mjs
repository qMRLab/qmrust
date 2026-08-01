#!/usr/bin/env node
// Verifies every vendored third-party file against `vendor/MANIFEST.json`.
//
// The playground ships its dependencies as committed blobs, several of them
// minified and megabytes long. Nobody reads that diff, so without a recorded
// hash a change to vendored code is indistinguishable from no change at all,
// whether it arrives by accident, by a careless "quick patch", or by a
// contributor who wants it unread. Pinning turns each of those into a manifest
// edit sitting in plain sight beside it.
//
// Two directions, both enforced: a file whose hash moved, and a file present in
// the directory but absent from the manifest. The second matters more — adding
// an unlisted file is the easy way past a check that only looks at what it
// already knows about.
//
//   node scripts/check_vendor_integrity.mjs            # verify
//   node scripts/check_vendor_integrity.mjs --update   # re-pin after a deliberate upgrade
import { createHash } from "node:crypto";
import { readFileSync, readdirSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const VENDOR = "docs/playground/vendor";
const MANIFEST = `${VENDOR}/MANIFEST.json`;

export function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

/** Files in `dir` that a manifest is expected to account for. */
export function vendoredFiles(dir) {
  return readdirSync(dir)
    .filter((name) => name !== "MANIFEST.json")
    .sort();
}

/**
 * Problems with `entries` against the real files: a hash that moved, an entry
 * with no file, and a file with no entry.
 */
export function verify(entries, files, digestOf) {
  const problems = [];
  const listed = new Set(entries.map((e) => e.file));

  for (const entry of entries) {
    if (!files.includes(entry.file)) {
      problems.push(`${entry.file}: in the manifest, but not in ${VENDOR}`);
      continue;
    }
    const actual = digestOf(entry.file);
    if (actual !== entry.sha256) {
      problems.push(
        `${entry.file}: content changed\n` +
          `      recorded ${entry.sha256}\n` +
          `      actual   ${actual}\n` +
          "      If the upgrade was deliberate, re-pin it with --update and record its version.",
      );
    }
  }
  for (const file of files) {
    if (!listed.has(file)) {
      problems.push(`${file}: shipped to every visitor, but unlisted in the manifest`);
    }
  }
  return problems;
}

export function main({ update = false } = {}) {
  const manifest = JSON.parse(readFileSync(MANIFEST, "utf8"));
  const files = vendoredFiles(VENDOR);
  const digestOf = (file) => sha256(readFileSync(`${VENDOR}/${file}`));

  if (update) {
    for (const entry of manifest.files) {
      if (files.includes(entry.file)) entry.sha256 = digestOf(entry.file);
    }
    writeFileSync(MANIFEST, `${JSON.stringify(manifest, null, 2)}\n`);
    // Re-pinning cannot invent an entry: an unlisted file is a decision about
    // what the app ships, which belongs to whoever added it.
    return verify(manifest.files, files, digestOf).filter((p) => p.includes("unlisted"));
  }
  return verify(manifest.files, files, digestOf);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const problems = main({ update: process.argv.includes("--update") });
  if (problems.length) {
    console.error(`${problems.length} vendor integrity problem(s):`);
    for (const p of problems) console.error(" -", p);
    process.exit(1);
  }
  console.log("vendored files ok: every one accounted for and unchanged");
}
