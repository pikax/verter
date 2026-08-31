#!/usr/bin/env node

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import {
  PROVIDER_CI_LANES,
  PROVIDER_LIVE_SELECTORS,
  buildProviderLaneFilterExpr,
  verifyProviderCiPartition,
} from "./provider-ci-internals.mjs";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(SCRIPT_DIR, "..");

function yamlJob(source, name) {
  const start = source.indexOf(`\n  ${name}:`);
  assert.notEqual(start, -1, `workflow must define the ${name} job`);
  const next = source.slice(start + 1).search(/\n  [a-z0-9][a-z0-9-]*:\r?\n/);
  return next === -1 ? source.slice(start) : source.slice(start, start + 1 + next);
}

function syntheticInventory() {
  const suites = {};
  for (const selector of PROVIDER_LIVE_SELECTORS) {
    const suite = (suites[selector.package] ||= {
      "package-name": selector.package,
      testcases: {},
    });
    if (selector.kind === "exact") {
      for (const name of selector.values) suite.testcases[name] = {};
    } else {
      suite.testcases[selector.example] = {};
    }
  }
  suites.verter_core_fixture = {
    "package-name": "verter_core_fixture",
    testcases: { "unit::provider_free_control": {} },
  };
  return { "rust-suites": suites };
}

test("provider filters form one non-empty, disjoint canonical partition", () => {
  assert.deepEqual(PROVIDER_CI_LANES, ["core", "tsserver", "tsgo"]);
  for (const lane of PROVIDER_CI_LANES) {
    const filter = buildProviderLaneFilterExpr(lane);
    assert.match(filter, /not package\(verter_shipped_cfg_contract\)/);
    assert.doesNotMatch(filter, /test-threads|max-threads|--jobs/);
  }
  const verdict = verifyProviderCiPartition(syntheticInventory());
  assert.equal(verdict.ok, true, verdict.errors.join("\n"));
  assert.ok(verdict.counts.core > 0);
  assert.ok(verdict.counts.tsserver > 0);
  assert.ok(verdict.counts.tsgo > 0);

  const missingExact = syntheticInventory();
  const exact = PROVIDER_LIVE_SELECTORS.find((selector) => selector.kind === "exact");
  delete missingExact["rust-suites"][exact.package].testcases[exact.values[0]];
  const missingVerdict = verifyProviderCiPartition(missingExact);
  assert.equal(missingVerdict.ok, false);
  assert.match(missingVerdict.errors.join("\n"), /exact provider test .* matched 0 times/);
});

test("CI builds one archive and fans out four core shards plus provider consumers", () => {
  const workflow = readFileSync(join(REPO_ROOT, ".github", "workflows", "ci.yml"), "utf8");
  const build = yamlJob(workflow, "rust-test-build");
  const core = yamlJob(workflow, "rust-test");
  const tsserver = yamlJob(workflow, "rust-tsserver-live");
  const tsgo = yamlJob(workflow, "rust-tsgo-live");
  const success = yamlJob(workflow, "ci-success");

  assert.match(build, /cargo nextest archive --workspace/);
  assert.match(build, /node scripts\/provider-ci\.mjs verify --archive-file/);
  assert.match(build, /name:\s*rust-nextest-archive/);
  assert.doesNotMatch(build, /--(?:build-)?jobs\b|-j\s*\d|--test-threads\b|max-threads/);
  assert.match(success, /^\s*- rust-test-build\s*$/m);
  assert.match(core, /strategy:\s*\n\s+fail-fast:\s*false/);
  assert.match(core, /shard:\s*\[1, 2, 3, 4\]/);
  assert.match(core, /name:\s*Rust Test \(Core \$\{\{ matrix\.shard \}\}\/4\)/);
  assert.match(core, /--partition "hash:\$\{\{ matrix\.shard \}\}\/4"/);
  assert.match(core, /name:\s*Rust Core Test Results \(\$\{\{ matrix\.shard \}\}\/4\)/);
  for (const [lane, job] of [
    ["core", core],
    ["tsserver", tsserver],
    ["tsgo", tsgo],
  ]) {
    assert.match(job, /needs:\s*\[detect-changes, rust-test-build\]/);
    assert.match(job, /name:\s*rust-nextest-archive/);
    assert.match(job, new RegExp(`provider-ci\\.mjs filter ${lane}`));
    assert.match(job, /mkdir -p target\/rust-nextest-archive/);
    assert.match(job, /cargo nextest run --archive-file/);
    assert.doesNotMatch(job, /cargo (?:build|test|check|nextest archive)/);
    assert.doesNotMatch(job, /--(?:build-)?jobs\b|-j\s*\d|--test-threads\b|max-threads/);
    assert.match(
      success,
      new RegExp(`^\\s*- ${lane === "core" ? "rust-test" : `rust-${lane}-live`}\\s*$`, "m"),
    );
  }
  assert.match(tsserver, /VERTER_REQUIRE_TSSERVER:\s*"1"/);
  assert.match(tsserver, /packages\/typescript-plugin\/src/);
  assert.match(tsserver, /--exclude ['"]packages\/typescript-plugin\/src\/tsc\/\*\*['"]/);
  assert.match(tsgo, /VERTER_REQUIRE_TSGO:\s*"1"/);
  assert.doesNotMatch(core, /VERTER_REQUIRE_TS(?:GO|SERVER)/);
});

test("native-only TypeScript plugin specs remain with the native artifact", () => {
  const workflow = readFileSync(join(REPO_ROOT, ".github", "workflows", "ci.yml"), "utf8");
  const native = yamlJob(workflow, "native-test");
  assert.match(native, /vitest run packages\/typescript-plugin\/src\/tsc/);
  assert.doesNotMatch(native, /pnpm --filter @verter\/typescript-plugin test/);

  const pkg = JSON.parse(readFileSync(join(REPO_ROOT, "package.json"), "utf8"));
  assert.match(pkg.scripts["test:scripts"], /provider-ci-lane-selftest\.mjs/);
});
