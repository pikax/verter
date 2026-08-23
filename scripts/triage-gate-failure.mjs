#!/usr/bin/env node
// triage-gate-failure.mjs — classify a FAILED `node scripts/gate.mjs` run's failures as REAL, FLAKY, or
// INTERACTION without ever re-running the full gate.
//
// THE RULING THIS IMPLEMENTS: "gate SHOULD ALWAYS be green in working branch! if the gate fails in a
// branch the agent should run the tests in isolation to confirm if is flaky or not, then report". The
// working branch's gate is green BY INVARIANT — it is never re-measured "to compare". A red working
// branch is a P0. So when a BLOCK branch's gate fails, this tool NEVER re-runs the base/working branch's
// gate to decide whether the failure pre-existed; it re-runs each failing test ALONE, in true per-test
// process isolation, N times, and reports what that proves.
//
// USAGE
//   node scripts/triage-gate-failure.mjs --log <path-to-captured-gate-output>
//   node scripts/triage-gate-failure.mjs --log /tmp/gate-output.txt --runs 8 --memory-limit 12GiB
//
// The log is the file you ALREADY have per this repo's long-running-command convention
// (`node scripts/gate.mjs 2>&1 | tee /tmp/gate-output.txt`) — this tool never re-runs the gate itself, and
// never needs to: gate.mjs's own FAIL verdict block already names every non-tolerated failure, and (for
// the two nextest surfaces) the raw recap mirrored earlier in the same log already carries each failing
// test's exact binary-id. Parsing reuses `gate-internals.mjs`'s own nextest recap/FAIL[ parser
// (`extractNextestTerminalFailures`) — there is no second nextest-output parser here.
//
// WHAT "isolation" MEANS. Every re-run is `cargo nextest run -E '<exact filter>' --test-threads 1` — ONE
// test, in its OWN process, nothing else scheduled alongside it. `.config/nextest.toml`'s `retries = 0` is
// left untouched and is load-bearing here too: a retry could let a later PASS quietly supersede an
// isolated FAIL, which is exactly the masking this tool exists to remove.
//
// CLASSIFICATION (see triage-gate-internals.mjs's doc-comment for the full contract):
//   REAL         — fails every isolated attempt.               The branch broke it.
//   FLAKY        — passes at least once AND fails at least once, ALONE. Genuinely intermittent.
//   INTERACTION  — passes every isolated attempt, despite failing under the full gate. Only fails under
//                  concurrency/ordering/shared state — report this as its OWN signal, not "flaky".
//   INCONCLUSIVE — no isolated attempt completed cleanly (every attempt aborted: timeout/stall/memory/
//                  zero-selection). Never silently folded into one of the three above.
//
// EXIT CODES: 0 once a report was produced (REAL/FLAKY/INTERACTION findings are the tool's SUCCESSFUL
// output, not a tool failure — see gate.mjs's own EXIT_* scheme, reused here). Non-zero when the tool
// itself could not do its job: no gate verdict found in the log (127), a FAIL verdict whose failure block
// parsed to literally nothing (1 — the "zero-selection is a failure" rule), or the run lock is held by a
// live gate/triage process (126).

import { readFileSync, mkdirSync } from "node:fs";
import { isAbsolute, join } from "node:path";
import { fileURLToPath } from "node:url";
import { dirname } from "node:path";
import {
  EXIT_PASS,
  EXIT_FAIL,
  EXIT_USAGE,
  EXIT_LOCK_REFUSED,
  log,
  warn,
  err,
  resolveRepoRoot,
  defaultLockDir,
  deriveGateResourceLimits,
  buildCargoEnv,
  Mutex,
  runContainedStep,
  provenanceSweep,
  parseDuration,
  parseMemorySize,
  extractNextestTerminalFailures,
  parseNextestSummary,
  nowMs,
} from "./gate-internals.mjs";
import {
  parseGateVerdict,
  splitGateLogSurfaces,
  resolveIsolationTargets,
  classifyAttempts,
} from "./triage-gate-internals.mjs";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));

function usageError(msg) {
  return new Error(msg);
}

function parsePositiveInteger(value, flag) {
  const parsed = Number(value);
  if (!/^\d+$/.test(String(value ?? "")) || !Number.isSafeInteger(parsed) || parsed < 1) {
    throw usageError(`${flag} requires a positive integer`);
  }
  return parsed;
}

