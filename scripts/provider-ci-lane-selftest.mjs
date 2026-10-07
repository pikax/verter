#!/usr/bin/env node

import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import test from "node:test";

import {
  PROVIDER_CI_LANES,
  PROVIDER_LIVE_SELECTORS,
  buildProviderLaneFilterExpr,
  verifyProviderCiPartition,
} from "./provider-ci-internals.mjs";
import { providerCargoInvocations, verifyArchive } from "./provider-ci.mjs";

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

function isAlive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return error.code === "EPERM";
  }
}

// A test binary that never returns from `--list` must fail the verify step
// within its bound, name the condition, and leave no descendant running.
test("verify fails at its listing bound and reaps a listing that never returns", async () => {
  const dir = mkdtempSync(join(tmpdir(), "provider-ci-list-"));
  const pidFile = join(dir, "listing.pids");
  const neverExits = "setInterval(() => {}, 1 << 30);";
  // The grandchild is detached so that it outlives its parent unless the
  // reaper itself ends it: it leaves the parent's job object on Windows and
  // its process group on POSIX.
  const fakeCargo =
    `const { spawn } = require("node:child_process");` +
    `const child = spawn(process.execPath, ["-e", ${JSON.stringify(neverExits)}], ` +
    `{ detached: true, stdio: "ignore" });` +
    `require("node:fs").writeFileSync(${JSON.stringify(pidFile)}, process.pid + " " + child.pid);` +
    neverExits;
  const listingPids = () => readFileSync(pidFile, "utf8").split(" ").map(Number);
  const reapListing = () => {
    for (const pid of listingPids()) if (isAlive(pid)) process.kill(pid, "SIGKILL");
  };
  let output = "";
  const backstop = new AbortController();
  try {
    const listing = verifyArchive(["--archive-file", join(dir, "archive.tar.zst")], {
      cargo: process.execPath,
      cargoArgsPrefix: ["-e", fakeCargo],
      listTimeoutMs: 3000,
      write: (text) => {
        output += text;
      },
    });
    // Without the bound the listing never settles; end it here so the
    // failure is reported instead of holding the test process open.
    const outcome = await Promise.race([
      listing,
      sleep(120_000, "unbounded", { signal: backstop.signal }).catch(() => "aborted"),
    ]);
    if (outcome === "unbounded") {
      reapListing();
      await listing;
      assert.fail("verify did not end at its listing bound");
    }
    assert.equal(outcome, 124);
    assert.match(output, /did not return from --list within 3s/);
    const [, grandchild] = listingPids();
    if (process.platform === "linux") assert.match(output, new RegExp(`\\b${grandchild}\\b`));
    const deadline = Date.now() + 10_000;
    while (isAlive(grandchild) && Date.now() < deadline) await sleep(100);
    assert.equal(isAlive(grandchild), false, "the listing's descendants must be reaped");
  } finally {
    backstop.abort();
    try {
      reapListing();
    } catch {}
    rmSync(dir, { recursive: true, force: true });
  }
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

test("CI builds one provider-free archive and runs providers serially in their own lane", () => {
  const workflow = readFileSync(join(REPO_ROOT, ".github", "workflows", "ci.yml"), "utf8");
  const build = yamlJob(workflow, "rust-test-build");
  const core = yamlJob(workflow, "rust-test");
  const providers = yamlJob(workflow, "rust-providers-live");
  const success = yamlJob(workflow, "ci-success");

  assert.match(build, /cargo nextest archive --workspace/);
  assert.match(build, /node scripts\/provider-ci\.mjs verify --archive-file/);
  // Each step that lists the archive carries its own bound below the job's.
  assert.match(
    build,
    /- name: Verify the provider CI partition\r?\n\s+timeout-minutes: \d+\r?\n\s+run: node scripts\/provider-ci\.mjs verify/,
  );
  assert.match(
    core,
    /- name: Run core Rust tests from the shared archive\r?\n\s+timeout-minutes: \d+\r?\n/,
  );
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
  // One job compiles the workspace once and runs each engine's serial libtest
  // lane in turn; neither engine's run goes through the nextest archive.
  assert.match(providers, /needs:\s*detect-changes/);
  assert.match(providers, /needs\.detect-changes\.outputs\.providers\s*==\s*'true'/);
  assert.match(providers, /Swatinem\/rust-cache/);
  assert.doesNotMatch(
    providers,
    /cargo nextest|install-action@nextest|name:\s*rust-nextest-archive|download-artifact/,
  );
  assert.match(success, /^\s*- rust-providers-live\s*$/m);
  for (const lane of ["tsserver", "tsgo"]) {
    assert.match(providers, new RegExp(`provider-ci\\.mjs run ${lane}`));
  }
  assert.match(providers, /VERTER_REQUIRE_TSSERVER:\s*"1"/);
  assert.match(providers, /packages\/typescript-plugin\/src/);
  assert.match(providers, /--exclude ['"]packages\/typescript-plugin\/src\/tsc\/\*\*['"]/);
  assert.match(providers, /VERTER_REQUIRE_TSGO:\s*"1"/);
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
