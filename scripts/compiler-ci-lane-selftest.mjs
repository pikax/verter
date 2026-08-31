#!/usr/bin/env node

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(SCRIPT_DIR, "..");

function yamlJob(source, name) {
  const start = source.indexOf(`\n  ${name}:`);
  assert.notEqual(start, -1, `workflow must define the ${name} job`);
  const next = source.slice(start + 1).search(/\n  [a-z0-9][a-z0-9-]*:\r?\n/);
  return next === -1 ? source.slice(start) : source.slice(start, start + 1 + next);
}

test("compiler trybuild cache belongs to the required job that executes the grouped driver", () => {
  const workflow = readFileSync(join(REPO_ROOT, ".github", "workflows", "ci.yml"), "utf8");
  const rustJob = yamlJob(workflow, "rust-test");
  const compilerJob = yamlJob(workflow, "compiler-contracts");
  const successJob = yamlJob(workflow, "ci-success");

  assert.match(compilerJob, /needs:\s*detect-changes/);
  assert.match(compilerJob, /needs\.detect-changes\.outputs\.rust\s*==\s*'true'/);
  assert.match(compilerJob, /target\/tests\/trybuild/);
  assert.match(compilerJob, /default_compiler_compile_fail_contracts_are_enforced/);
  assert.match(compilerJob, /generated_svelte_artifacts_match_their_authoritative_inputs/);
  assert.doesNotMatch(compilerJob, /max-threads|test-threads|--jobs|-j\s*\d/);
  assert.doesNotMatch(rustJob, /target\/tests\/trybuild/);
  assert.match(successJob, /^\s*- compiler-contracts\s*$/m);
});