function parseArgs(argv) {
  if (argv.includes("--help") || argv.includes("-h")) {
    return { mode: "help" };
  }
  const resources = deriveGateResourceLimits();
  const opts = {
    mode: "triage",
    logPath: "",
    runs: 5,
    buildJobs: resources.buildJobs,
    memoryLimitBytes: resources.memoryLimitBytes,
    targetDir: process.env.VERTER_GATE_TARGET_DIR || "",
    runTimeoutSecs: parseDuration("10m"),
    stallSecs: parseDuration("3m"),
  };
  let i = 0;
  while (i < argv.length) {
    const a = argv[i];
    if (a === "--log") {
      opts.logPath = argv[++i];
      if (opts.logPath === undefined) throw usageError("--log requires a value");
    } else if (a === "--runs") {
      opts.runs = parsePositiveInteger(argv[++i], "--runs");
    } else if (a === "--build-jobs") {
      opts.buildJobs = parsePositiveInteger(argv[++i], "--build-jobs");
    } else if (a === "--memory-limit") {
      opts.memoryLimitBytes = parseMemorySize(argv[++i]);
    } else if (a === "--target-dir") {
      opts.targetDir = argv[++i];
    } else if (a === "--run-timeout") {
      opts.runTimeoutSecs = parseDuration(argv[++i]);
    } else if (a === "--stall") {
      opts.stallSecs = parseDuration(argv[++i]);
    } else if (!a.startsWith("--") && !opts.logPath) {
      // Allow a bare positional as a convenience alias for --log.
      opts.logPath = a;
    } else {
      throw usageError(
        `unknown argument: '${a}'. This tool accepts --log/--runs/--build-jobs/--memory-limit/` +
          `--target-dir/--run-timeout/--stall/--help.`,
      );
    }
    i++;
  }
  if (!opts.logPath) {
    throw usageError(
      "--log <path> is required — the captured output of a FAILED `node scripts/gate.mjs` run " +
        "(this tool never re-runs the gate itself to obtain it).",
    );
  }
  return opts;
}

function printHelp() {
  const lines = readFileSync(fileURLToPath(import.meta.url), "utf8").split("\n");
  const header = [];
  for (let i = 0; i < lines.length; i++) {
    const l = lines[i];
    if (i === 0 && l.startsWith("#!")) continue;
    if (l.startsWith("//")) {
      header.push(l.replace(/^\/\/ ?/, ""));
      continue;
    }
    if (l.trim() === "") {
      if (header.length > 0 && lines[i + 1] && !lines[i + 1].startsWith("//")) break;
      if (header.length > 0) header.push("");
      continue;
    }
    break;
  }
  process.stderr.write(header.join("\n") + "\n");
}

// Run one isolated attempt of `target` under the given ctx. Returns { outcome, detail, text, code }.
async function runOneAttempt(target, ctx, opts) {
  const res = await runContainedStep({
    cmd: "cargo",
    args: target.runArgs,
    cwd: ctx.repoRealpath,
    env: ctx.cargoEnv,
    phase: "test",
    deadlineMs: nowMs() + opts.runTimeoutSecs * 1000,
    stallMs: opts.stallSecs * 1000,
    targetDir: ctx.runnerTarget,
    memoryLimitBytes: opts.memoryLimitBytes,
  });
  if (res.reason) {
    return {
      outcome: "abort",
      detail: `${res.reason} after ${Math.round(res.durationMs / 1000)}s`,
    };
  }
  const text = res.stdout + "\n" + res.stderr;
  const summary = parseNextestSummary(text);
  // SELECTION INTEGRITY, applied to our own re-run too: a filter that silently selected zero tests proves
  // nothing about the test — it is an isolation-command setup failure, never a clean pass.
  if (!summary.runCountFound || summary.runCount === 0) {
    return {
      outcome: "abort",
      detail: `isolation filter selected zero tests (exit ${res.code}) — the filter/binary-id may be stale`,
    };
  }
  const { failures } = extractNextestTerminalFailures(text);
  const matched = failures.some(
    (f) => f.name === target.name && (!target.binaryId || f.binaryId === target.binaryId),
  );
  if (matched)
    return { outcome: "fail", detail: `exit ${res.code}, ${summary.runCount} test(s) run` };
  if (res.code === 0 && summary.nonPassed === 0) {
    return { outcome: "pass", detail: `exit 0, ${summary.runCount} test(s) run` };
  }
  // Ran, matched nothing under our target's identity, but did not cleanly pass either (e.g. the filter
  // over-matched and swept in an UNRELATED failing test). Never silently counted as a vote either way.
  return {
    outcome: "abort",
    detail:
      `ambiguous isolated run (exit ${res.code}, ${summary.nonPassed} did not pass, none matched the ` +
      `target identity) — the filter likely matched more than the intended test`,
  };
}

