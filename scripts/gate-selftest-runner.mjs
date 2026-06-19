#!/usr/bin/env node
// gate-selftest-runner.mjs — SELF-TEST-ONLY subprocess harness for the gate's mutex / process-containment /
// timeout / stall / teardown integration scenarios.
//
// THIS IS NOT THE PRODUCTION GATE. It exists ONLY so the self-test (`gate-selftest.mjs`) can drive the
// REAL gate primitives — the single-flight `Mutex`, `runContainedStep` (process-group containment + the
// whole-gate budget + the phase stall detector), the signal-teardown lifecycle (reap the active step tree
// before releasing the lock), and the multi-step seam — in a genuine subprocess with a genuine process
// group, against cargo-free `sleep`/`echo` stand-ins. The production gate CLI (`gate.mjs`) deliberately
// exposes NO arbitrary-command or seam mode; those test-only entry points live HERE, on a self-test
// script that production never runs, so the production gate can never return success without running the
// real gate.
//
// It imports the gate primitives DIRECTLY from `gate-internals.mjs` (the same code `gate.mjs` composes) so
// the scenarios exercise the ACTUAL gate behavior, not a re-implementation.
//
// MODES (selected by the first argv token — these args live on the SELF-TEST runner, never on gate.mjs):
//   --st-cmd  [--timeout D] [--stall D]        run a single shell command (from VERTER_SELFTEST_CMD) under
//                                              the mutex + containment + whole-gate budget + teardown.
//                                              Exit: 0 PASS · 1 FAIL · 124 TIMEOUT · 125 STALL · 126 LOCK.
//   --st-seam [--timeout D] [--stall D]        run the multi-step seam (steps from
//                                              VERTER_GATE_SELFTEST_STEPS, "\n"-joined "name|cmd" specs)
//                                              under the SHARED whole-gate budget. Same exit contract.
//
// ENV (read ONLY by this self-test runner):
//   VERTER_GATE_LOCK / MOM_GATE_LOCK   lockdir (same resolution as gate.mjs)
//   VERTER_GATE_TARGET_DIR             runner-owned target dir
//   VERTER_SELFTEST_CMD                the single shell command for --st-cmd
//   VERTER_GATE_SELFTEST_STEPS         the "\n"-joined "name|cmd" specs for --st-seam

import { fileURLToPath } from "node:url";
import {
  // EXIT_TIMEOUT / EXIT_STALL are mapped inside mapStepReason / runMultiStepSeam, not referenced here.
  EXIT_PASS,
  EXIT_LOCK_REFUSED,
  EXIT_USAGE,
  log,
  warn,
  err,
  nowMs,
  parseDuration,
  resolveRepoRoot,
  defaultLockDir,
  buildCargoEnv,
  Mutex,
  reapActiveStep,
  provenanceSweep,
  runContainedStep,
  mapStepReason,
  runMultiStepSeam,
  shellInvocation,
  mkdirSync,
  writeFileSync,
  join,
  dirname,
  isAbsolute,
} from "./gate-internals.mjs";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));

function parseArgs(argv) {
  const opts = {
    mode: null, // "cmd" | "seam"
    timeoutSecs: parseDuration("50m"),
    stallSecs: parseDuration("12m"),
  };
  let i = 0;
  if (argv[0] === "--st-cmd") {
    opts.mode = "cmd";
    i = 1;
  } else if (argv[0] === "--st-seam") {
    opts.mode = "seam";
    i = 1;
  } else {
    throw new Error(
      `gate-selftest-runner: first arg must be --st-cmd or --st-seam (got '${argv[0]}')`,
    );
  }
  while (i < argv.length) {
    const a = argv[i];
    if (a === "--timeout") opts.timeoutSecs = parseDuration(argv[++i]);
    else if (a === "--stall") opts.stallSecs = parseDuration(argv[++i]);
    else throw new Error(`gate-selftest-runner: unknown arg '${a}'`);
    i++;
  }
  return opts;
}

