#!/usr/bin/env node

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { LANE_GATES } from "./ci-impact.mjs";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(SCRIPT_DIR, "..");

function yamlJob(source, name) {
  const start = source.indexOf(`\n  ${name}:`);
  assert.notEqual(start, -1, `workflow must define the ${name} job`);
  const next = source.slice(start + 1).search(/\n  [a-z0-9][a-z0-9-]*:\r?\n/);
  return next === -1 ? source.slice(start) : source.slice(start, start + 1 + next);
}

test("compile-contract cache belongs to the required standalone runner job", () => {
  const workflow = readFileSync(join(REPO_ROOT, ".github", "workflows", "ci.yml"), "utf8");
  const rustJob = yamlJob(workflow, "rust-test");
  const compilerJob = yamlJob(workflow, "compiler-contracts");
  const successJob = yamlJob(workflow, "ci-success");

  assert.match(compilerJob, /needs:\s*detect-changes/);
  // Gated on the compile-contract owners' dependency closure, not on any
  // Rust change: an LSP-only or tooling-crate change does not re-run them.
  assert.match(compilerJob, /needs\.detect-changes\.outputs\.compiler_contracts\s*==\s*'true'/);
  assert.match(compilerJob, /target\/tests\/trybuild/);
  assert.match(compilerJob, /node scripts\/compile-contracts\.mjs/);
  assert.match(compilerJob, /generated_svelte_artifacts_match_their_authoritative_inputs/);
  assert.match(compilerJob, /cargo build -p verter_compiler/);
  assert.match(compilerJob, /cargo test -p verter_compiler --lib/);
  assert.doesNotMatch(compilerJob, /cargo nextest|install-action@nextest/);
  assert.doesNotMatch(compilerJob, /max-threads|test-threads|--jobs|-j\s*\d/);
  assert.doesNotMatch(rustJob, /target\/tests\/trybuild/);
  assert.match(successJob, /^\s*- compiler-contracts\s*$/m);
});

test("compile-contract runner fetches the locked workspace before trybuild goes offline", () => {
  const runner = readFileSync(join(REPO_ROOT, "scripts", "compile-contracts.mjs"), "utf8");
  const fetch = runner.indexOf('["fetch", "--locked"]');
  const firstOwner = runner.indexOf("for (const owner of OWNERS)");

  assert.notEqual(fetch, -1, "the runner must fetch the locked dependency graph");
  assert.notEqual(firstOwner, -1, "the runner must execute every compile-contract owner");
  assert.ok(fetch < firstOwner, "the fetch must finish before trybuild starts any owner");
});

// @ai-generated - Guards workflow prerequisites for generated Svelte conformance artifacts.
test("Svelte conformance runs for golden-generator changes", () => {
  const workflow = readFileSync(join(REPO_ROOT, ".github", "workflows", "ci.yml"), "utf8");
  const conformanceJob = yamlJob(workflow, "svelte-conformance");

  // The gate is composed in detect-changes: the `svelte_oracle` path filter
  // (golden generator, corpus, vendored oracle sources) OR the conformance
  // crate's dependency closure. Both halves are asserted where they live.
  assert.match(
    conformanceJob,
    /if:\s*needs\.detect-changes\.outputs\.svelte_conformance\s*==\s*'true'/,
  );
  assert.deepEqual(LANE_GATES.svelte_conformance, {
    filters: ["svelte_oracle"],
    impact: ["svelte_conformance"],
  });
  assert.match(conformanceJob, /gen-svelte-goldens\.mjs --conformance --check/);
});

// @ai-generated - Guards clean-checkout package entrypoints used by release JavaScript tests.
test("release builds TypeScript package entrypoints before JavaScript tests", () => {
  const workflow = readFileSync(join(REPO_ROOT, ".github", "workflows", "release.yml"), "utf8");
  const testJob = yamlJob(workflow, "test");
  const build = testJob.indexOf("pnpm run build:ts");
  const tests = testJob.indexOf("pnpm test");

  assert.notEqual(build, -1, "release tests must build untracked package dist entrypoints");
  assert.notEqual(tests, -1, "release must execute the JavaScript test suite");
  assert.ok(
    build < tests,
    "TypeScript package entrypoints must exist before JavaScript tests start",
  );
});
