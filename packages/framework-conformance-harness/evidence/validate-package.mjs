#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

const repo = resolve(import.meta.dirname, "../../..");
const root = import.meta.dirname;
// Sole authority for pin identities and evidence-lock digests (see that
// module's header) — avoids a second hardcoded copy drifting out of sync
// with it, as happened here across the 3.6.0-rc.3 -> 3.6.0-rc.5 bump.
// pathToFileURL is required: a bare OS path (e.g. `C:\...` on Windows) is
// not a valid dynamic-import specifier there.
const { VUE_DOMAIN, EVIDENCE_LOCK_DIGESTS } = await import(
  pathToFileURL(resolve(repo, "packages/framework-conformance-harness/src/domain-pin.mjs")).href
);
let checks = 0;

let reviewPhase;
let reviewedCommit;
let reviewedTree;
const args = process.argv.slice(2);
for (let index = 0; index < args.length; index += 1) {
  const arg = args[index];
  if (arg === "--pre-review") {
    if (reviewPhase !== undefined) throw new Error("review phase specified more than once");
    reviewPhase = "pre-review";
  } else if (arg === "--post-review") {
    if (reviewPhase !== undefined) throw new Error("review phase specified more than once");
    reviewPhase = "post-review";
  } else if (arg === "--reviewed-commit") {
    reviewedCommit = args[++index];
    if (!reviewedCommit) throw new Error("--reviewed-commit requires a value");
  } else if (arg === "--reviewed-tree") {
    reviewedTree = args[++index];
    if (!reviewedTree) throw new Error("--reviewed-tree requires a value");
  } else {
    throw new Error(`unknown argument: ${arg}`);
  }
}
reviewPhase ??= "pre-review";

function assert(condition, message) {
  checks += 1;
  if (!condition) throw new Error(message);
}

function read(path) {
  return readFileSync(resolve(root, path), "utf8");
}

function digest(path) {
  return createHash("sha256")
    .update(readFileSync(resolve(root, path)))
    .digest("hex");
}

function parseTsv(path) {
  const lines = read(path)
    .replace(/\r?\n$/, "")
    .split("\n");
  const columns = lines[0].split("\t");
  const rows = lines.slice(1).map((line, index) => {
    const values = line.split("\t");
    assert(
      values.length === columns.length,
      `${path}:${index + 2}: expected ${columns.length} columns, got ${values.length}`,
    );
    return Object.fromEntries(columns.map((column, i) => [column, values[i]]));
  });
  assert(rows.length > 0, `${path}: zero rows`);
  return rows;
}

const classes = new Set([
  "supported canonical",
  "derived",
  "host-resolved",
  "test-only",
  "external",
  "unsupported fail-closed",
  "not applicable",
]);
const optionCounts = new Map([
  ["vue-options.tsv", 118],
  ["svelte-options.tsv", 35],
]);
for (const path of ["vue-options.tsv", "svelte-options.tsv"]) {
  const rows = parseTsv(path);
  assert(
    rows.length === optionCounts.get(path),
    `${path}: expected ${optionCounts.get(path)} rows, got ${rows.length}`,
  );
  const seen = new Set();
  for (const row of rows) {
    const key = `${row.surface}\0${row.option}`;
    assert(!seen.has(key), `${path}: duplicate ${row.surface}/${row.option}`);
    seen.add(key);
    assert(
      classes.has(row.classification),
      `${path}: invalid classification ${row.classification}`,
    );
    assert(row["canonical treatment / refusal"].length > 0, `${path}: empty treatment for ${key}`);
  }
}

const caseDispositions = new Set([
  "imported",
  "equivalent",
  "not_applicable",
  "unsupported_fail_closed",
  "blocked",
]);
const caseCounts = new Map([
  ["vue-official-cases.tsv", 2003],
  ["svelte-official-cases.tsv", 3475],
]);
for (const path of ["vue-official-cases.tsv", "svelte-official-cases.tsv"]) {
  const rows = parseTsv(path);
  assert(
    rows.length === caseCounts.get(path),
    `${path}: expected ${caseCounts.get(path)} rows, got ${rows.length}`,
  );
  const seen = new Set();
  for (const row of rows) {
    assert(!seen.has(row.case_id), `${path}: duplicate case ID ${row.case_id}`);
    seen.add(row.case_id);
    assert(
      caseDispositions.has(row.disposition),
      `${path}: invalid disposition ${row.disposition}`,
    );
    assert(
      /^[0-9a-f]{40}$/.test(row.source_object),
      `${path}: invalid source object ${row.source_object}`,
    );
  }
}