function formatTargetReport(target, attempts, verdict) {
  const cmd = `cargo ${target.runArgs.map((a) => (/\s/.test(a) ? `'${a}'` : a)).join(" ")}`;
  const lines = [];
  lines.push(`--- [${verdict.classification}] ${target.name}`);
  lines.push(`    surface:  ${target.surface}`);
  if (target.binaryId) lines.push(`    binary:   ${target.binaryId}`);
  if (target.cargoProfile) lines.push(`    profile:  ${target.cargoProfile}`);
  if (target.packageScope) lines.push(`    package:  ${target.packageScope}`);
  lines.push(
    `    attempts: ${verdict.totalAttempts} total, ${verdict.validAttempts} valid ` +
      `(${verdict.passes} passed, ${verdict.fails} failed)${verdict.aborted > 0 ? `, ${verdict.aborted} aborted` : ""}`,
  );
  if (!verdict.complete) {
    lines.push(
      `    NOTE: ${verdict.aborted} of ${verdict.totalAttempts} attempts aborted (see below) — this ` +
        `classification is based on the ${verdict.validAttempts} valid attempt(s) only`,
    );
  }
  if (target.caveat) lines.push(`    caveat:   ${target.caveat}`);
  lines.push(`    reproduce: ${cmd}`);
  for (const a of attempts) {
    lines.push(`      attempt ${a.index}: ${a.outcome}${a.detail ? ` — ${a.detail}` : ""}`);
  }
  return lines.join("\n");
}

