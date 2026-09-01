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
import { providerCargoInvocations } from "./provider-ci.mjs";

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

// @ai-generated - Ensures real-provider modules cannot fall through to the provider-free core lane.
test("real-provider module tests require explicit provider ownership", () => {
  const inventory = syntheticInventory();
  inventory["rust-suites"].verter_lsp.testcases[
    "real_provider_tests::rename::unsuffixed_provider_test"
  ] = {};

  const verdict = verifyProviderCiPartition(inventory);
  assert.equal(verdict.ok, false);
  assert.match(
    verdict.errors.join("\n"),
    /real-provider test .* has no explicit tsserver or tsgo selector/,
  );
});

test("provider runners use serial libtest commands instead of nextest", () => {
  for (const lane of ["tsserver", "tsgo"]) {
    const invocations = providerCargoInvocations(lane);
    assert.ok(invocations.length > 0);
    for (const invocation of invocations) {
      assert.equal(invocation.args[0], "test");
      assert.ok(invocation.args.includes("--locked"));
      assert.ok(invocation.args.includes("--no-fail-fast"));
      assert.ok(invocation.args.includes("--test-threads=1"));
      assert.ok(invocation.args.includes("-p"));
      assert.doesNotMatch(invocation.args.join(" "), /nextest|--(?:build-)?jobs\b|-j\s*\d/);
    }
  }
});

test("CI builds one provider-free archive and runs providers independently", () => {
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
  assert.match(core, /needs:\s*\[detect-changes, rust-test-build\]/);
  assert.match(core, /name:\s*rust-nextest-archive/);
  assert.match(core, /provider-ci\.mjs filter core/);
  assert.match(core, /cargo nextest run --archive-file/);
  assert.match(success, /^\s*- rust-test\s*$/m);
  for (const [lane, job] of [
    ["tsserver", tsserver],
    ["tsgo", tsgo],
  ]) {
    assert.match(job, /needs:\s*detect-changes/);
    assert.match(job, /Swatinem\/rust-cache/);
    assert.match(job, new RegExp(`provider-ci\\.mjs run ${lane}`));
    assert.doesNotMatch(
      job,
      /cargo nextest|install-action@nextest|name:\s*rust-nextest-archive|actions\/download-artifact/,
    );
    assert.match(success, new RegExp(`^\\s*- rust-${lane}-live\\s*$`, "m"));
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

test("nightly coverage builds real-tsserver prerequisites before workspace tests", () => {
  const workflow = readFileSync(join(REPO_ROOT, ".github", "workflows", "nightly.yml"), "utf8");
  const coverage = yamlJob(workflow, "rust-coverage");
  const install = coverage.indexOf("pnpm install --frozen-lockfile");
  const build = coverage.indexOf(
    "pnpm --filter @verter/language-shared --filter @verter/typescript-plugin build",
  );
  const testRun = coverage.indexOf("cargo llvm-cov --workspace");

  assert.notEqual(install, -1, "coverage must install the pinned TypeScript toolchain");
  assert.notEqual(build, -1, "coverage must build the tsserver plugin and its runtime dependency");
  assert.notEqual(testRun, -1, "coverage must execute the workspace test universe");
  assert.ok(
    install < build && build < testRun,
    "coverage prerequisites must be built before tests",
  );
});
