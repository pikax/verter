#!/usr/bin/env node

import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
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
import {
  providerCargoInvocations,
  providerTestTimer,
  runProviderLane,
  verifyArchive,
} from "./provider-ci.mjs";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(SCRIPT_DIR, "..");

test("provider runner stays serial, collects later failures, and keeps timing advisory", async () => {
  for (const outcome of ["pass", "fail", "interrupt"]) {
    let calls = 0;
    let active = 0;
    let output = "";
    let diagnostics = "";
    let stopped = 0;
    const verdict = await runProviderLane("tsserver", {
      spawnCargo(command, args) {
        assert.equal(command, "cargo");
        assert.ok(args.includes("--test-threads=1"));
        assert.ok(args.includes("--color=never"));
        assert.equal(active++, 0, "a provider invocation must finish before the next starts");
        const index = calls++;
        const child = new EventEmitter();
        child.stdout = new EventEmitter();
        setImmediate(() => {
          child.stdout.emit("data", Buffer.from("running 1 test\ntest actual::body ... "));
          child.stdout.emit("data", Buffer.from("ok\ntest result: ok.\n"));
          if (outcome === "interrupt" && index === 0) process.emit("SIGINT");
          active--;
          child.emit("close", outcome === "fail" && index === 0 ? 1 : 0, null);
        });
        return child;
      },
      stopChild: () => {
        stopped++;
      },
      writeStdout: (chunk) => {
        output += chunk;
      },
      writeStderr: (text) => {
        diagnostics += text;
      },
      saveTimings(rows) {
        assert.equal(rows.length, calls);
        assert.ok(rows.every((row) => row.name === "actual::body"));
        throw new Error("read-only timing directory");
      },
    });
    assert.equal(verdict, outcome === "pass" ? 0 : 1);
    assert.equal(calls, outcome === "interrupt" ? 1 : providerCargoInvocations("tsserver").length);
    assert.equal(stopped, outcome === "interrupt" ? 1 : 0);
    assert.match(output, /test actual::body \.\.\. ok/);
    assert.match(diagnostics, /PROVIDER TEST TIME unavailable: read-only timing directory/);
  }
});

test("serial provider timings exclude setup and handle split output and failed tests", () => {
  let now = 0;
  const rows = [];
  const timer = providerTestTimer(
    (row) => rows.push(row),
    () => now,
  );
  timer.push("Compiling a crate\nrunning 3 tests\ntest slow::ro");
  now = 100;
  timer.push("undtrip ... ");
  now = 350;
  timer.push("ok\ntest rejected ... ");
  now = 850;
  timer.push("FAILED\ntest omitted ... ignored\n\nfailures:\n");
  timer.push("test fabricated ... ok\ntest result: FAILED.\n");
  now = 10_000;
  timer.push("running 1 test\ntest src/protocol.rs - decode (line 42) - should panic ... ");
  now = 10_050;
  timer.push("ok\ntest result: ok.\n");
  assert.deepEqual(rows, [
    { name: "slow::roundtrip", status: "ok", elapsedMs: 250 },
    { name: "rejected", status: "FAILED", elapsedMs: 500 },
    { name: "omitted", status: "ignored", elapsedMs: 0 },
    { name: "src/protocol.rs - decode (line 42)", status: "ok", elapsedMs: 50 },
  ]);
  timer.push("running 2 tests\ntest long_running ... ");
  now = 11_000;
  timer.push("test long_running has been running for over 60 seconds\n");
  now = 12_000;
  timer.push("ok\ntest interrupted ... ");
  now = 13_000;
  timer.finish();
  assert.deepEqual(rows.slice(-2), [
    { name: "long_running", status: "ok", elapsedMs: 1950 },
    { name: "interrupted", status: "incomplete", elapsedMs: 1000 },
  ]);
});

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

  assert.match(build, /node scripts\/compile-fixture-artifacts\.mjs archive --workspace/);
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
  assert.match(core, /nextest_argv=\(nextest run --archive-file/);
  assert.match(core, /^\s+cargo "\$\{nextest_argv\[@\]\}"$/m);
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
  // Pull request CI runs tsgo, the main platform. tsserver, the legacy engine,
  // and its plugin run in the scheduled tsserver nightly workflow.
  assert.match(providers, /provider-ci\.mjs run tsgo/);
  assert.match(providers, /VERTER_REQUIRE_TSGO:\s*"1"/);
  assert.doesNotMatch(providers, /VERTER_REQUIRE_TSSERVER/);
  assert.doesNotMatch(
    workflow,
    /provider-ci\.mjs run tsserver|vitest run packages\/typescript-plugin\//,
  );
  assert.doesNotMatch(core, /VERTER_REQUIRE_TS(?:GO|SERVER)/);

  const nightly = readFileSync(
    join(REPO_ROOT, ".github", "workflows", "tsserver-nightly.yml"),
    "utf8",
  );
  assert.match(nightly, /^name: tsserver nightly$/m);
  assert.match(nightly, /^\s+schedule:\s*$/m);
  assert.match(nightly, /^\s+workflow_dispatch:\s*$/m);
  const tsserver = yamlJob(nightly, "providers");
  assert.match(tsserver, /provider-ci\.mjs run tsserver/);
  assert.match(tsserver, /VERTER_REQUIRE_TSSERVER:\s*"1"/);
  assert.doesNotMatch(tsserver, /cargo nextest|install-action@nextest/);
  const plugin = yamlJob(nightly, "plugin");
  assert.match(
    plugin,
    /vitest run packages\/typescript-plugin\/src --exclude ['"]packages\/typescript-plugin\/src\/tsc\/\*\*['"]/,
  );
  // The one job a watcher reads must depend on every lane in the workflow.
  const result = yamlJob(nightly, "result");
  const jobs = nightly.slice(nightly.search(/^jobs:\s*$/m));
  const lanes = [...jobs.matchAll(/^ {2}([a-z0-9-]+):\r?\n/gm)]
    .map((match) => match[1])
    .filter((job) => !["jobs", "result"].includes(job));
  assert.ok(lanes.length >= 5, `expected the nightly lanes, found ${lanes.join(", ")}`);
  for (const job of lanes) {
    assert.match(result, new RegExp(`needs:.*\\b${job}\\b`), `the nightly result must need ${job}`);
  }
  // Timings are written under CARGO_TARGET_DIR (default target/). The upload
  // must use that same root or a relocated target dir drops the artifact.
  assert.match(
    providers,
    /path:\s*\$\{\{\s*format\('\{0\}\/provider-timings\/\*\.json',\s*env\.CARGO_TARGET_DIR\s*\|\|\s*'target'\)\s*\}\}/,
  );
  assert.doesNotMatch(providers, /path:\s*target\/provider-timings\/\*\.json/);
});

