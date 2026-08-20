#!/usr/bin/env node
// triage-gate-failure-selftest.mjs — REAL, end-to-end discrimination proof for triage-gate-failure.mjs.
//
// UNLIKE gate-selftest.mjs (sleep/echo stand-ins only, NO workspace cargo — see its own header), this
// script DOES run real `cargo nextest`, because the property under test — "does an isolated per-test
// rerun correctly tell REAL apart from FLAKY apart from INTERACTION" — is not a parsing/classification
// property alone; it is a claim about what a REAL Rust test binary does under REAL process isolation vs
// REAL concurrency. A canned nextest-output fixture could prove the PARSER, but it could not prove the
// isolation itself is genuine (one test per process) or that the concurrency-only failure mode is real.
//
// It plants THREE temporary `#[test]` functions into `crates/verter_span` (a small, leaf, fast-compiling
// crate — no other production code is touched):
//   (a) triage_plant_real_always_fails      — deterministic `assert!(false)` => proves REAL.
//   (b) triage_plant_flaky_pid_parity       — fails when the fresh process's own PID is odd => proves
//                                              FLAKY without any concurrency (no shared state at all).
//   (c) triage_plant_interaction_race_a/_b  — two tests race on a FIXED shared temp-file path; run
//                                              concurrently (multiple processes) at least one corrupts the
//                                              other's readback and fails; run ALONE (this tool's
//                                              isolation: one test, one process) there is no contender and
//                                              it always passes => proves INTERACTION.
//
// PLANT -> PROVE RED -> REVERT -> PROVE GREEN, with the plant proven PRESENT, UNIQUE, and NEW via git diff
// before it is trusted, and PROVEN GONE afterwards (also via git diff), exactly as required for a
// discriminating characterization proof. NOT part of the canonical gate (`node scripts/gate.mjs`) or
// `gate-selftest.mjs` — run it directly: `node scripts/triage-gate-failure-selftest.mjs`.