async function main() {
  let opts;
  try {
    opts = parseArgs(process.argv.slice(2));
  } catch (e) {
    err(e.message);
    process.exit(EXIT_USAGE);
  }

  const repoRealpath = resolveRepoRoot(SCRIPT_DIR);
  if (!repoRealpath) {
    err("gate-selftest-runner: could not determine repo root");
    process.exit(EXIT_USAGE);
  }

  const targetDirArg = process.env.VERTER_GATE_TARGET_DIR || "";
  const runnerTarget = targetDirArg
    ? isAbsolute(targetDirArg)
      ? targetDirArg
      : join(repoRealpath, targetDirArg)
    : join(repoRealpath, "target", "gate-runner");

  const lockdir =
    process.env.VERTER_GATE_LOCK || process.env.MOM_GATE_LOCK || defaultLockDir(repoRealpath);
  const token = `${process.pid}.${nowMs()}.${Math.floor(Math.random() * 1e9)}`;
  const cargoEnv = buildCargoEnv(process.env, runnerTarget);

  mkdirSync(runnerTarget, { recursive: true });
  try {
    writeFileSync(join(runnerTarget, ".metadata_never_index"), "");
  } catch {
    /* ignore */
  }

  const mutex = new Mutex(lockdir, token, {
    pid: process.pid,
    repoRealpath,
    targetDir: runnerTarget,
  });

  // The SAME teardown lifecycle gate.mjs uses: reap the active step's whole tree (verified) BEFORE
  // releasing the lock, then sweep, then release — memoized so the signal handler and the main `finally`
  // share one completion. This is what scenario (xvii) (SIGTERM to the gate pid only) exercises.
  let teardownPromise = null;
  const teardown = () => {
    if (teardownPromise) return teardownPromise;
    teardownPromise = (async () => {
      try {
        const reap = await reapActiveStep();
        if (reap && reap.reaped && !reap.confirmedDead) {
          warn(
            "gate-selftest-runner: active step tree NOT confirmed dead within budget (released anyway)",
          );
        }
      } catch {
        /* best-effort */
      }
      try {
        await provenanceSweep(runnerTarget, mutex.KILL_GRACE_MS);
      } catch {
        /* ignore */
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

  let acquired = false;
  try {
    acquired = await mutex.acquire();
  } catch (e) {
    err(`gate-selftest-runner: mutex error: ${e.message}`);
    await teardown();
    process.exit(EXIT_USAGE);
  }
  if (!acquired) {
    err(`gate-selftest-runner: LOCK-REFUSED: ${mutex.refuseDetail} (lockdir=${lockdir})`);
    await teardown();
    process.exit(EXIT_LOCK_REFUSED);
  }
  log(`gate-selftest-runner: mutex acquired (token=${token} lockdir=${lockdir})`);

  const deadlineMs = nowMs() + opts.timeoutSecs * 1000;
  const stallMs = opts.stallSecs * 1000;

  let exitCode = EXIT_PASS;
  try {
    if (opts.mode === "seam") {
      const steps = (process.env.VERTER_GATE_SELFTEST_STEPS || "").split("\n");
      exitCode = await runMultiStepSeam({
        steps,
        cargoEnv,
        repoRealpath,
        runnerTarget,
        deadlineMs,
        stallMs,
      });
    } else {
      // Single-command mode (TEST phase: byte-growth-only liveness — the silent-`sleep` stall scenario
      // relies on this).
      const cmdString = process.env.VERTER_SELFTEST_CMD || "";
      if (!cmdString.trim()) {
        err("gate-selftest-runner: --st-cmd given but VERTER_SELFTEST_CMD is empty");
        exitCode = EXIT_USAGE;
      } else {
        const inv = shellInvocation(cmdString);
        const res = await runContainedStep({
          cmd: inv.cmd,
          args: inv.args,
          cwd: repoRealpath,
          env: cargoEnv,
          phase: "test",
          deadlineMs,
          stallMs,
          targetDir: runnerTarget,
        });
        log(
          `gate-selftest-runner: cmd exit=${res.code} reason=${res.reason || "-"} secs=${Math.round(res.durationMs / 1000)}`,
        );
        exitCode = mapStepReason(res);
      }
    }
  } catch (e) {
    err(`gate-selftest-runner: error: ${e && e.stack ? e.stack : e}`);
    exitCode = EXIT_USAGE;
  } finally {
    await teardown();
  }
  process.exit(exitCode);
}

main().catch((e) => {
  err(`gate-selftest-runner: fatal: ${e && e.stack ? e.stack : e}`);
  process.exit(EXIT_USAGE);
});
