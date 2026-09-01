#!/usr/bin/env node
// ----------------------------------------------------------------------------------------------------
// wasm-js-boundary-lane.mjs — the workspace's `#[wasm_bindgen_test]` cases, run on
// `wasm32-unknown-unknown` through `wasm-bindgen-test-runner`.
//
// WHY A STANDALONE ENTRY. Those cases exist because a deserializer can visit only the fields a schema
// declares, so a closed-shape refusal must be proven where a browser caller meets it — against a real JS
// object graph, on the wasm target. They are `#[cfg(target_arch = "wasm32")]`, so no host-target run can
// contain them: the shared nextest archive CI builds cannot execute a single one.
//
// The local gate owns this lane inside `gate.mjs`. CI does not run `gate.mjs` — it builds one archive and
// fans out — so without this entry the lane exists, is documented, and never fires in CI. That is the exact
// defect class the lane was written to catch, so it gets its own invocation rather than an assumption.
//
// It shares `gate-internals.mjs`'s prerequisite probe, argument builder and transcript evaluator with the
// gate, so the two cannot drift on what "the lane passed" means. What it does NOT share is the gate's
// deadline/stall/RSS containment: a CI job supplies its own timeout, and reimplementing that here would be
// a second containment model to keep in step.
//
// Exit codes: 0 pass, 1 a real lane failure, 127 a missing prerequisite (target, runner, version skew).
// ----------------------------------------------------------------------------------------------------

import { spawnSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  buildWasmLaneTestArgs,
  checkWasmLanePrerequisites,
  evaluateWasmLanePackageRun,
  WASM_LANE_RUNNER_ENV_KEY,
  WASM_LANE_TARGET,
} from "./gate-internals.mjs";

const EXIT_PASS = 0;
const EXIT_FAIL = 1;
const EXIT_SETUP = 127;

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const exhaustive = process.argv.includes("--exhaustive");

const log = (line) => process.stdout.write(`[wasm-lane] ${line}\n`);
const err = (line) => process.stderr.write(`[wasm-lane] ${line}\n`);

const prereq = checkWasmLanePrerequisites({ repoRoot, env: process.env });
if (!prereq.ok) {
  for (const line of prereq.lines) err(line);
  process.exit(EXIT_SETUP);
}
log(
  `${prereq.packages.length} package(s) ` +
    `(${prereq.packages.map((pkg) => `${pkg.name}=${prereq.perPackage[pkg.name]}`).join(", ")}), ` +
    `${prereq.discoveredCases} discovered case(s); runner ${prereq.runnerPath} pinned at ` +
    `${prereq.expectedVersion} by this tree's wasm-bindgen dependency`,
);

const laneEnv = { ...process.env, [WASM_LANE_RUNNER_ENV_KEY]: prereq.runnerPath };
const failures = [];
let executedCases = 0;
let incomplete = false;

for (const pkg of prereq.packages) {
  const expected = prereq.perPackage[pkg.name];
  const args = buildWasmLaneTestArgs({ packageName: pkg.name, exhaustive });
  log(`cargo ${args.join(" ")}`);
  const run = spawnSync("cargo", args, {
    cwd: repoRoot,
    env: laneEnv,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
    windowsHide: true,
  });
  if (run.error) {
    err(`could not launch cargo for ${pkg.name}: ${run.error.message}`);
    process.exit(EXIT_SETUP);
  }
  const text = `${run.stdout ?? ""}${run.stderr ?? ""}`;
  process.stdout.write(text);

  const verdict = evaluateWasmLanePackageRun({
    packageName: pkg.name,
    expected,
    text,
    exitCode: run.status ?? 1,
  });
  executedCases += verdict.summary.passed + verdict.summary.failed;
  // A transcript that announced work and never closed it is a run that did not
  // finish — never a pass, and distinct from a run whose cases failed.
  if (!verdict.parseable || !verdict.complete) incomplete = true;
  failures.push(...verdict.failures);
  if (failures.length > 0 && !exhaustive) break;
}

if (incomplete) {
  failures.push({
    surface: "wasm:lane",
    name: "<a package's harness transcript did not finish — the lane did not complete>",
  });
}

if (failures.length > 0) {
  err(`VERDICT: FAIL — ${failures.length} failure(s) on ${WASM_LANE_TARGET}:`);
  for (const failure of failures.slice(0, 50)) err(`  [${failure.surface}] ${failure.name}`);
  process.exit(EXIT_FAIL);
}

log(
  `VERDICT: PASS — executed ${executedCases}/${prereq.discoveredCases} discovered case(s) on ${WASM_LANE_TARGET}`,
);
process.exit(EXIT_PASS);