test("native-only TypeScript plugin specs run after a native build in the tsserver nightly", () => {
  const nightly = readFileSync(
    join(REPO_ROOT, ".github", "workflows", "tsserver-nightly.yml"),
    "utf8",
  );
  const plugin = yamlJob(nightly, "plugin");
  const build = plugin.indexOf("pnpm run build:native");
  const specs = plugin.indexOf("vitest run packages/typescript-plugin/src/tsc");
  assert.ok(build !== -1 && specs !== -1 && build < specs, "src/tsc specs run after build:native");
  assert.doesNotMatch(plugin, /pnpm --filter @verter\/typescript-plugin test/);

  const pkg = JSON.parse(readFileSync(join(REPO_ROOT, "package.json"), "utf8"));
  assert.match(pkg.scripts["test:scripts"], /provider-ci-lane-selftest\.mjs/);
});

test("nightly coverage instruments every CI lane after its prerequisites", () => {
  const workflow = readFileSync(join(REPO_ROOT, ".github", "workflows", "nightly.yml"), "utf8");
  const coverage = yamlJob(workflow, "rust-coverage-lane");
  for (const [lane, require] of [
    ["core", null],
    ["tsgo", "VERTER_REQUIRE_TSGO"],
    ["tsserver", "VERTER_REQUIRE_TSSERVER"],
  ]) {
    const entry = require
      ? `- lane: ${lane}\\r?\\n\\s+require: ${require}\\r?\\n`
      : `- lane: ${lane}\\r?\\n(?!\\s+require:)`;
    assert.match(coverage, new RegExp(entry), `the ${lane} lane and its provider switch`);
  }
  assert.match(
    coverage,
    /^\s+NEXTEST_PROFILE: ci$/m,
    "the core lane must keep CI's nextest profile",
  );

  const order = [
    ["install the pinned TypeScript toolchain", "pnpm install --frozen-lockfile"],
    [
      "build the tsserver plugin and its runtime dependency",
      "pnpm --filter @verter/language-shared --filter @verter/typescript-plugin build",
    ],
    ["instrument the cargo commands", "source <(cargo llvm-cov show-env --sh)"],
    [
      "prepare the compile-fixture metadata",
      "node scripts/compile-fixture-artifacts.mjs prepare --workspace",
    ],
    [
      "run the core partition under nextest",
      'nextest_argv=(nextest run --workspace --no-fail-fast -E "$(node scripts/provider-ci.mjs filter core)")',
    ],
    ["run the recorded nextest argv", 'cargo "${nextest_argv[@]}" || status=1'],
    ["run the provider lane", 'env "$REQUIRE=1" node scripts/provider-ci.mjs run "$LANE"'],
    ["report the lane", 'cargo llvm-cov report --lcov --output-path "lcov-$LANE.info" || status=1'],
  ].map(([what, text]) => {
    const at = coverage.indexOf(text);
    assert.notEqual(at, -1, `coverage must ${what}`);
    return at;
  });
  assert.deepEqual(
    order,
    [...order].sort((a, b) => a - b),
    "coverage steps must run in order",
  );

  const result = yamlJob(workflow, "rust-coverage");
  assert.match(result, /^\s+name: Rust Coverage$/m);
  assert.match(result, /^\s+needs: rust-coverage-lane$/m);
  assert.match(result, /if: needs\.rust-coverage-lane\.result != 'success'/);
});
