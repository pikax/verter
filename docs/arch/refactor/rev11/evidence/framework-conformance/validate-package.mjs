#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";

const repo = resolve(import.meta.dirname, "../../../../../..");
const root = resolve(repo, "docs/arch/refactor/rev11/evidence/framework-conformance");
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
  ["svelte-official-cases.tsv", 3457],
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
  assert(entry?.version === "3.6.0-rc.3", `Vue direct package ${name} drifted`);
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
  [
    "oracles/vue/package-lock.json",
    "0dd2290c0b7d01f4727953b838610727b18bcb999b634eeb8ab726508a34b951",
  ],
  ["oracles/vue/closure.tsv", "d5caba234d8545b8b7bc7cc4cca8b8cf63f8ed594140d7cae80f3c7ae64606b2"],
  [
    "oracles/svelte/package-lock.json",
    "0c27c9fc7bed24be3fd7a546b55b6ee5858b244a57613390a213fdb454b92ce2",
  ],
  [
    "oracles/svelte/closure.tsv",
    "3dc4209c2911700de92858e350ddda2e6f5f333874a2eb330125ee808910dbce",
  ],
  ["vue-official-cases.tsv", "30123a6d88e1e7382afdcc752b5438c3486dd462e59ce831742ad0a3a3dd95bd"],
  ["svelte-official-cases.tsv", "c251be5b8b1de3e58c526700c426e2502e8bd1eb1dd622e22119b667adee7a8e"],
]);
for (const [path, expected] of expectedDigests)
  assert(digest(path) === expected, `${path}: digest drift`);

function dagBlocks(text) {
  const rows = [];
  let row = null;
  for (const line of text.split(/\r?\n/)) {
    if (line === "[[block]]") {
      if (row) rows.push(row);
      row = {};
      continue;
    }
    if (!row) continue;
    let match = line.match(/^id = "([^"]+)"$/);
    if (match) row.id = match[1];
    match = line.match(/^predecessors = \[(.*)\]$/);
    if (match) row.predecessors = [...match[1].matchAll(/"([^"]+)"/g)].map((item) => item[1]);
  }
  if (row) rows.push(row);
  return rows;
}

const dagPath = resolve(repo, "docs/arch/refactor/rev11/program-dag.toml");
const dag = dagBlocks(readFileSync(dagPath, "utf8"));
assert(dag.length === 56, `DAG expected 56 rows, got ${dag.length}`);
const requiredEdges = {
  BF1: ["B1"],
  BF2: ["BF1"],
  BF3: ["BF2"],
  B2: ["BF3"],
  B3: ["BF3"],
  B4: ["B2", "B3"],
  BV1: ["B4"],
  BS1: ["B4"],
  B5: ["BV1", "BS1"],
  B6: ["B5"],
  C1: ["A6", "B1", "B2"],
  C2: ["B3", "B5", "C1"],
  C3: ["C2"],
  C4: ["B6", "C3"],
};
for (const [id, predecessors] of Object.entries(requiredEdges)) {
  const row = dag.find((candidate) => candidate.id === id);
  assert(
    JSON.stringify(row?.predecessors) === JSON.stringify(predecessors),
    `${id}: incorrect predecessors`,
  );
}
assert(
  createHash("sha256").update(readFileSync(dagPath)).digest("hex") ===
    "335e0863ba1f21473a24befc0093dc01bad4f065ff03e6716c113448be054489",
  "DAG digest drift",
);

for (const statePath of [
  "docs/arch/refactor/rev11/templates/program-state.template.toml",
  "docs/arch/architecture-lock/ledger/program-state.toml",
]) {
  const stateIds = dagBlocks(readFileSync(resolve(repo, statePath), "utf8")).map((row) => row.id);
  assert(
    JSON.stringify(stateIds) === JSON.stringify(dag.map((row) => row.id)),
    `${statePath}: block order/universe differs from DAG`,
  );
}

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
