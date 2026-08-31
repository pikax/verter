#!/usr/bin/env node

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import {
  ARCHIVE_FEATURES,
  BF2_AUTHORITATIVE_AGGREGATIONS,
  BF2_AUTHORITATIVE_FEATURE,
  BF2_AUTHORITATIVE_MODULES,
  BF2_HARNESS_SMOKE_MODES,
  CORE_HARNESS_SMOKE_MODES,
  buildBf2NextestArgs,
  countBf2AuthoritativeListTests,
  decideBf2AuthoritativeInventoryMatch,
  scanBf2AuthoritativeSourceInventory,
} from "./gate-internals.mjs";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(SCRIPT_DIR, "..");

function yamlJob(source, name) {
  const start = source.indexOf(`\n  ${name}:`);
  assert.notEqual(start, -1, `workflow must define the ${name} job`);
  const next = source.slice(start + 1).search(/\n  [a-z0-9][a-z0-9-]*:\r?\n/);
  return next === -1 ? source.slice(start) : source.slice(start, start + 1 + next);
}

function yamlPathFilter(source, name) {
  const startToken = `            ${name}:`;
  const start = source.indexOf(startToken);
  assert.notEqual(start, -1, `workflow must define the ${name} path filter`);
  const tail = source.slice(start + startToken.length);
  const next = tail.search(/\r?\n            [a-z][a-z0-9_]*:\r?\n/);
  return next === -1 ? tail : tail.slice(0, next);
}

test("BF2 is absent from the core archive and has exact source-derived nextest coverage", () => {
  assert.deepEqual(ARCHIVE_FEATURES, []);
  assert.deepEqual(CORE_HARNESS_SMOKE_MODES, ["typescript"]);
  assert.deepEqual(BF2_HARNESS_SMOKE_MODES, ["vapor"]);
  assert.equal(BF2_AUTHORITATIVE_FEATURE, "bf2-authoritative");
  assert.deepEqual(BF2_AUTHORITATIVE_MODULES, [
    "bf2_full_axis_gate",
    "bf2_seed_matrix",
    "ide_surface_typescript_observation",
    "nested_v_for_runtime_proof",
    "public_api_typescript_observation",
    "svelte_official_conformance_gate",
    "svelte_official_conformance_matrix",
  ]);

  const source = scanBf2AuthoritativeSourceInventory(REPO_ROOT);
  assert.deepEqual(source.modules, BF2_AUTHORITATIVE_MODULES);
  assert.ok(source.total > 0, "the BF2 source inventory must not be empty");
  assert.deepEqual(source.aggregationsByModule, {
    bf2_full_axis_gate: {
      ownerModule: BF2_AUTHORITATIVE_AGGREGATIONS.bf2_full_axis_gate.ownerModule,
      ownerTest: BF2_AUTHORITATIVE_AGGREGATIONS.bf2_full_axis_gate.ownerTest,
      requiredCalls: [...BF2_AUTHORITATIVE_AGGREGATIONS.bf2_full_axis_gate.requiredCalls],
    },
  });

  const listArgs = buildBf2NextestArgs("list");
  const runArgs = buildBf2NextestArgs("run");
  for (const args of [listArgs, runArgs]) {
    assert.ok(args.includes("--features"));
    assert.equal(args[args.indexOf("--features") + 1], BF2_AUTHORITATIVE_FEATURE);
    assert.ok(args.includes("-E"));
    assert.ok(!args.some((arg) => /threads|jobs/.test(arg)), "BF2 must use full nextest capacity");
  }
  assert.ok(listArgs.includes("--message-format"));
  assert.ok(runArgs.includes("--no-fail-fast"));

  const listed = countBf2AuthoritativeListTests({
    "rust-suites": Object.fromEntries(
      BF2_AUTHORITATIVE_MODULES.map((module) => [
        module,
        {
          testcases: Object.fromEntries(
            Array.from({ length: source.countByModule[module] }, (_, index) => [
              `compile::map_equality_tests::${module}::synthetic_${index}`,
              { "filter-match": { status: "matches" } },
            ]),
          ),
        },
      ]),
    ),
  });
  assert.equal(decideBf2AuthoritativeInventoryMatch(listed, source), null);
  assert.match(
    decideBf2AuthoritativeInventoryMatch({ ...listed, total: listed.total - 1 }, source),
    new RegExp(`selected ${listed.total - 1}.*declares ${listed.total}`),
  );
  assert.match(
    decideBf2AuthoritativeInventoryMatch(listed, { ...source, aggregationsByModule: {} }),
    /aggregation registry drifted/,
  );

  const filteredListing = countBf2AuthoritativeListTests({
    "rust-suites": {
      session: {
        testcases: {
          "compile::map_equality_tests::bf2_seed_matrix::selected": {
            "filter-match": { status: "matches" },
          },
          "unrelated::filtered_out": {
            "filter-match": { status: "mismatch", reason: "expression" },
          },
        },
      },
    },
  });
  assert.equal(filteredListing.total, 1);
  assert.deepEqual(filteredListing.unexpected, []);

  const malformedListing = countBf2AuthoritativeListTests({
    "rust-suites": { session: { testcases: { "missing::filter_metadata": {} } } },
  });
  assert.match(malformedListing.unexpected[0], /invalid filter-match status/);
});

