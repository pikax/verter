#!/usr/bin/env node

// Tests for check-ci-aggregate.mjs. Run: node --test scripts/check-ci-aggregate.test.mjs
//
// analyzeCiAggregate is pure and tested against synthetic workflow fixtures
// covering complete aggregation, missing/unknown needs, missing always(), a
// wrong display name, and both inline-array and block-list needs forms. One
// test loads the real `.github/workflows/ci.yml`.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { analyzeCiAggregate } from "./check-ci-aggregate.mjs";

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), "..");

function workflow({
  aggregateName = "CI Required",
  always = true,
  alwaysForm = "always()",
  needs,
  extraJobs = [],
}) {
  const jobs = ["detect-changes", "rust-fmt", ...extraJobs];
  const jobBlocks = jobs
    .map(
      (id) => `  ${id}:
    name: ${id}
    runs-on: ubuntu-latest
    steps:
      - name: work
        run: echo ok`,
    )
    .join("\n");
  const ifLine = always ? `    if: ${alwaysForm}\n` : "";
  return `name: CI
on:
  push:
    branches: main
permissions:
  contents: read
jobs:
${jobBlocks}
  ci-success:
    name: ${aggregateName}
${ifLine}    runs-on: ubuntu-latest
    needs: ${needs}
    steps:
      - name: Verify no dependency failed
        run: echo ok
`;
}

test("complete aggregation (block-list needs) passes", () => {
  const yaml = workflow({
    needs: `
      - detect-changes
      - rust-fmt`,
  });
  const result = analyzeCiAggregate(yaml);
  assert.deepEqual(result.jobIds, ["detect-changes", "rust-fmt", "ci-success"]);
  assert.equal(result.aggregateName, "CI Required");
  assert.equal(result.hasAlways, true);
  assert.deepEqual(result.needs, ["detect-changes", "rust-fmt"]);
  assert.deepEqual(result.missing, []);
  assert.deepEqual(result.unknownNeeds, []);
});

test("a job missing from needs is reported", () => {
  const yaml = workflow({
    needs: `
      - detect-changes`,
  });
  const result = analyzeCiAggregate(yaml);
  assert.deepEqual(result.missing, ["rust-fmt"]);
  assert.deepEqual(result.unknownNeeds, []);
});

test("a needs entry naming a nonexistent job is reported", () => {
  const yaml = workflow({
    needs: `
      - detect-changes
      - rust-fmt
      - ghost-job`,
  });
  const result = analyzeCiAggregate(yaml);
  assert.deepEqual(result.missing, []);
  assert.deepEqual(result.unknownNeeds, ["ghost-job"]);
});

test("aggregator without if: always() is reported", () => {
  const yaml = workflow({
    always: false,
    needs: `
      - detect-changes
      - rust-fmt`,
  });
  const result = analyzeCiAggregate(yaml);
  assert.equal(result.hasAlways, false);
  assert.deepEqual(result.missing, []);
  assert.deepEqual(result.unknownNeeds, []);
});

test("aggregator with the wrong display name is reported", () => {
  const yaml = workflow({
    aggregateName: "CI Success",
    needs: `
      - detect-changes
      - rust-fmt`,
  });
  const result = analyzeCiAggregate(yaml);
  assert.equal(result.aggregateName, "CI Success");
  assert.equal(result.hasAlways, true);
  assert.deepEqual(result.missing, []);
});

test("inline-array needs form is parsed", () => {
  const yaml = workflow({
    alwaysForm: "${{ always() }}",
    needs: "[detect-changes, rust-fmt] # all jobs",
  });
  const result = analyzeCiAggregate(yaml);
  assert.deepEqual(result.needs, ["detect-changes", "rust-fmt"]);
  assert.equal(result.hasAlways, true);
  assert.deepEqual(result.missing, []);
  assert.deepEqual(result.unknownNeeds, []);
});

test("block-list needs form strips comments", () => {
  const yaml = workflow({
    extraJobs: ["wasm-build"],
    needs: `
      - detect-changes  # first
      - rust-fmt
      - wasm-build`,
  });
  const result = analyzeCiAggregate(yaml);
  assert.deepEqual(result.jobIds, ["detect-changes", "rust-fmt", "wasm-build", "ci-success"]);
  assert.deepEqual(result.needs, ["detect-changes", "rust-fmt", "wasm-build"]);
  assert.deepEqual(result.missing, []);
  assert.deepEqual(result.unknownNeeds, []);
});

test("real ci.yml aggregator is complete", () => {
  const yaml = readFileSync(join(repoRoot, ".github", "workflows", "ci.yml"), "utf8");
  const result = analyzeCiAggregate(yaml);
  assert.deepEqual(result.missing, []);
  assert.deepEqual(result.unknownNeeds, []);
  assert.equal(result.hasAlways, true);
  assert.equal(result.aggregateName, "CI Required");
  assert.ok(result.jobIds.length >= 25, `expected at least 25 jobs, got ${result.jobIds.length}`);
});