async function runTriage(opts) {
  let raw;
  try {
    raw = readFileSync(opts.logPath, "utf8");
  } catch (e) {
    err(`could not read --log '${opts.logPath}': ${e.message}`);
    return EXIT_USAGE;
  }

  const verdict = parseGateVerdict(raw);
  if (verdict.kind === "none") {
    err(
      `no gate VERDICT line found in '${opts.logPath}'. This tool triages a FAILED gate run's own ` +
        "output — it never re-runs the gate to obtain one. Capture the gate's output " +
        "(node scripts/gate.mjs 2>&1 | tee <file>) and point --log at the complete file.",
    );
    return EXIT_USAGE;
  }
  if (verdict.kind === "pass") {
    log(`'${opts.logPath}' shows a gate PASS — nothing to triage.`);
    return EXIT_PASS;
  }

  // verdict.kind === "fail" from here.
  if (verdict.failures.length === 0) {
    err(
      `'${opts.logPath}' shows VERDICT: FAIL but the failure block parsed to ZERO test ids. This is a ` +
        "triage FAILURE, not a clean bill of health — either the log is truncated right after the VERDICT " +
        "line, or gate.mjs's verdict-block format has drifted from what this tool parses. Do not treat this " +
        "as 'nothing to triage'.",
    );
    return EXIT_FAIL;
  }

  const surfaces = splitGateLogSurfaces(raw);
  const { targets, unclassifiable } = resolveIsolationTargets({
    failures: verdict.failures,
    surfaces,
    extractNextestTerminalFailures,
  });

  if (targets.length === 0 && unclassifiable.length === 0) {
    err(
      `'${opts.logPath}' shows ${verdict.failures.length} failure(s) but none could be classified at ` +
        "all — this is a triage FAILURE, not a clean bill of health.",
    );
    return EXIT_FAIL;
  }

  log(
    `parsed ${verdict.failures.length} non-tolerated failure(s) from the gate verdict: ` +
      `${targets.length} isolatable, ${unclassifiable.length} unclassifiable`,
  );

  const repoRealpath = resolveRepoRoot(SCRIPT_DIR);
  if (!repoRealpath) {
    err(`could not determine repo root (git rev-parse failed from ${SCRIPT_DIR})`);
    return EXIT_USAGE;
  }
  const runnerTarget = opts.targetDir
    ? isAbsolute(opts.targetDir)
      ? opts.targetDir
      : join(repoRealpath, opts.targetDir)
    : join(repoRealpath, "target", "gate-runner");
  mkdirSync(runnerTarget, { recursive: true });
  const cargoEnv = buildCargoEnv(process.env, runnerTarget, undefined, opts.buildJobs);
  const ctx = { repoRealpath, runnerTarget, cargoEnv };

  // Same single-flight lock gate.mjs uses (VERTER_GATE_LOCK / MOM_GATE_LOCK / the repo-keyed default) —
  // triage runs REAL cargo/nextest and must never run concurrently with a real gate (or another triage).
  const lockdir =
    process.env.VERTER_GATE_LOCK || process.env.MOM_GATE_LOCK || defaultLockDir(repoRealpath);
  const token = `triage.${process.pid}.${nowMs()}.${Math.floor(Math.random() * 1e9)}`;
  const mutex = new Mutex(lockdir, token, {
    pid: process.pid,
    repoRealpath,
    targetDir: runnerTarget,
  });
  let acquired = false;
  let teardownPromise = null;
  const teardown = () => {
    if (teardownPromise) return teardownPromise;
    teardownPromise = (async () => {
      if (acquired) {
        // Each runContainedStep call owns and reaps its own step-scoped supervisor in its own `finally`
        // (see gate-internals.mjs) — there is no cross-call "active step" handle to reap externally. On a
        // forced SIGINT/SIGTERM mid-step that in-flight reap may not get to run before we exit, so fall
        // back to the same general orphan sweep gate.mjs itself relies on: scan for any build-tool process
        // whose cmdline references our target dir and terminate it.
        try {
          await provenanceSweep(runnerTarget, mutex.KILL_GRACE_MS);
        } catch {
          /* ignore */
        }
      }
      mutex.release();
    })();
    return teardownPromise;
  };
  process.on("SIGINT", async () => {
    await teardown();
    process.exit(130);
  });
  process.on("SIGTERM", async () => {
    await teardown();
    process.exit(143);
  });

  try {
    acquired = await mutex.acquire();
  } catch (e) {
    err(`mutex error: ${e.message}`);
    await teardown();
    return EXIT_USAGE;
  }
  if (!acquired) {
    err(
      `LOCK-REFUSED: ${mutex.refuseDetail} (lockdir=${lockdir}) — another gate/triage run is active.`,
    );
    await teardown();
    return EXIT_LOCK_REFUSED;
  }
  log(`mutex acquired (token=${token} lockdir=${lockdir})`);

  let exitCode = EXIT_PASS;
  try {
    const reportSections = [];
    let realCount = 0;
    let flakyCount = 0;
    let interactionCount = 0;
    let inconclusiveCount = 0;

    for (const target of targets) {
      log(`isolating '${target.name}' (${target.surface}), ${opts.runs} run(s) …`);
      const attempts = [];
      for (let n = 1; n <= opts.runs; n++) {
        const a = await runOneAttempt(target, ctx, opts);
        attempts.push({ index: n, ...a });
        log(`  attempt ${n}/${opts.runs}: ${a.outcome}${a.detail ? ` (${a.detail})` : ""}`);
      }
      const cls = classifyAttempts(attempts);
      if (cls.classification === "REAL") realCount++;
      else if (cls.classification === "FLAKY") flakyCount++;
      else if (cls.classification === "INTERACTION") interactionCount++;
      else inconclusiveCount++;
      reportSections.push(formatTargetReport(target, attempts, cls));
    }

    process.stdout.write("\n=== GATE FAILURE TRIAGE REPORT ===\n");
    process.stdout.write(`source log: ${opts.logPath}\n`);
    process.stdout.write(
      `${targets.length} test(s) triaged: ${realCount} REAL, ${flakyCount} FLAKY, ` +
        `${interactionCount} INTERACTION, ${inconclusiveCount} INCONCLUSIVE\n`,
    );
    if (unclassifiable.length > 0) {
      process.stdout.write(
        `${unclassifiable.length} failure(s) could not be reduced to a single test id:\n`,
      );
      for (const u of unclassifiable) {
        process.stdout.write(`  [${u.surface}] ${u.name}\n    reason: ${u.reason}\n`);
      }
    }
    process.stdout.write("\n" + reportSections.join("\n\n") + "\n");
    process.stdout.write(
      "\nA FLAKY or INTERACTION classification is still a gate FAILURE — this report explains it, it " +
        "does not convert it into a pass. This tool never ran the full gate on any other branch to compare.\n",
    );

    if (inconclusiveCount > 0) {
      warn(
        `${inconclusiveCount} target(s) were INCONCLUSIVE — every isolated attempt aborted; see above.`,
      );
    }
    exitCode = EXIT_PASS;
  } finally {
    await teardown();
  }
  return exitCode;
}

async function main() {
  let opts;
  try {
    opts = parseArgs(process.argv.slice(2));
  } catch (e) {
    err(e.message);
    process.exit(EXIT_USAGE);
  }
  if (opts.mode === "help") {
    printHelp();
    process.exit(EXIT_PASS);
  }
  const code = await runTriage(opts);
  process.exit(code);
}

main().catch((e) => {
  err(`fatal: ${e && e.stack ? e.stack : e}`);
  process.exit(EXIT_USAGE);
});
