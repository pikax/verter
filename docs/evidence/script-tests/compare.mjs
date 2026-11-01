#!/usr/bin/env node
// SCR0-AC1 equivalence evidence: the fragment-driven runner reproduces the
// replaced root `package.json` chains command-for-command, in the same order.
//
// Recorded inputs (frozen at the conversion, before `package.json` was
// edited): baseline-full.txt holds the 48 command lines of the replaced
// `test:scripts` chain, baseline-ci.txt the 25 command lines of the replaced
// `test:scripts:ci` chain.
//
// Method: run the real CLI (`--list` / `--ci --list`) as child processes and
// check, first, that the CLI prints exactly the resolver's view, then diff
// the entries registered by `scripts/test-scripts.d/00-baseline.json` — the
// migrated chain — against the recorded chains. The single excluded entry is
// the runner's own self-test, the one registration SCR0 added next to the
// migrated chain (it is `ci: false`, so the CI comparison has no exclusion);
// fragments added by later nodes do not enter this comparison. The recorded
// diff is empty exactly when every migrated command matches, in order.

import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { resolveScriptTests } from "../../../scripts/run-script-tests.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "../../..");
const RUNNER = path.join(REPO_ROOT, "scripts", "run-script-tests.mjs");
const BASELINE_FRAGMENT = "00-baseline.json";
const SELF_TEST = "scripts/run-script-tests.test.mjs";

function runCli(args) {
  const result = spawnSync(process.execPath, [RUNNER, ...args], {
    cwd: REPO_ROOT,
    encoding: "utf8",
  });
  if (result.status !== 0) {
    throw new Error(`runner failed (${args.join(" ")}): ${result.stderr}`);
  }
  return result.stdout.split("\n").filter((line) => line.length > 0);
}

function diffLines(recorded, resolved, label) {
  const length = Math.max(recorded.length, resolved.length);
  let mismatches = 0;
  for (let i = 0; i < length; i++) {
    if (recorded[i] !== resolved[i]) {
      mismatches++;
      console.log(`${label} line ${i + 1}:`);
      console.log(`  recorded: ${recorded[i] ?? "<missing>"}`);
      console.log(`  resolved: ${resolved[i] ?? "<missing>"}`);
    }
  }
  return mismatches;
}

let failures = 0;
for (const [label, ci, recordedFile] of [
  ["full", false, "baseline-full.txt"],
  ["ci", true, "baseline-ci.txt"],
]) {
  const recorded = readFileSync(path.join(HERE, recordedFile), "utf8")
    .split("\n")
    .filter((line) => line.length > 0);
  const resolvedAll = resolveScriptTests({ ci });
  const cliLines = runCli(ci ? ["--ci", "--list"] : ["--list"]);
  if (cliLines.join("\n") !== resolvedAll.map((e) => e.display).join("\n")) {
    console.log(`${label}: CLI --list output does not match the resolver's view`);
    failures++;
  }
  const migrated = resolvedAll
    .filter(
      (entry) =>
        entry.fragment === BASELINE_FRAGMENT &&
        !(entry.runner !== "pnpm" && entry.file === SELF_TEST),
    )
    .map((entry) => entry.display);
  const mismatches = diffLines(recorded, migrated, label);
  if (mismatches === 0) {
    console.log(
      `SCR0-AC1 ${label}: ${recorded.length}/${recorded.length} recorded commands reproduced command-for-command, in order (diff empty)`,
    );
  } else {
    console.log(`SCR0-AC1 ${label}: ${mismatches} mismatching command line(s) — diff NOT empty`);
    failures++;
  }
}

if (failures > 0) {
  process.exitCode = 1;
  console.log("FAILED");
} else {
  console.log("OK");
}
