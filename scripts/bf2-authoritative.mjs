#!/usr/bin/env node

// Required CI entry point for the feature-gated BF2 authoritative inventory.
// The network boundary stays outside this command: CI first provisions the
// pinned npm cache, then this lane realizes it offline, proves exact source /
// nextest inventory parity, and runs every selected test at normal nextest
// concurrency.

import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  BF2_HARNESS_SMOKE_MODES,
  buildBf2NextestArgs,
  checkOracleCachePrerequisite,
  countBf2AuthoritativeListTests,
  createGateRunSupervisor,
  decideHarnessSmokeResult,
  decideBf2AuthoritativeInventoryMatch,
  formatHarnessSmokeFailure,
  harnessSmokeCommand,
  parseNextestListJson,
  scanBf2AuthoritativeSourceInventory,
} from "./gate-internals.mjs";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(SCRIPT_DIR, "..");
const BF2_LANE_MAX_MS = 90 * 60_000;

function fail(message, code = 127) {
  process.stderr.write(`BF2 AUTHORITATIVE LANE: ${message}\n`);
  return code;
}

async function main() {
  if (process.argv.length !== 2) {
    return fail(
      "this command accepts no arguments; its feature and selector are intentionally fixed",
    );
  }

  const deadlineMs = Date.now() + BF2_LANE_MAX_MS;
  const oracle = checkOracleCachePrerequisite({ repoRoot: REPO_ROOT, env: process.env });
  if (!oracle.ok) {
    for (const line of oracle.lines) process.stderr.write(`${line}\n`);
    return 127;
  }
  process.stderr.write(
    `BF2 authoritative oracle realization satisfied: ${JSON.stringify(oracle.realized)}\n`,
  );

  const runnerTarget = resolve(REPO_ROOT, process.env.CARGO_TARGET_DIR || "target");
  // Safety bounds are orthogonal to capacity: this supervisor adds one
  // absolute wall-clock deadline and process-tree teardown, but passes no
  // build-job or test-thread limit to Cargo/Nextest.
  const supervisor = createGateRunSupervisor({
    deadlineMs,
    stallMs: 0,
    memoryLimitBytes: 0,
    ownershipRoots: [runnerTarget],
  });
  try {
    for (const mode of BF2_HARNESS_SMOKE_MODES) {
      const command = harnessSmokeCommand(REPO_ROOT, mode);
      const smoke = await supervisor.runStep("bf2-authoritative", {
        ...command,
        env: process.env,
        phase: "test",
        targetDir: runnerTarget,
        captureStdoutSeparately: true,
      });
      const decision = decideHarnessSmokeResult(mode, smoke);
      if (!decision.ok) return fail(formatHarnessSmokeFailure(mode, decision));
      process.stderr.write(`BF2 harness smoke [${mode}] satisfied.\n`);
    }

    let sourceInventory;
    try {
      sourceInventory = scanBf2AuthoritativeSourceInventory(REPO_ROOT);
    } catch (error) {
      return fail(`could not scan the feature-gated Rust sources: ${error.message}`);
    }

    const list = await supervisor.runStep("bf2-authoritative", {
      cmd: "cargo",
      args: buildBf2NextestArgs("list"),
      cwd: REPO_ROOT,
      env: process.env,
      phase: "build",
      targetDir: runnerTarget,
      captureStdoutSeparately: true,
    });
    if (list.reason) return fail(`cargo nextest list was aborted: ${list.reason}`);
    if (list.spawnError) return fail(`cargo nextest list could not start: ${list.stderr}`);
    if (list.signalName) return fail(`cargo nextest list was killed by signal ${list.signalName}`);
    if (list.code !== 0)
      return fail(`cargo nextest list failed with exit ${list.code}`, list.code || 1);

    let listJson;
    try {
      listJson = parseNextestListJson(list.stdout || "");
    } catch (error) {
      return fail(`cargo nextest list did not emit valid inventory JSON: ${error.message}`);
    }
    const listedInventory = countBf2AuthoritativeListTests(listJson);
    const inventoryFailure = decideBf2AuthoritativeInventoryMatch(listedInventory, sourceInventory);
    if (inventoryFailure) return fail(inventoryFailure);
    process.stderr.write(
      `BF2 authoritative inventory admitted exactly ${listedInventory.total} source-declared tests.\n`,
    );

    const run = await supervisor.runStep("bf2-authoritative", {
      cmd: "cargo",
      args: buildBf2NextestArgs("run"),
      cwd: REPO_ROOT,
      env: process.env,
      phase: "test",
      targetDir: runnerTarget,
    });
    if (run.reason) return fail(`cargo nextest run was aborted: ${run.reason}`, 1);
    if (run.spawnError) return fail(`cargo nextest run could not start: ${run.stderr}`, 1);
    if (run.signalName) return fail(`cargo nextest run was killed by signal ${run.signalName}`, 1);
    return run.code === 0 ? 0 : run.code || 1;
  } finally {
    await supervisor.closeAndReapAll("BF2_AUTHORITATIVE_TEARDOWN");
  }
}

main().then(
  (code) => {
    process.exitCode = code;
  },
  (error) => {
    process.stderr.write(`BF2 AUTHORITATIVE LANE: ${String(error?.stack ?? error)}\n`);
    process.exitCode = 1;
  },
);