test("ci.yml keeps BF2 parallel, required, pinned/offline, and off the Rust core path", () => {
  const workflow = readFileSync(join(REPO_ROOT, ".github", "workflows", "ci.yml"), "utf8");
  const rustFilter = yamlPathFilter(workflow, "rust");
  const jsFilter = yamlPathFilter(workflow, "js");
  const rustJob = yamlJob(workflow, "rust-test");
  const bf2Job = yamlJob(workflow, "bf2-authoritative");
  const successJob = yamlJob(workflow, "ci-success");

  assert.match(bf2Job, /needs:\s*detect-changes/);
  assert.doesNotMatch(bf2Job, /needs:[^\n]*rust-test/);
  assert.match(bf2Job, /provision-oracle-npm-cache\.mjs/);
  assert.match(bf2Job, /node scripts\/bf2-authoritative\.mjs/);
  assert.match(bf2Job, /NEXTEST_PROFILE:\s*ci/);
  assert.doesNotMatch(bf2Job, /max-threads|test-threads|--jobs|-j\s*\d/);
  assert.match(successJob, /^\s*- bf2-authoritative\s*$/m);
  for (const ownedPath of [
    "scripts/bf2-authoritative.mjs",
    "scripts/bf2-ci-lane-selftest.mjs",
    "packages/types/**",
  ]) {
    assert.match(
      rustFilter,
      new RegExp(ownedPath.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")),
      `the rust filter must route ${ownedPath} through BF2`,
    );
  }
  assert.match(jsFilter, /\.github\/workflows\/release\.yml/);

  assert.doesNotMatch(
    rustJob,
    /provision-oracle-npm-cache\.mjs|--features\s+(?:verter_session\/)?bf2-authoritative/,
  );
  assert.doesNotMatch(
    rustJob,
    /--(?:build-)?jobs\b|-j\s*\d|--test-threads\b|max-threads/,
    "the core Rust CI lane must use the runner's full Cargo and Nextest capacity",
  );
  assert.match(rustJob, /SCCACHE_GHA_ENABLED:\s*"true"/);

  const coreSource = readFileSync(join(REPO_ROOT, "scripts", "gate.mjs"), "utf8");
  const bf2Source = readFileSync(join(REPO_ROOT, "scripts", "bf2-authoritative.mjs"), "utf8");
  assert.doesNotMatch(coreSource, /checkOracleCachePrerequisite|BF2_HARNESS_SMOKE_MODES/);
  assert.match(coreSource, /CORE_HARNESS_SMOKE_MODES/);
  const oracleAt = bf2Source.indexOf("const oracle = checkOracleCachePrerequisite");
  const vaporAt = bf2Source.indexOf("for (const mode of BF2_HARNESS_SMOKE_MODES)");
  const listAt = bf2Source.indexOf('buildBf2NextestArgs("list")');
  assert.ok(
    oracleAt >= 0 && oracleAt < vaporAt && vaporAt < listAt,
    "BF2 must own oracle realization and vapor smoke before inventory listing",
  );
  assert.match(bf2Source, /createGateRunSupervisor\(\{/);
  assert.match(bf2Source, /BF2_LANE_MAX_MS/);
  assert.match(bf2Source, /closeAndReapAll\("BF2_AUTHORITATIVE_TEARDOWN"\)/);
  assert.doesNotMatch(bf2Source, /spawnSync|test-threads|max-threads|--jobs/);
});

test("release.yml runs the same BF2 command in parallel and blocks publishing on it", () => {
  const workflow = readFileSync(join(REPO_ROOT, ".github", "workflows", "release.yml"), "utf8");
  const coreJob = yamlJob(workflow, "test");
  const bf2Job = yamlJob(workflow, "bf2-authoritative");
  assert.doesNotMatch(
    coreJob,
    /provision-oracle-npm-cache\.mjs|--features\s+(?:verter_session\/)?bf2-authoritative/,
  );
  assert.doesNotMatch(bf2Job, /^\s*needs:/m);
  assert.match(bf2Job, /provision-oracle-npm-cache\.mjs/);
  assert.match(bf2Job, /node scripts\/bf2-authoritative\.mjs/);
  assert.match(bf2Job, /NEXTEST_PROFILE:\s*ci/);
  assert.doesNotMatch(bf2Job, /max-threads|test-threads|--jobs|-j\s*\d/);
  for (const requiredJob of ["publish-crates", "publish-npm", "build-vsix", "github-release"]) {
    assert.match(yamlJob(workflow, requiredJob), /\bbf2-authoritative\b/);
  }
});

test("nextest retains timeout overrides without Windows-only serialized groups", () => {
  const config = readFileSync(join(REPO_ROOT, ".config", "nextest.toml"), "utf8");
  assert.doesNotMatch(config, /max-threads\s*=\s*1/);
  assert.doesNotMatch(config, /test-group\s*=\s*'(?:shared-provider-live|lsp-server-unit)'/);
  assert.equal((config.match(/terminate-after\s*=\s*6/g) || []).length, 2);
  assert.equal((config.match(/terminate-after\s*=\s*3/g) || []).length, 6);
});