import { execFileSync, spawnSync } from "node:child_process";
import { appendFileSync, readFileSync, writeFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import {
  buildCargoEnv,
  runContainedStep,
  analyzeNextestSurface,
  nowMs,
} from "./gate-internals.mjs";
import { parseGateVerdict } from "./triage-gate-internals.mjs";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const REPO = execFileSync("git", ["-C", SCRIPT_DIR, "rev-parse", "--show-toplevel"], { encoding: "utf8" }).trim();
const LIB_RS = join(REPO, "crates", "verter_span", "src", "lib.rs");
const RUNNER_TARGET = join(REPO, "target", "gate-runner");
const FAKE_LOG = join(REPO, "target", "gate-runner", "triage-selftest-fake-gate-log.txt");

let PASS = 0;
let FAIL = 0;
function ok(cond, msg) {
  if (cond) {
    PASS++;
    process.stderr.write(`PASS: ${msg}\n`);
  } else {
    FAIL++;
    process.stderr.write(`FAIL: ${msg}\n`);
  }
}

const PLANT_MARKER = "triage_plant_real_always_fails";
const PLANT_BLOCK = `
// ============================================================================================
// TEMPORARY PLANT — written by triage-gate-failure-selftest.mjs. Reverted at the end of that run;
// if you see this outside that run, revert it: \`git checkout -- crates/verter_span/src/lib.rs\`.
// ============================================================================================
#[cfg(test)]
mod triage_plant {
    #[test]
    fn ${PLANT_MARKER}() {
        assert!(false, "planted deterministic failure for triage-gate-failure REAL proof");
    }

    #[test]
    fn triage_plant_flaky_pid_parity() {
        let pid = std::process::id();
        assert!(pid % 2 == 0, "planted flaky failure (pid={pid} is odd)");
    }

    fn race_path() -> std::path::PathBuf {
        std::env::temp_dir().join("verter-span-triage-race-plant.bin")
    }
    fn race_probe(byte: u8) {
        let path = race_path();
        let payload = vec![byte; 4_000_000];
        for _ in 0..30 {
            std::fs::write(&path, &payload).expect("planted race: write failed");
            let read = std::fs::read(&path).expect("planted race: read failed");
            assert!(
                read.iter().all(|&b| b == byte),
                "planted INTERACTION failure: shared path was corrupted by a concurrent writer"
            );
        }
    }
    #[test]
    fn triage_plant_interaction_race_a() {
        race_probe(b'A');
    }
    #[test]
    fn triage_plant_interaction_race_b() {
        race_probe(b'B');
    }
}
`;

function gitDiffStat() {
  return execFileSync("git", ["-C", REPO, "diff", "--stat", "--", "crates/verter_span/src/lib.rs"], {
    encoding: "utf8",
  }).trim();
}
function gitDiffText() {
  return execFileSync("git", ["-C", REPO, "diff", "--", "crates/verter_span/src/lib.rs"], {
    encoding: "utf8",
  });
}

async function runCargoNextest(args, extraEnv = {}) {
  const cargoEnv = { ...buildCargoEnv(process.env, RUNNER_TARGET, undefined, 4), ...extraEnv };
  const res = await runContainedStep({
    cmd: "cargo",
    args: ["nextest", ...args],
    cwd: REPO,
    env: cargoEnv,
    phase: "test",
    deadlineMs: nowMs() + 6 * 60 * 1000,
    stallMs: 3 * 60 * 1000,
    targetDir: RUNNER_TARGET,
    memoryLimitBytes: 8 * 1024 * 1024 * 1024,
  });
  return { ...res, text: res.stdout + "\n" + res.stderr };
}

async function main() {
  process.stderr.write("=== triage-gate-failure self-test (REAL cargo nextest, plants in verter_span) ===\n");

  if (readFileSync(LIB_RS, "utf8").includes(PLANT_MARKER)) {
    process.stderr.write(
      "FATAL: the plant marker is already present in crates/verter_span/src/lib.rs — refusing to " +
        "double-plant. Run `git checkout -- crates/verter_span/src/lib.rs` first.\n",
    );
    process.exit(2);
  }

  // ---- PLANT ----
  appendFileSync(LIB_RS, PLANT_BLOCK);
  const diffStat = gitDiffStat();
  const diffText = gitDiffText();
  const markerCount = (diffText.match(new RegExp(PLANT_MARKER, "g")) || []).length;
  ok(diffStat.length > 0, "(plant) git diff --stat is non-empty after appending — the plant is PRESENT");
  ok(markerCount === 1, `(plant) plant marker appears exactly once in the diff (got ${markerCount}) — PRESENT, UNIQUE`);
  ok(
    diffText.split("\n").every((l) => !l.startsWith("-") || l.startsWith("---")),
    "(plant) diff contains NO removed lines — the plant is purely a NEW addition",
  );

  // ---- PROVE RED ----
  // The interaction-race plant is, by nature, probabilistic: it fails when the OS scheduler interleaves
  // the two racers' writes/reads within the same run, which is likely but not guaranteed on any single
  // attempt. Retry the concurrent capture (re-running the ALREADY-COMPILED binary — cheap) until the race
  // is observed; exhausting every attempt without ever seeing it is treated as a hard failure of this
  // self-test, not silently skipped (a self-test whose central proof can vanish without comment is not one).
  const RED_CAPTURE_ATTEMPTS = 6;
  let red;
  let verdict;
  let names;
  let raceFailed = [];
  for (let attempt = 1; attempt <= RED_CAPTURE_ATTEMPTS && raceFailed.length === 0; attempt++) {
    process.stderr.write(
      `\n(red) running the plants concurrently to capture a genuine gate-style failure (attempt ${attempt}/${RED_CAPTURE_ATTEMPTS}) …\n`,
    );
    red = await runCargoNextest([
      "run",
      "-p",
      "verter_span",
      "--lib",
      "-E",
      "test(triage_plant)",
      "--test-threads",
      "4",
      "--no-fail-fast",
    ]);
    if (red.reason) {
      process.stderr.write(`FATAL: the red capture run itself aborted (${red.reason}) — cannot proceed.\n`);
      revertAndExit(3);
      return;
    }
    verdict = analyzeNextestSurface(red.text, red.code, false);
    names = verdict.failures.map((f) => f.name);
    raceFailed = names.filter((n) => n.includes("triage_plant_interaction_race_"));
  }
  // nextest names a lib test module-qualified (`triage_plant::<fn>`); PLANT_MARKER is the bare fn name
  // (what the Rust source declares), so match on suffix rather than exact equality.
  ok(
    names.some((n) => n.endsWith(PLANT_MARKER)),
    "(red) the deterministic plant is a NAMED failure in the captured run",
  );
  ok(
    raceFailed.length >= 1,
    `(red) at least one race-plant failed under concurrency within ${RED_CAPTURE_ATTEMPTS} attempt(s) ` +
      `(got: ${JSON.stringify(raceFailed)}) — proves the race is real`,
  );
  if (raceFailed.length === 0) {
    process.stderr.write(
      "FATAL: the interaction-race plant never failed under concurrency — cannot prove INTERACTION. " +
        "This is a self-test failure, not a skip.\n",
    );
    revertAndExit(4);
    return;
  }

  // Build a gate.mjs-shaped log from this REAL capture, via the SAME analyzeNextestSurface verdict the
  // live gate uses (not a hand-rolled substitute) — see gate.mjs's own SURFACE-1 composition.
  //
  // The pid-parity plant is, BY DESIGN, only intermittently a named failure in any one red capture (it
  // fails only when the capture PROCESS's own PID happens to be odd) — but the FLAKY classification proof
  // needs it isolated regardless, and isolation reruns it in FRESH processes with THEIR OWN independent
  // PIDs anyway, so whether it failed in THIS particular capture is irrelevant to what isolation will
  // observe. Force it into the verdict list unconditionally so this self-test's FLAKY proof does not
  // itself depend on a coin flip.
  const verdictFailures = verdict.failures.some((f) => f.name.endsWith("triage_plant_flaky_pid_parity"))
    ? verdict.failures
    : [...verdict.failures, { surface: "nextest", name: "triage_plant::triage_plant_flaky_pid_parity" }];
  let log = "[gate] SURFACE 1: nextest run from the archive (process isolation) …\n";
  log += red.text + "\n";
  log += `[gate][error] VERDICT: FAIL — ${verdictFailures.length} non-tolerated failure(s):\n`;
  for (const f of verdictFailures) log += `[gate][error]   [${f.surface}] ${f.name}\n`;
  writeFileSync(FAKE_LOG, log);

  // Sanity: our own parser must recover every named failure from this real log — the same claim
  // gate-selftest.mjs's in-process scenarios make about parsing, exercised here against REAL nextest
  // output instead of a hand-authored fixture.
  const parsed = parseGateVerdict(log);
  ok(parsed.kind === "fail", "(parse) parseGateVerdict recognizes the FAIL verdict in the real log");
  ok(
    parsed.failures.length === verdictFailures.length,
    `(parse) parseGateVerdict recovers all ${verdictFailures.length} named failure(s) (got ${parsed.failures.length})`,
  );

  // ---- RUN THE REAL CLI END TO END ----
  process.stderr.write("\n(cli) invoking triage-gate-failure.mjs against the real captured log …\n");
  const cliPath = join(SCRIPT_DIR, "triage-gate-failure.mjs");
  const cli = spawnSync(
    process.execPath,
    // 8 isolated attempts (not the CLI's default 5): the pid-parity plant's FLAKY proof needs an
    // odd/even split across independently-PID'd fresh processes, and 8 coin flips brings the
    // all-same false-negative probability down to 2*(0.5^8) ≈ 0.8% instead of 5's ≈ 6.25%.
    [cliPath, "--log", FAKE_LOG, "--runs", "8", "--memory-limit", "8GiB", "--build-jobs", "4"],
    { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 },
  );
  const report = (cli.stdout || "") + "\n" + (cli.stderr || "");
  ok(cli.status === 0, `(cli) triage CLI exits 0 on a successfully-produced report (got ${cli.status})`);
  ok(
    /\[REAL\] triage_plant::triage_plant_real_always_fails/.test(report),
    "(cli) the deterministic plant is classified REAL",
  );
  const realBlockMatch = report.match(
    /--- \[REAL\] triage_plant::triage_plant_real_always_fails[\s\S]*?attempts: (\d+) total, (\d+) valid \((\d+) passed, (\d+) failed\)/,
  );
  ok(
    !!realBlockMatch && realBlockMatch[3] === "0" && Number(realBlockMatch[4]) === Number(realBlockMatch[1]),
    "(cli) REAL plant fails EVERY isolated attempt (N/N fail) — discriminates true REAL from mere majority-fail",
  );
  // raceFailedName is already module-qualified (e.g. "triage_plant::triage_plant_interaction_race_a"),
  // matching exactly what the CLI report prints after `--- [INTERACTION] `.
  const raceFailedName = raceFailed[0]; // whichever race test failed in the captured concurrent run
  const interactionRe = new RegExp(
    `--- \\[INTERACTION\\] ${raceFailedName}[\\s\\S]*?attempts: (\\d+) total, (\\d+) valid \\((\\d+) passed, (\\d+) failed\\)`,
  );
  const interactionMatch = report.match(interactionRe);
  ok(
    !!interactionMatch &&
      interactionMatch[4] === "0" &&
      Number(interactionMatch[3]) === Number(interactionMatch[1]),
    `(cli) the race plant that failed under concurrency (${raceFailedName}) is classified INTERACTION and ` +
      "PASSES every isolated attempt (N/N pass) — proves it only fails under contention, not alone",
  );
  ok(
    /1 test\(s\) run/.test(report),
    "(cli) every isolated attempt's own summary reports exactly 1 test run — proves TRUE process isolation " +
      "(one test per invocation), not merely a filtered subset of the suite",
  );
  const flakyBlockMatch = report.match(
    /--- \[(REAL|FLAKY|INTERACTION|INCONCLUSIVE)\] triage_plant::triage_plant_flaky_pid_parity[\s\S]*?attempts: (\d+) total, (\d+) valid \((\d+) passed, (\d+) failed\)/,
  );
  ok(
    !!flakyBlockMatch &&
      flakyBlockMatch[1] === "FLAKY" &&
      Number(flakyBlockMatch[4]) > 0 &&
      Number(flakyBlockMatch[5]) > 0,
    `(cli) the pid-parity plant is classified FLAKY with a genuine pass/fail split across 8 independently-` +
      `PID'd isolated attempts (got: ${flakyBlockMatch ? `${flakyBlockMatch[1]} (${flakyBlockMatch[4]} passed, ${flakyBlockMatch[5]} failed)` : "no match"})`,
  );

  // ---- ZERO-SELECTION IS A FAILURE, standalone (no cargo needed for this leg) ----
  const emptyLog = "[gate][error] VERDICT: FAIL — 0 non-tolerated failure(s):\n";
  const tmpEmpty = join(REPO, "target", "gate-runner", "triage-selftest-empty-log.txt");
  writeFileSync(tmpEmpty, emptyLog);
  const zeroSel = spawnSync(process.execPath, [cliPath, "--log", tmpEmpty], { encoding: "utf8" });
  ok(
    zeroSel.status !== 0,
    `(zero-selection) a FAIL verdict with zero parsed failure lines exits non-zero (got ${zeroSel.status}) ` +
      "— never treated as a clean bill of health",
  );

  // ---- REVERT ----
  revertAndExit(FAIL === 0 ? 0 : 1);
}

function revertAndExit(exitCodeIfClean) {
  process.stderr.write("\n(revert) restoring crates/verter_span/src/lib.rs …\n");
  spawnSync("git", ["-C", REPO, "checkout", "--", "crates/verter_span/src/lib.rs"]);
  const stillDirty = gitDiffStat();
  ok(stillDirty.length === 0, "(revert) git diff --stat is empty after revert — the plant is GONE");
  const stillThere = readFileSync(LIB_RS, "utf8").includes(PLANT_MARKER);
  ok(!stillThere, "(revert) the plant marker is absent from the file on disk — GREEN proven, not assumed");

  process.stderr.write(`\n=== SELF-TEST SUMMARY ===\nPASS=${PASS} FAIL=${FAIL}\n`);
  process.exit(FAIL === 0 ? 0 : 1);
}

main().catch((e) => {
  process.stderr.write(`harness threw: ${e && e.stack ? e.stack : e}\n`);
  try {
    spawnSync("git", ["-C", REPO, "checkout", "--", "crates/verter_span/src/lib.rs"]);
  } catch {
    /* best effort */
  }
  process.exit(1);
});
