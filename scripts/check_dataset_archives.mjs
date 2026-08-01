#!/usr/bin/env node
// Verifies that the dataset archives the playground fetches actually exist,
// at the URL the playground builds.
//
// The app's datasets live outside the repository, on a Zenodo record named by a
// hand-edited id in `sources.json`. Publishing a new archive mints a NEW
// version id, so that id, its DOI and its base URL are the one part of the
// pipeline kept in step by remembering to. Nothing else in CI reads it: the
// integration job fetches from OSF and rebuilds BIDS locally, so a wrong record
// id, an archive that was never uploaded, or a URL form the browser refuses all
// ship green and break only in front of a reader.
//
// Failure policy, because a check that reaches the network must not become a
// coin toss. Zenodo being unreachable or unwell is not this repository's
// problem and skips; an answered request that disagrees with what we ship is,
// and fails:
//
//   network error, 5xx, 429   ->  skip, exit 0
//   404 / 410 on the record   ->  fail: the id in sources.json is wrong
//   200 missing an archive    ->  fail: a model the app offers cannot load
//
//   node scripts/check_dataset_archives.mjs
import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";

const DATA = "docs/playground/data";

// Files in the data directory that are not model payloads.
const NOT_A_PAYLOAD = new Set(["index.json", "sources.json", "citation.json"]);

/** The archive each shipped payload will ask for, keyed by model. */
export function wantedArchives(dir, names) {
  return names
    .filter((name) => name.endsWith(".json") && !NOT_A_PAYLOAD.has(name))
    .map((name) => [
      name.replace(/\.json$/, ""),
      JSON.parse(readFileSync(`${dir}/${name}`, "utf8")).archive,
    ])
    .filter(([, archive]) => Boolean(archive));
}

/**
 * Zenodo serves two URL forms for the same file. The human-facing
 * `/records/<id>/files/<name>?download=1` sends no `Access-Control-Allow-Origin`
 * and the browser blocks it; only the `/api/` form sends `ACAO: *`. The app
 * cannot detect the difference until a reader clicks, so the form is asserted
 * here rather than discovered there.
 */
export function usesTheApiForm(base) {
  return /^https:\/\/zenodo\.org\/api\/records\/\d+\/files$/.test(base);
}

/** Archives the record does not hold. `entries` is Zenodo's own file listing. */
export function missingArchives(entries, wanted) {
  const present = new Set(entries.map((e) => e.key));
  return wanted.filter(([, archive]) => !present.has(archive));
}

/**
 * What a response status means for this check. Split out so the policy is
 * readable and testable on its own, rather than buried in a fetch handler.
 */
export function classify(status) {
  if (status === 200) return "read";
  if (status === 404 || status === 410) return "fail";
  return "skip";
}

async function main() {
  const sources = JSON.parse(readFileSync(`${DATA}/sources.json`, "utf8"));
  const wanted = wantedArchives(DATA, readdirSync(DATA));
  const problems = [];

  if (wanted.length === 0) {
    problems.push("no model payloads found: this check would verify nothing");
  }
  if (!usesTheApiForm(sources.base)) {
    problems.push(
      `sources.json 'base' is not Zenodo's /api/ content form, which is the ` +
        `only one that sends Access-Control-Allow-Origin: ${sources.base}`,
    );
  }
  if (problems.length) return { problems, checked: 0 };

  let response;
  try {
    response = await fetch(sources.base, { signal: AbortSignal.timeout(30_000) });
  } catch (e) {
    console.log(`skipped: cannot reach Zenodo (${e.message})`);
    return { problems: [], checked: 0 };
  }

  const verdict = classify(response.status);
  if (verdict === "skip") {
    console.log(`skipped: Zenodo answered ${response.status}`);
    return { problems: [], checked: 0 };
  }
  if (verdict === "fail") {
    return {
      problems: [
        `record ${sources.record} answered ${response.status}. A new Zenodo ` +
          "version mints a new id, so 'record', 'doi' and 'base' in " +
          "sources.json all have to be updated together.",
      ],
      checked: 0,
    };
  }

  const entries = (await response.json()).entries ?? [];
  for (const [model, archive] of missingArchives(entries, wanted)) {
    problems.push(
      `${model} asks for ${archive}, which record ${sources.record} does not ` +
        `hold. That model's dataset cannot load. Present: ` +
        entries.map((e) => e.key).join(", "),
    );
  }
  return { problems, checked: wanted.length };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const { problems, checked } = await main();
  if (problems.length) {
    console.error(`${problems.length} dataset archive problem(s):`);
    for (const p of problems) console.error(" -", p);
    process.exit(1);
  }
  if (checked) console.log(`dataset archives ok: all ${checked} resolve on the record`);
}