const emitDispositions = new Set(["Preserve", "Converge", "Replace", "Delete", "Defer"]);
for (const row of parseTsv("emitter-mapping-dispositions.tsv")) {
  assert(emitDispositions.has(row.disposition), `invalid emitter disposition ${row.disposition}`);
}

const capabilityDispositions = new Set([
  "supported",
  "unsupported fail-closed",
  "experimental",
  "projection-required",
  "version-incompatible",
  "not applicable",
]);
for (const row of parseTsv("capability-matrix.tsv")) {
  assert(
    capabilityDispositions.has(row.target_disposition),
    `invalid capability disposition ${row.target_disposition} for ${row.cell_id}`,
  );
  assert(row.acceptance_id.startsWith("FC-"), `missing acceptance ID for ${row.cell_id}`);
}

const vueLock = JSON.parse(read("oracles/vue/package-lock.json"));
const svelteLock = JSON.parse(read("oracles/svelte/package-lock.json"));
assert(Object.keys(vueLock.packages).length - 1 === 25, "Vue closure is not 25 packages");
assert(Object.keys(svelteLock.packages).length - 1 === 20, "Svelte closure is not 20 packages");
const vueDirect = [
  "vue",
  "@vue/compiler-core",
  "@vue/compiler-dom",
  "@vue/compiler-sfc",
  "@vue/compiler-ssr",
  "@vue/compiler-vapor",
  "@vue/runtime-core",
  "@vue/runtime-dom",
  "@vue/runtime-vapor",
  "@vue/server-renderer",
  "@vue/reactivity",
  "@vue/shared",
];
for (const name of vueDirect) {
  const entry = vueLock.packages[`node_modules/${name}`];
  assert(entry?.version === VUE_DOMAIN.packageVersion, `Vue direct package ${name} drifted`);
  assert(entry.integrity?.startsWith("sha512-"), `Vue direct package ${name} has no integrity`);
}
assert(
  svelteLock.packages["node_modules/svelte"]?.version === "5.56.8",
  "Svelte direct package drifted",
);
assert(
  svelteLock.packages["node_modules/svelte"].integrity.startsWith("sha512-"),
  "Svelte has no integrity",
);

const expectedDigests = new Map([
  ["oracles/vue/package-lock.json", EVIDENCE_LOCK_DIGESTS.vuePackageLockSha256],
  ["oracles/vue/closure.tsv", EVIDENCE_LOCK_DIGESTS.vueClosureSha256],
  ["oracles/svelte/package-lock.json", EVIDENCE_LOCK_DIGESTS.sveltePackageLockSha256],
  ["oracles/svelte/closure.tsv", EVIDENCE_LOCK_DIGESTS.svelteClosureSha256],
  ["vue-official-cases.tsv", "76cbe75f5dbee5b6014ab44ec4b5e58ff77a65839fafdc40d7328dda30f456ba"],
  ["svelte-official-cases.tsv", "0ba28efe7aafde6463d0a0977d8297561525d1c6d4161ffec33d0b8369eaaa3c"],
]);
for (const [path, expected] of expectedDigests)
  assert(digest(path) === expected, `${path}: digest drift`);

const reports = [
  "architecture-challenge.md",
  "conformance-challenge.md",
  "governance-challenge.md",
];
if (reviewPhase === "pre-review") {
  assert(reviewedCommit === undefined, "--reviewed-commit is valid only with --post-review");
  assert(reviewedTree === undefined, "--reviewed-tree is valid only with --post-review");
  for (const report of reports) {
    assert(
      !existsSync(resolve(root, "reviews", report)),
      `${report} must be absent in pre-review preparation mode`,
    );
  }
} else {
  assert(/^[0-9a-f]{40}$/.test(reviewedCommit ?? ""), "post-review commit must be a full SHA");
  assert(/^[0-9a-f]{40}$/.test(reviewedTree ?? ""), "post-review tree must be a full OID");
  for (const report of reports) {
    const path = resolve(root, "reviews", report);
    assert(existsSync(path), `${report} must be attached in post-review mode`);
    const contents = readFileSync(path, "utf8");
    assert(contents.includes(reviewedCommit), `${report} does not bind reviewed commit`);
    assert(contents.includes(reviewedTree), `${report} does not bind reviewed tree`);
    assert(/\b(?:PASS|BLOCKING(?:_FINDINGS)?)\b/.test(contents), `${report} has no closed verdict`);
  }
}

process.stdout.write(
  `OK: AMD-005 package evidence validated in ${reviewPhase} mode (${checks} non-zero assertions)\n`,
);
