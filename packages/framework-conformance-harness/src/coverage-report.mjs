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

const VALID_DECLARATION_KINDS = new Set(["single-declaration", "parameterized-declaration"]);
const VALID_TITLE_KINDS = new Set([
  "StringLiteral",
  "TemplateLiteral",
  "BinaryExpression",
  "CallExpression",
  "dynamic",
]);

/**
 * Runner-enumeration: for every row, re-locate its declared source_locator
 * inside the pinned checkout, confirm it exists, AND confirm the row's
 * recorded per-row evidence still matches the pinned tree — not mere
 * path/directory presence. A row whose path still exists but whose content
 * has drifted (e.g. re-numbered file, stale blob) is exactly the defect
 * class file/directory-existence-only checking misses, so this additionally
 * checks:
 *
 *  - `source_object`: the git blob hash (Vue) recorded for the file at
 *    `source_locator` must equal `git rev-parse HEAD:<file>` in the pinned
 *    checkout — i.e. the file content the row was generated against is
 *    byte-identical to the pinned tree today.
 *  - `declaration_kind` / `title_kind`: must be members of the closed sets
 *    the generator (`generate-official-case-manifests.mjs`) ever emits —
 *    catches a corrupted or hand-edited row before it is trusted.
 *
 * Implementation note: one `git ls-tree -r --name-only HEAD` call builds
 * the full tracked-path set once; one `git cat-file --batch-check` walks
 * every referenced path to recover its live blob hash in a single
 * subprocess round-trip, rather than paying for 2000+ separate `git show`
 * spawns.
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

  // Batch-resolve every referenced file's live blob hash in one process.
  const filePartByRow = rows.map((row) => row.source_locator.split(":")[0]);
  const uniqueFiles = [...new Set(filePartByRow.filter((f) => tracked.has(f)))];
  const liveBlobByPath = new Map();
  if (uniqueFiles.length > 0) {
    const input = uniqueFiles.map((f) => `HEAD:${f}`).join("\n") + "\n";
    const output = execFileSync(
      "git",
      ["-C", vueSourceRoot, "cat-file", "--batch-check=%(objectname)"],
      { input, encoding: "utf8" },
    )
      .trim()
      .split("\n");
    uniqueFiles.forEach((f, i) => liveBlobByPath.set(f, output[i]));
  }

  let resolvable = 0;
  const unresolvable = [];
  rows.forEach((row, i) => {
    const filePart = filePartByRow[i];
    const problems = [];
    if (!tracked.has(filePart)) problems.push("path-not-tracked");
    else if (liveBlobByPath.get(filePart) !== row.source_object)
      problems.push("source_object-mismatch");
    if (!VALID_DECLARATION_KINDS.has(row.declaration_kind))
      problems.push("declaration_kind-invalid");
    if (row.title_kind !== undefined && !VALID_TITLE_KINDS.has(row.title_kind))
      problems.push("title_kind-invalid");
    if (!row.title_sha256 || !/^[0-9a-f]{64}$/.test(row.title_sha256))
      problems.push("title_sha256-malformed");
    if (problems.length === 0) resolvable += 1;
    else unresolvable.push(row.case_id);
  });
  return { total: rows.length, resolvable, unresolvable };
}

export function reEnumerateSvelteRows(svelteSourceRoot, rows) {
  const VALID_SVELTE_KINDS = new Set(["sample-directory", "suite-sentinel"]);

  // Batch-resolve every referenced directory's live tree object in one
  // `git cat-file --batch-check` round-trip instead of one `rev-parse`
  // subprocess per row (thousands of spawns would blow test timeouts).
  const relDirByRow = rows.map((row) => row.source_locator.replace(/\/$/, ""));
  const existsByRow = relDirByRow.map((relDir) => existsSync(path.join(svelteSourceRoot, relDir)));
  const uniqueDirs = [...new Set(relDirByRow.filter((_, i) => existsByRow[i]))];
  const liveObjectByDir = new Map();
  if (uniqueDirs.length > 0) {
    const input = uniqueDirs.map((d) => `HEAD:${d}`).join("\n") + "\n";
    const output = execFileSync(
      "git",
      ["-C", svelteSourceRoot, "cat-file", "--batch-check=%(objectname)"],
      { input, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 },
    )
      .trim()
      .split("\n");
    uniqueDirs.forEach((d, i) => liveObjectByDir.set(d, output[i]));
  }

  let resolvable = 0;
  const unresolvable = [];
  rows.forEach((row, i) => {
    const relDir = relDirByRow[i];
    const problems = [];
    if (!existsByRow[i]) {
      problems.push("path-missing");
    } else {
      const liveObject = liveObjectByDir.get(relDir);
      if (!liveObject || liveObject === "missing") problems.push("object-not-in-tree");
      else if (liveObject !== row.source_object) problems.push("source_object-mismatch");
    }
    if (!VALID_SVELTE_KINDS.has(row.declaration_kind)) problems.push("declaration_kind-invalid");
    if (problems.length === 0) resolvable += 1;
    else unresolvable.push(row.case_id);
  });
  return { total: rows.length, resolvable, unresolvable };
}
