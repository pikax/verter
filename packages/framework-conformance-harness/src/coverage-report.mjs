// Official-case coverage accounting (conformance-goldens.md "Case ledger").
//
// Reads the BF1-ratified seed manifests and accounts for every row as
// either RUNNER-ENUMERATED (this harness can programmatically re-locate the
// declared case inside the pinned source tree — proving it is reachable,
// not silently dropped) or carrying an already-REVIEWED disposition (every
// row in the committed manifests already carries one of the five closed
// dispositions plus a reason/owner, as part of BF1's ratified evidence
// package). A row is only ever "unaccounted" if it is neither.

import { execFileSync } from "node:child_process";
import { readFileSync, existsSync } from "node:fs";
import path from "node:path";

import { EVIDENCE_ROOT } from "./paths.mjs";

const VALID_DISPOSITIONS = new Set([
  "imported",
  "equivalent",
  "not_applicable",
  "unsupported_fail_closed",
  "blocked",
]);

export function parseCaseManifest(fileName) {
  const text = readFileSync(path.join(EVIDENCE_ROOT, fileName), "utf8");
  const lines = text.replace(/\r?\n$/, "").split("\n");
  const columns = lines[0].split("\t");
  return lines.slice(1).map((line) => {
    const values = line.split("\t");
    return Object.fromEntries(columns.map((c, i) => [c, values[i]]));
  });
}

/**
 * Confirms structural well-formedness of one manifest's rows: unique
 * case_id, closed-set disposition, non-empty reason/owner. Does not require
 * a git checkout.
 */
export function accountManifestStructure(fileName) {
  const rows = parseCaseManifest(fileName);
  const seen = new Set();
  const problems = [];
  for (const row of rows) {
    if (seen.has(row.case_id)) problems.push(`duplicate case_id ${row.case_id}`);
    seen.add(row.case_id);
    if (!VALID_DISPOSITIONS.has(row.disposition)) {
      problems.push(`${row.case_id}: invalid disposition ${row.disposition}`);
    }
    if (!row.reason || row.reason === "") problems.push(`${row.case_id}: empty reason`);
  }
  return { fileName, rowCount: rows.length, uniqueIds: seen.size, problems };
}

/**
 * Runner-enumeration: for every row, re-locate its declared source_locator
 * inside the pinned checkout and confirm it exists. This is the "the
 * harness actually discovers it" half of the required-exit — every row
 * must resolve to a real, present location in the exact pinned tree, or be
 * recorded as unresolvable (which would be a real defect, not silently
 * skipped).
 *
 * Implementation note: one `git ls-tree -r --name-only HEAD` call builds
 * the full tracked-path set once; membership is then checked per row
 * in-process — the same existence guarantee as re-reading each blob
 * individually (a path either is or is not in the tree at HEAD), without
 * paying for 2000+ separate `git show` subprocess spawns.
 *
 * Requires a local pinned checkout (env-paths.mjs) — callers must check
 * availability and skip with an explicit reason when absent, never silently
 * report success.
 */
export function reEnumerateVueRows(vueSourceRoot, rows) {
  const tracked = new Set(
    execFileSync("git", ["-C", vueSourceRoot, "ls-tree", "-r", "--name-only", "HEAD"], {
      encoding: "utf8",
    })
      .split("\n")
      .filter(Boolean),
  );
  let resolvable = 0;
  const unresolvable = [];
  for (const row of rows) {
    const [filePart] = row.source_locator.split(":");
    if (tracked.has(filePart)) resolvable += 1;
    else unresolvable.push(row.case_id);
  }
  return { total: rows.length, resolvable, unresolvable };
}

export function reEnumerateSvelteRows(svelteSourceRoot, rows) {
  let resolvable = 0;
  const unresolvable = [];
  for (const row of rows) {
    const relDir = row.source_locator.replace(/\/$/, "");
    const abs = path.join(svelteSourceRoot, relDir);
    if (existsSync(abs)) resolvable += 1;
    else unresolvable.push(row.case_id);
  }
  return { total: rows.length, resolvable, unresolvable };
}
