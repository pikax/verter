#!/usr/bin/env node
// gate-selftest.mjs — proves the safety properties of the gate using ONLY sleep/echo stand-ins.
//
// HOW IT DRIVES THE GATE PRIMITIVES (no magic flag on the production gate).
//   The classifier / verdict / sweep-matcher / suite-selection scenarios call the REAL gate functions
//   DIRECTLY, imported in-process from `gate-internals.mjs` (the same code `gate.mjs` composes) — NOT by
//   invoking `gate.mjs` with a test-seam flag (the production gate has none). The mutex / process-
//   containment / timeout / stall / teardown / seam scenarios — which genuinely need a real subprocess and
//   a real process group — spawn the SELF-TEST-ONLY runner `gate-selftest-runner.mjs`, which imports the
//   same gate primitives and runs them against `sleep`/`echo` stand-ins. The production gate (`gate.mjs`)
//   is exercised ONLY where the test asserts it has NO bypass mode (scenario U-P0).
//
// NO workspace cargo runs here. Every contained command is a `sleep`/`echo` stand-in, so the build lock is
// never touched. Each test uses a UNIQUE lock dir (an os.tmpdir() mkdtemp) so a developer's real lock is
// safe. The process-containment scenarios are POSIX-only (they assert the process-group containment that
// only exists on POSIX); the PLATFORM-INDEPENDENT classifier/sweep-matcher/argv/sentinel scenarios run on
// EVERY platform including Windows. On Windows, the POSIX-only process-management scenarios emit a TRUE
// skip that is NOT counted in the pass total (so a green Windows run never falsely implies process-mgmt
// runtime coverage); the gate.mjs Windows kill path — taskkill — is statically reviewed, not exercised
// here, because there is no portable `sleep`/argv-rename stand-in on Windows.
//
// ARGV-TAGGED SLEEPS (critical for honest, PORTABLE discrimination).
//   Every sleep stand-in is launched as `exec -a sleep_${RUN_TAG}_<role> sleep <n>` (via a `bash -c`
//   string the gate spawns) so the unique tag lands in the process ARGV, NOT in a shell COMMENT. A
//   `# tag` comment never reaches the sleep argv, so a `pgrep -f "$tag"` would return 0 whether or not
//   orphans leaked — a vacuous always-pass. The surviving-orphan assertions count REAL sleeps by reading
//   the FULL untruncated argv (`ps -o command=`) and matching the FIRST argv token's basename against
//   `sleep_${RUN_TAG}_`. They deliberately do NOT use `ps -o comm=`: on Linux procps truncates comm to
//   ~15 chars (TASK_COMM_LEN), so the long tag would never match the truncated comm and EVERY survivor
//   assertion would FALSELY PASS (vacuous on Linux). The full-argv first-token match works identically on
//   macOS and Linux and EXCLUDES the `bash -c "<string>"` wrapper (its argv CONTAINS the sentinels so a
//   substring search finds it, but its argv[0] basename is `bash`, not `sleep_…`).
//
// Properties proven (acceptance criteria — they MUST discriminate, not always-pass):
//   (i)    MUTEX        — a second concurrent run REFUSES with the LOCK-REFUSED code (126).
//   (ii)   STALE        — a SIGKILL'd holder's lockdir is reclaimed; a fresh run PASSes.
//   (iii)  TIMEOUT+ORPHAN — `--timeout 5s` on a wrapper that backgrounds two argv-tagged sleep 600s and
//                           waits returns TIMEOUT (124) and leaves ZERO surviving argv-tagged sleeps
//                           (the WHOLE process group reaped, not just the wrapper). FAILS if only the
//                           wrapper died.
//   (iv)   STALL        — a silent argv-tagged sleep 600 (TEST phase) is killed with STALL (125) well
//                           before 600s.
//   (v)    TEARDOWN     — after each test, no stray runner-spawned argv-tagged sleeps linger; lockdir gone.
//   (vi)   SWEEP        — a fake process whose argv carries $REPO_ROOT but NOT the runner target dir
//                           SURVIVES the provenance sweep; a fake process whose argv carries the runner
//                           target dir is SWEPT. (Proves the sweep keys SOLELY on the runner-owned target.)
//   (vii)  ALLOWLIST    — EXACT-name tolerated allowlist via canned nextest fixtures: only-allowlisted =>
//                           PASS-WITH-TOLERATED (0); allowlisted+non-allowlisted => FAIL (1); a
//                           non-allowlisted name that CONTAINS an allowlisted substring => FAIL; a name
//                           that is an ENTIRE allowlisted name PLUS a suffix => FAIL (exact-equality).
//   (viii) WHOLE-GATE TIMEOUT — a multi-step seam run (via the SELF-TEST-ONLY runner, `--st-seam`) whose
//                           cumulative time exceeds the WHOLE-gate budget TIMEOUTs at the budget, not at
//                           N×. The inverse (a fitting sequence) PASSes, so the test discriminates.
//   (U-P0) NO PRODUCTION-GATE BYPASS MODE — EVERY removed mode on the production CLI (`gate.mjs`) — the
//                           `--internal-selftest-seam` seam (incl. the empty-step case), each `--selftest-*`
//                           classifier hook, and the `-- <cmd>` custom-command path — now EXITS NON-ZERO
//                           (unknown-flag / usage, code 127), NEVER 0. With the legacy VERTER_GATE_SELFTEST
//                           env set, NO `node gate.mjs` argv returns the gate success contract without
//                           running the real gate. The discriminating control: a removed flag exits 127,
//                           while `--help` (a legitimate non-gate mode) still exits 0.
//   (ix)   SURFACE-1 NON-FAIL — a crash/leak (SIGABRT/LEAK) or a setup/harness error (non-zero exit, no
//                           `FAIL [` line) classifies FAIL on both the classifier and the live-aggregation
//                           hook; the tolerated baseline stays PASS-WITH-TOLERATED.
//   (x)    FAIL-CLOSED MUTEX — an alive holder with an EMPTY/uncheckable start-identity REFUSES (126); a
//                           dead holder with empty identity still reclaims + PASSes (discriminating).
//   (xi)   SURFACE-2 GATE — zero / partial verter_session suite selection FAILS SETUP (127); a proper
//                           1-lib + N-test listing passes (discriminating).
//   (xii)  WINDOWS .exe SWEEP — rustc.exe / cargo-nextest.exe / mixed-case CARGO.EXE referencing the runner
//                           target MATCH the provenance matcher; a repo-root-only dev cargo.exe does NOT.
//   (xiii) WATCHDOG REASON SURVIVES TRAPPED-SIGTERM EXIT-0 — a custom command that traps SIGTERM and
//                           exit(0)s AFTER a real `--timeout` still reports TIMEOUT (124); the verdict is
//                           keyed on a signaled-LIVE reap, not on the close `code === 0`. The pre-fix
//                           `code === 0`-clears-reason logic returned 0 (a real timeout masked as PASS).
//   (xiv)  SURFACE-1 SUMMARY-REQUIRED — a non-zero exit with tolerated `FAIL [` lines but a MISSING/
//                           unparseable Summary FAILS (it cannot prove the failures are accounted for); a
//                           summary-accounted tolerated failure and a clean exit-0 stay PASS-WITH-TOLERATED.
//                           Pre-fix the negative `unaccounted` swallowed it as PASS-WITH-TOLERATED.
//   (xv)   WINDOWS SWEEP QUOTED PATHS + PATH-TOKEN MATCH — a QUOTED full path to cargo.exe / rustc.exe is
//                           recognized (a quote is an exec-name boundary), and the runner target matches on
//                           a path-SEGMENT boundary so a SIBLING `…\target\gate-runner2` does NOT spuriously
//                           match `…\target\gate-runner`. Pre-fix: quoted paths missed; the sibling matched.
//   (P-1)  FOREIGN-SENTINEL RECLAIM REFUSE — a lockdir carrying ONLY the gate sentinel whose stored repo
//                           realpath is FOREIGN (differs from ours) and NO owner.json, past the init grace,
//                           is REFUSED (126) and its decoy file SURVIVES. Proves the sentinel-repo-realpath
//                           validation runs on the owner==null reclaim path too (pre-fix: `_reclaim(null)`
//                           renamed/removed a foreign checkout's mid-init sentinel-only lock).
//   (xix)  STUB-INVOKED side-effect — the A1 "no env bypass" assertion proves the failing-cargo STUB WAS
//                           ACTUALLY INVOKED by checking a marker FILE the stub writes (not merely that the
//                           exit was non-zero) — so a non-zero exit for an UNRELATED reason cannot vacuously
//                           pass the assertion.
//   (xx)   TEARDOWN VERIFIED REAP — a contained child that traps+ignores SIGTERM (so only SIGKILL ends it)
//                           is reaped on TIMEOUT and the tree is CONFIRMED dead (0 survivors) — proving the
//                           reap verifies death (poll past SIGKILL), not merely that the kill was issued.
//
// Exit non-zero if any property fails.

import { spawn, spawnSync, execFileSync } from "node:child_process";
import {
  mkdtempSync,
  rmSync,
  writeFileSync,
  readFileSync,
  existsSync,
  mkdirSync,
  realpathSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
// The REAL gate primitives — imported in-process so the classifier/verdict/sweep-matcher/suite-selection
// scenarios drive the ACTUAL gate code, not a re-implementation and not a magic flag on the production CLI.
import {
  classifyNextestFailures,
  analyzeNextestSurface,
  analyzeLibtestSurface,
  selectSessionSuites,
  isBuildTool,
  targetDirMatches,
  preparedSuccessLines,
  PREPARE_SUCCESS_MARKER,
} from "./gate-internals.mjs";

const SELFTEST_DIR = dirname(fileURLToPath(import.meta.url));
// The PRODUCTION gate CLI — exercised ONLY by the U-P0 "no bypass mode" scenario (to assert every removed
// mode exits non-zero). All contained-command / seam / mutex scenarios use the self-test-only runner below.
const GATE = join(SELFTEST_DIR, "gate.mjs");
// The SELF-TEST-ONLY subprocess runner (mutex + containment + timeout/stall + teardown + seam, against
// sleep/echo stand-ins). It imports the same gate primitives; production never runs it.
const RUNNER = join(SELFTEST_DIR, "gate-selftest-runner.mjs");

// The gate-owned lock sentinel file name — must match GATE_LOCK_SENTINEL in gate.mjs. A lockdir is
// reclaimable ONLY if it carries this marker (proving the gate created it); the crafted-lock scenarios
// below stamp it to model a real gate-created lock.
const GATE_LOCK_SENTINEL = ".verter-gate-lock";

// The repo realpath the gate itself computes (realpathSync of the git toplevel). The crafted owner.json
// scenarios must use this BYTE-IDENTICAL value so the gate's reclaim "owned by THIS repo" check passes for
// a dead holder (and so the refusal in the live/empty-identity case is proven to be the fail-closed-identity
// path, not the foreign-repo path).
const REPO_REALPATH = (() => {
  try {
    const top = execFileSync("git", ["-C", SELFTEST_DIR, "rev-parse", "--show-toplevel"], {
      encoding: "utf8",
    }).trim();
    try {
      return realpathSync(top);
    } catch {
      return top;
    }
  } catch {
    return "";
  }
})();

// Stamp a lockdir as gate-owned the same way gate.mjs does at acquire (sentinel = "<token>\n<repo>\n").
function writeSentinel(lockdir, token, repo) {
  writeFileSync(join(lockdir, GATE_LOCK_SENTINEL), `${token}\n${repo}\n`);
}

// Exit-code contract (must match gate.mjs).
const EXIT_PASS = 0;
const EXIT_FAIL = 1;
const EXIT_TIMEOUT = 124;
const EXIT_STALL = 125;
const EXIT_LOCK_REFUSED = 126;

let PASS_COUNT = 0;
let FAIL_COUNT = 0;
let SKIP_COUNT = 0;
const RESULTS = [];

function pass(msg) {
  RESULTS.push(`PASS  ${msg}`);
  PASS_COUNT++;
  process.stderr.write(`  PASS: ${msg}\n`);
}
function fail(msg) {
  RESULTS.push(`FAIL  ${msg}`);
  FAIL_COUNT++;
  process.stderr.write(`  FAIL: ${msg}\n`);
}
// A TRUE skip — recorded separately and NOT counted in the pass total, so a green Windows run never
// falsely implies runtime coverage of a scenario that did not run.
function skip(msg) {
  RESULTS.push(`SKIP  ${msg}`);
  SKIP_COUNT++;
  process.stderr.write(`  SKIP: ${msg}\n`);
}
function note(msg) {
  process.stderr.write(`  ... ${msg}\n`);
}

const IS_WINDOWS = process.platform === "win32";

// Unique marker per harness invocation so we only ever match OUR sleeps, never a developer's.
const RUN_TAG = `gatetest_${process.pid}_${Math.floor(Date.now() / 1000)}`;

const CLEAN_DIRS = [];
function registerClean(d) {
  CLEAN_DIRS.push(d);
}
function harnessCleanup() {
  // Kill any of OUR argv-tagged sleeps that somehow survived (belt and suspenders; tests assert empty).
  try {
    spawnSync("pkill", ["-9", "-f", `sleep_${RUN_TAG}_`], { stdio: "ignore" });
  } catch {
    /* ignore */
  }
  for (const d of CLEAN_DIRS) {
    try {
      rmSync(d, { recursive: true, force: true });
    } catch {
      /* ignore */
    }
  }
}
process.on("exit", harnessCleanup);
process.on("SIGINT", () => {
  harnessCleanup();
  process.exit(130);
});
process.on("SIGTERM", () => {
  harnessCleanup();
  process.exit(143);
});

function delay(ms) {
  return new Promise((r) => setTimeout(r, ms));
}

// Run the PRODUCTION gate.mjs CLI synchronously with the given argv + env; return { code }. Used ONLY by
// the U-P0 "no bypass mode" scenario (to assert removed modes exit non-zero) — every contained-command /
// seam scenario uses the self-test runner helpers below, NOT this.
function runGate(args, env) {
  const r = spawnSync(process.execPath, [GATE, ...args], {
    env: { ...process.env, ...env },
    stdio: ["ignore", "ignore", "ignore"],
  });
  // spawnSync sets .status (exit code) or .signal. A signalled exit yields null status.
  if (r.status === null && r.signal) return { code: 128, signal: r.signal };
  return { code: r.status === null ? 1 : r.status };
}

// Run the SELF-TEST-ONLY runner synchronously in single-command mode (`--st-cmd`): the given shell command
// runs under the REAL mutex + process containment + whole-gate budget + stall + teardown (the same
// primitives gate.mjs composes), against `sleep`/`echo` stand-ins. `flags` are runner flags (e.g.
// ["--timeout","5s"]). Returns { code }.
function runContainedCmd(cmdString, env, flags = []) {
  const r = spawnSync(process.execPath, [RUNNER, "--st-cmd", ...flags], {
    env: { ...process.env, ...env, VERTER_SELFTEST_CMD: cmdString },
    stdio: ["ignore", "ignore", "ignore"],
  });
  if (r.status === null && r.signal) return { code: 128, signal: r.signal };
  return { code: r.status === null ? 1 : r.status };
}

// Run the SELF-TEST-ONLY runner synchronously in multi-step seam mode (`--st-seam`): the "\n"-joined
// "name|cmd" steps run under the SHARED whole-gate budget. `flags` are runner flags. Returns { code }.
function runSeam(stepsString, env, flags = []) {
  const r = spawnSync(process.execPath, [RUNNER, "--st-seam", ...flags], {
    env: { ...process.env, ...env, VERTER_GATE_SELFTEST_STEPS: stepsString },
    stdio: ["ignore", "ignore", "ignore"],
  });
  if (r.status === null && r.signal) return { code: 128, signal: r.signal };
  return { code: r.status === null ? 1 : r.status };
}

// Spawn the SELF-TEST-ONLY runner DETACHED in the background (so we can hold a lock while probing it).
// detached:true gives the holder its OWN process group (PGID==PID), so we can SIGKILL its whole group with
// `process.kill(-pid, …)` for the STALE-reclaim scenario WITHOUT touching the harness's own group. The
// runner runs the given shell command under the mutex (single-command mode). Returns the ChildProcess.
function spawnContainedCmd(cmdString, env, flags = []) {
  const child = spawn(process.execPath, [RUNNER, "--st-cmd", ...flags], {
    env: { ...process.env, ...env, VERTER_SELFTEST_CMD: cmdString },
    stdio: ["ignore", "ignore", "ignore"],
    detached: true,
  });
  child.unref();
  return child;
}

// Wait (bounded, ~6s) for a gate holder to acquire the lock — its owner.json appears once it is held.
async function waitLockHeld(lk) {
  for (let w = 0; w < 60; w++) {
    if (existsSync(join(lk, "owner.json"))) return true;
    await delay(100);
  }
  return false;
}

// ----------------------------------------------------------------------------------------------------
// In-process verdict helpers — call the REAL imported gate functions directly and return the SAME
// PASS | PASS-WITH-TOLERATED | FAIL verdict string the old gate.mjs `--selftest-*` hooks emitted (now
// removed from the production CLI). These drive the actual classifier/verdict code in-process; no
// subprocess and no magic flag.
// ----------------------------------------------------------------------------------------------------

// nextest classifier (log content only, no exit code) — mirrors the old `--selftest-classify-nextest`.
function verdictClassifyNextest(text) {
  const cls = classifyNextestFailures(text);
  if (cls === "regression") return "FAIL";
  if (cls === "tolerated") return "PASS-WITH-TOLERATED";
  return "PASS";
}
function verdictClassifyNextestFile(file) {
  return verdictClassifyNextest(readFileSync(file, "utf8"));
}

// nextest LIVE-aggregation verdict (with exit code) — mirrors the old `--selftest-classify-nextest-run`.
function verdictNextestRun(code, text) {
  const r = analyzeNextestSurface(text, code);
  if (r.failures.length > 0) return "FAIL";
  if (r.toleratedCount > 0) return "PASS-WITH-TOLERATED";
  return "PASS";
}
function verdictNextestRunFile(code, file) {
  return verdictNextestRun(code, readFileSync(file, "utf8"));
}

// SURFACE-2 libtest verdict — mirrors the old `--selftest-libtest`.
function verdictLibtest(code, binaryId, text) {
  const r = analyzeLibtestSurface(text, code, binaryId);
  if (r.verdict === "fail") return "FAIL";
  if (r.verdict === "tolerated") return "PASS-WITH-TOLERATED";
  return "PASS";
}
function verdictLibtestFile(code, binaryId, file) {
  return verdictLibtest(code, binaryId, readFileSync(file, "utf8"));
}

// SURFACE-2 suite-selection gate — mirrors the old `--selftest-surface2`. Returns the same { code, out }
// shape the hook produced: 127 (USAGE/SETUP) on a tripped integrity gate, 0 + "OK lib=<n> test=<n>" else.
function verdictSurface2(allSuites) {
  const sel = selectSessionSuites(allSuites);
  if (sel.error) return { code: 127, out: "" };
  return { code: 0, out: `OK lib=${sel.lib} test=${sel.test}` };
}

// Provenance sweep matcher — mirrors the old `--selftest-sweep-match`. Returns "MATCH" | "NOMATCH" for the
// REAL predicate `isBuildTool(cmd) && targetDirMatches(cmd, targetDir, windows)`.
function verdictSweepMatch(plat, targetDir, cmd) {
  const windows = plat === "windows";
  return isBuildTool(cmd) && targetDirMatches(cmd, targetDir, windows) ? "MATCH" : "NOMATCH";
}

// ----------------------------------------------------------------------------------------------------
// Count live processes that are REAL argv-tagged sleeps for a given role pattern. PORTABLE — keys on the
// FULL untruncated argv, NEVER on `ps -o comm=`. `ps -o comm=` is unusable on Linux (procps truncates
// comm to TASK_COMM_LEN ~15 chars), so the long tag would never match and every survivor assertion would
// FALSELY PASS. Instead read the full argv via `ps -axww -o pid=,command=`, take argv[0]'s basename, and
// count it ONLY if that basename starts with `sleep_${pat}` — i.e. argv[0] IS the renamed sleep. The
// `bash -c "…"` wrapper's argv[0] basename is `bash`, so it is excluded.
// ----------------------------------------------------------------------------------------------------
function countArgvSleeps(pat) {
  let out = "";
  try {
    out = execFileSync("ps", ["-axww", "-o", "pid=,command="], {
      encoding: "utf8",
      maxBuffer: 64 * 1024 * 1024,
    });
  } catch {
    return 0;
  }
  let n = 0;
  for (const line of out.split("\n")) {
    const trimmed = line.replace(/^\s+/, "");
    if (!trimmed) continue;
    const sp = trimmed.indexOf(" ");
    if (sp < 0) continue;
    const pidTok = trimmed.slice(0, sp);
    if (!/^\d+$/.test(pidTok)) continue;
    const cmd = trimmed.slice(sp + 1).trimStart();
    if (!cmd) continue;
    const argv0 = cmd.split(/\s+/)[0];
    const base = argv0.slice(argv0.lastIndexOf("/") + 1).replace(/\\/g, "/");
    const baseName = base.slice(base.lastIndexOf("/") + 1);
    if (baseName.startsWith(`sleep_${pat}`)) n++;
  }
  return n;
}

// Is a pid alive?
function pidAlive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch (e) {
    return e.code === "EPERM";
  }
}

// Build a gate command string that backgrounds two argv-tagged sleeps and waits (the orphan shape).
function orphanCmd(dur) {
  return (
    `( exec -a sleep_${RUN_TAG}_orphanA sleep ${dur} ) & ` +
    `( exec -a sleep_${RUN_TAG}_orphanB sleep ${dur} ) & wait`
  );
}
// A single argv-tagged sleep (the stall / hold shapes).
function singleSleepCmd(role, dur) {
  return `exec -a sleep_${RUN_TAG}_${role} sleep ${dur}`;
}

// A fresh temp lockdir path (the dir itself must NOT exist yet — mkdir is the acquire).
function freshLock() {
  const base = mkdtempSync(join(tmpdir(), "gatetest-lock-"));
  registerClean(base);
  return join(base, "lock.d");
}
function freshTmpDir(prefix) {
  const d = mkdtempSync(join(tmpdir(), prefix));
  registerClean(d);
  return d;
}

// ====================================================================================================
async function main() {
  process.stderr.write("=== gate.mjs self-test (sleep/echo stand-ins only; NO cargo) ===\n");
  process.stderr.write(`gate: ${GATE}\n`);
  process.stderr.write(`run-tag: ${RUN_TAG}\n`);

  if (!existsSync(GATE)) {
    fail(`gate.mjs not found at ${GATE}`);
    finish();
    return;
  }

  // Platform split (U-P2 honesty): the PLATFORM-INDEPENDENT scenarios — the in-process classifier / verdict
  // / sweep-matcher / suite-selection / removed-mode-argv units — run on EVERY platform INCLUDING Windows
  // (they are pure function calls + argv probes, no `sleep`/`exec -a`/process-group primitives). The
  // POSIX-process-management scenarios (the mutex / containment / timeout / stall / teardown / seam ones
  // that spawn the runner with `sleep`/`exec -a` stand-ins and assert process-group reaping) are guarded by
  // `if (!IS_WINDOWS)` below; on Windows they emit a TRUE skip (NOT counted in the pass total) so a green
  // Windows run never falsely implies process-management RUNTIME coverage. The Windows kill path (taskkill
  // /T /F + CIM start-identity) is covered by static review + the Windows sweep-MATCHER regex units (xii,
  // xv), not by a `sleep`/argv-rename stand-in (there is none on Windows).
  if (IS_WINDOWS) {
    process.stderr.write(
      "\n[platform] Windows: running the PLATFORM-INDEPENDENT scenarios (classifier / verdict / sweep-\n" +
        "matcher / suite-selection / removed-mode-argv); the POSIX process-group containment scenarios\n" +
        "(mutex / timeout / stall / teardown / seam with sleep/exec-a stand-ins) emit a TRUE skip below and\n" +
        "are NOT counted as passes. The Windows kill path (taskkill /T /F + CIM start-identity) is covered\n" +
        "by static review + the Windows sweep-MATCHER regex units (xii, xv).\n",
    );
  }
  // Each POSIX-only scenario below opens a LABELED block and, on Windows, emits a TRUE skip (not counted in
  // the pass total) then `break`s out — so the platform-independent scenarios still run on Windows.

  // --------------------------------------------------------------------------------------------------
  // (i) MUTEX — a second concurrent run must REFUSE with LOCK-REFUSED (126).
  // --------------------------------------------------------------------------------------------------
  process.stderr.write("\n(i) MUTEX\n");
  posix_i: {
    if (IS_WINDOWS) {
      skip("(i) MUTEX — POSIX process-group containment (no Windows sleep/exec-a stand-in)");
      break posix_i;
    }
    const lk = freshLock();
    const tgt1 = freshTmpDir("gatetest-target-");
    const holder = spawnContainedCmd(singleSleepCmd("hold_i", 30), {
      VERTER_GATE_LOCK: lk,
      VERTER_GATE_TARGET_DIR: tgt1,
    });
    // Wait until the holder has acquired the lock (owner.json stamped) — bounded.
    const acq = await waitLockHeld(lk);
    if (!acq) {
      fail("(i) holder never acquired the lock within 6s");
    } else {
      note(`holder acquired lock (pid=${holder.pid})`);
      const tgt2 = freshTmpDir("gatetest-target-");
      const second = runContainedCmd(singleSleepCmd("second_i", 30), {
        VERTER_GATE_LOCK: lk,
        VERTER_GATE_TARGET_DIR: tgt2,
      });
      if (second.code === EXIT_LOCK_REFUSED) {
        pass(`(i) MUTEX: second concurrent run refused with LOCK-REFUSED (${EXIT_LOCK_REFUSED})`);
      } else {
        fail(
          `(i) MUTEX: second run returned ${second.code}, expected LOCK-REFUSED (${EXIT_LOCK_REFUSED})`,
        );
      }
    }
    // Graceful SIGTERM to the holder's group so the gate's SIGTERM trap runs teardown (releases the lock).
    try {
      process.kill(-holder.pid, "SIGTERM");
    } catch {
      try {
        holder.kill("SIGTERM");
      } catch {
        /* ignore */
      }
    }
    // Wait (bounded) for the holder's teardown to release the lockdir.
    for (let w = 0; w < 40; w++) {
      if (!existsSync(lk)) break;
      await delay(100);
    }
    spawnSync("pkill", ["-9", "-f", `sleep_${RUN_TAG}_hold_i`], { stdio: "ignore" });
    await delay(500);
    const left = countArgvSleeps(`${RUN_TAG}_hold_i`);
    if (!existsSync(lk) && left === 0) {
      pass("(v/i) TEARDOWN: lockdir released and 0 stray tagged sleeps after MUTEX test");
    } else {
      fail(`(v/i) TEARDOWN: lockdir-exists=${existsSync(lk)} stray_sleeps=${left}`);
    }
  }

  // --------------------------------------------------------------------------------------------------
  // (ii) STALE reclaim — SIGKILL a holder (cleanup never runs), fresh run reclaims and PASSes.
  // --------------------------------------------------------------------------------------------------
  process.stderr.write("\n(ii) STALE reclaim\n");
  posix_ii: {
    if (IS_WINDOWS) {
      skip(
        "(ii) STALE reclaim — POSIX process-group containment (no Windows sleep/exec-a stand-in)",
      );
      break posix_ii;
    }
    const lk = freshLock();
    const tgt = freshTmpDir("gatetest-target-");
    const holder = spawnContainedCmd(singleSleepCmd("hold_ii", 60), {
      VERTER_GATE_LOCK: lk,
      VERTER_GATE_TARGET_DIR: tgt,
    });
    const acq = await waitLockHeld(lk);
    if (!acq) {
      fail("(ii) holder never acquired the lock");
    } else {
      note(
        `stale-holder acquired (pid=${holder.pid}); SIGKILL the whole group so cleanup never runs`,
      );
      // SIGKILL the holder's OWN process group (spawnGate ran it detached, PGID==pid). This kills the gate
      // process + its step child + the sleep WITHOUT running the gate's cleanup — so the lockdir must
      // survive for the stale-reclaim path. The harness lives in a different group, untouched.
      try {
        process.kill(-holder.pid, "SIGKILL");
      } catch {
        try {
          holder.kill("SIGKILL");
        } catch {
          /* ignore */
        }
      }
      spawnSync("pkill", ["-9", "-f", `sleep_${RUN_TAG}_hold_ii`], { stdio: "ignore" });
      await delay(1000);
      if (!existsSync(lk)) {
        fail("(ii) lockdir vanished after SIGKILL — cannot exercise stale reclaim");
      } else {
        note("lockdir survived SIGKILL (as required for the stale path)");
        const tgt2 = freshTmpDir("gatetest-target-");
        const reclaim = runContainedCmd(`echo reclaimed_ii_${RUN_TAG}`, {
          VERTER_GATE_LOCK: lk,
          VERTER_GATE_TARGET_DIR: tgt2,
        });
        if (reclaim.code === EXIT_PASS) {
          pass("(ii) STALE: fresh run reclaimed the stale lock and PASSed (rc=0, did NOT refuse)");
        } else if (reclaim.code === EXIT_LOCK_REFUSED) {
          fail(
            `(ii) STALE: fresh run REFUSED a dead holder's lock (rc=${EXIT_LOCK_REFUSED}) — reclaim broken`,
          );
        } else {
          fail(`(ii) STALE: fresh run returned ${reclaim.code} (expected PASS=0)`);
        }
      }
    }
    await delay(500);
    const left = countArgvSleeps(`${RUN_TAG}_hold_ii`);
    if (!existsSync(lk) && left === 0) {
      pass("(v/ii) TEARDOWN: lockdir released and 0 stray tagged sleeps after STALE test");
    } else {
      fail(`(v/ii) TEARDOWN: lockdir-exists=${existsSync(lk)} stray_sleeps=${left}`);
    }
  }

  // --------------------------------------------------------------------------------------------------
  // (iii) TIMEOUT + ORPHAN-FIX (HEADLINE). Two backgrounded argv-tagged sleep 600s under a wrapper that
  //       `wait`s them. After --timeout 5s, NO argv-tagged sleep may survive. This discriminates: killing
  //       only the wrapper (the bash that did `wait`) would orphan both sleeps and FAIL this test.
  // --------------------------------------------------------------------------------------------------
  process.stderr.write("\n(iii) TIMEOUT + ORPHAN-FIX (headline)\n");
  posix_iii: {
    if (IS_WINDOWS) {
      skip(
        "(iii) TIMEOUT+ORPHAN — POSIX process-group containment (no Windows sleep/exec-a stand-in)",
      );
      break posix_iii;
    }
    const lk = freshLock();
    const tgt = freshTmpDir("gatetest-target-");
    note(`before: argv-tagged-sleep count = ${countArgvSleeps(`${RUN_TAG}_orphan`)}`);
    const t0 = Date.now();
    const r = runContainedCmd(
      orphanCmd(600),
      { VERTER_GATE_LOCK: lk, VERTER_GATE_TARGET_DIR: tgt },
      ["--timeout", "5s"],
    );
    const elapsed = Math.round((Date.now() - t0) / 1000);
    await delay(2000);
    const after = countArgvSleeps(`${RUN_TAG}_orphan`);
    note(
      `returned rc=${r.code} after ${elapsed}s; after: surviving argv-tagged-sleep count = ${after}`,
    );
    let ok = true;
    if (r.code !== EXIT_TIMEOUT) {
      fail(`(iii) expected TIMEOUT code (${EXIT_TIMEOUT}), got ${r.code}`);
      ok = false;
    }
    if (elapsed >= 60) {
      fail(`(iii) took ${elapsed}s — timeout did not bound the run near 5s`);
      ok = false;
    }
    if (after !== 0) {
      fail(
        `(iii) ORPHAN-FIX: ${after} argv-tagged sleep 600 process(es) SURVIVED the group kill — only the wrapper was reaped, NOT the group`,
      );
      spawnSync("pkill", ["-9", "-f", `sleep_${RUN_TAG}_orphan`], { stdio: "ignore" });
      ok = false;
    }
    if (ok) {
      pass(
        `(iii) TIMEOUT+ORPHAN: rc=TIMEOUT in ${elapsed}s AND 0 surviving sleep 600 (whole process group reaped)`,
      );
    }
    await delay(500);
    const left = countArgvSleeps(`${RUN_TAG}_orphan`);
    if (!existsSync(lk) && left === 0) {
      pass("(v/iii) TEARDOWN: lockdir released and 0 stray tagged sleeps after TIMEOUT test");
    } else {
      fail(`(v/iii) TEARDOWN: lockdir-exists=${existsSync(lk)} stray_sleeps=${left}`);
    }
  }

  // --------------------------------------------------------------------------------------------------
  // (iv) STALL — a silent argv-tagged sleep 600 (no output) under the TEST phase is killed with STALL
  //      well before 600s. We force the command to be classified as a TEST-phase step via the gate's
  //      custom-step phase override (a custom `--` gate is treated as a TEST phase: byte-growth only).
  // --------------------------------------------------------------------------------------------------
  process.stderr.write("\n(iv) STALL (test-phase: byte-growth-only liveness)\n");
  posix_iv: {
    if (IS_WINDOWS) {
      skip("(iv) STALL — POSIX process-group containment (no Windows sleep/exec-a stand-in)");
      break posix_iv;
    }
    const lk = freshLock();
    const tgt = freshTmpDir("gatetest-target-");
    const t0 = Date.now();
    const r = runContainedCmd(
      singleSleepCmd("stall", 600),
      { VERTER_GATE_LOCK: lk, VERTER_GATE_TARGET_DIR: tgt },
      ["--stall", "5s", "--timeout", "600s"],
    );
    const elapsed = Math.round((Date.now() - t0) / 1000);
    await delay(2000);
    let ok = true;
    if (r.code !== EXIT_STALL) {
      fail(`(iv) expected STALL code (${EXIT_STALL}), got ${r.code}`);
      ok = false;
    }
    if (elapsed >= 120) {
      fail(`(iv) took ${elapsed}s — stall detector did not fire near 5s (well before 600s)`);
      ok = false;
    }
    const stallLeft = countArgvSleeps(`${RUN_TAG}_stall`);
    if (stallLeft !== 0) {
      fail(
        `(iv) STALL: the stalled sleep survived (${stallLeft} left) — group not reaped on stall`,
      );
      spawnSync("pkill", ["-9", "-f", `sleep_${RUN_TAG}_stall`], { stdio: "ignore" });
      ok = false;
    }
    if (ok) {
      pass(`(iv) STALL: rc=STALL in ${elapsed}s (<<600s) and the stalled group was reaped`);
    }
    await delay(500);
    const left = countArgvSleeps(`${RUN_TAG}_stall`);
    if (!existsSync(lk) && left === 0) {
      pass("(v/iv) TEARDOWN: lockdir released and 0 stray tagged sleeps after STALL test");
    } else {
      fail(`(v/iv) TEARDOWN: lockdir-exists=${existsSync(lk)} stray_sleeps=${left}`);
    }
  }

  // --------------------------------------------------------------------------------------------------
  // (vi) SWEEP provenance scoping. The provenance sweep must key SOLELY on the runner-owned target dir.
  //      We spawn TWO argv-tagged decoys directly (NOT through the gate): one whose argv carries the repo
  //      root but NOT the target dir (a stand-in for a dev `cargo build --manifest-path <repo>/...`), and
  //      one whose argv carries the runner target dir (a runner-owned rustc stand-in). We run a trivial
  //      gate (whose teardown sweep keys on its own runner target dir) and assert: repo-root-only decoy
  //      SURVIVES, target-dir decoy is SWEPT.
  // --------------------------------------------------------------------------------------------------
  process.stderr.write("\n(vi) SWEEP provenance scoping\n");
  posix_vi: {
    if (IS_WINDOWS) {
      skip(
        "(vi) SWEEP provenance scoping — POSIX process spawn/kill (no Windows sleep/exec-a stand-in; the matcher itself is unit-covered by xii/xv)",
      );
      break posix_vi;
    }
    const repoRoot = (() => {
      try {
        return execFileSync("git", ["-C", SELFTEST_DIR, "rev-parse", "--show-toplevel"], {
          encoding: "utf8",
        }).trim();
      } catch {
        return "";
      }
    })();
    const gateTarget = freshTmpDir("gatetest-target-");
    if (!repoRoot) {
      fail("(vi) could not determine repo root for the sweep test");
    } else {
      note(`repo_root=${repoRoot}`);
      note(`gate_target=${gateTarget}`);
      // Decoy A: argv carries the repo root but NOT the runner target dir => legit dev process model.
      const decoyAArgv = `sleep_${RUN_TAG}_sweepDevA cargo build --manifest-path ${repoRoot}/Cargo.toml`;
      // Decoy B: argv carries the runner-owned target dir => runner-owned rustc model (must be swept).
      const decoyBArgv = `sleep_${RUN_TAG}_sweepRunnerB rustc --out-dir ${gateTarget}/debug/deps lib.rs`;
      const decoyA = spawn("bash", ["-c", `exec -a "${decoyAArgv}" sleep 30`], { stdio: "ignore" });
      const decoyB = spawn("bash", ["-c", `exec -a "${decoyBArgv}" sleep 30`], { stdio: "ignore" });
      await delay(700);
      const aBefore = pidAlive(decoyA.pid);
      const bBefore = pidAlive(decoyB.pid);
      note(`before sweep: devA(repo-root-only)=${aBefore} runnerB(target-dir)=${bBefore}`);
      if (!aBefore || !bBefore) {
        fail(`(vi) decoys did not both start (devA=${aBefore} runnerB=${bBefore})`);
      } else {
        const lk = freshLock();
        // A trivial gate run whose teardown provenance sweep keys on THIS runner target dir.
        runContainedCmd(`echo sweep_probe_${RUN_TAG}`, {
          VERTER_GATE_LOCK: lk,
          VERTER_GATE_TARGET_DIR: gateTarget,
        });
        await delay(2000);
        const aAfter = pidAlive(decoyA.pid);
        const bAfter = pidAlive(decoyB.pid);
        note(`after sweep:  devA(repo-root-only)=${aAfter} runnerB(target-dir)=${bAfter}`);
        let ok = true;
        if (!aAfter) {
          fail(
            "(vi) SWEEP: the repo-root-only dev decoy was KILLED — the sweep must NOT key on the repo root",
          );
          ok = false;
        }
        if (bAfter) {
          fail(
            "(vi) SWEEP: the runner-owned target-dir decoy SURVIVED — the sweep must reap target-dir processes",
          );
          ok = false;
        }
        if (ok) {
          pass(
            "(vi) SWEEP: repo-root-only dev process SURVIVED and target-dir process was SWEPT (sweep keys solely on the runner-owned target dir)",
          );
        }
      }
      try {
        process.kill(decoyA.pid, "SIGKILL");
      } catch {
        /* ignore */
      }
      try {
        process.kill(decoyB.pid, "SIGKILL");
      } catch {
        /* ignore */
      }
    }
  }

  // --------------------------------------------------------------------------------------------------
  // (vii) EXACT-name tolerated allowlist. Drives the REAL classifier + verdict mapping IN-PROCESS
  //       (`classifyNextestFailures`) on canned nextest-style fixtures (no cargo). FOUR cases — the last two
  //       are lookalikes that a substring/prefix match would WRONGLY tolerate but exact-equality must FAIL:
  //       (c) the allowlisted token as a SUBSTRING inside a different path, and (d) the ENTIRE allowlisted
  //       name PLUS a suffix.
  // --------------------------------------------------------------------------------------------------
  process.stderr.write("\n(vii) EXACT-name tolerated allowlist\n");
  {
    const fixDir = freshTmpDir("gatetest-fix-");
    const A = join(fixDir, "only_allowlisted.log");
    const B = join(fixDir, "allowlisted_plus_real.log");
    const C = join(fixDir, "substring_lookalike.log");
    const D = join(fixDir, "affix_lookalike.log");
    // (a) ONLY allowlisted tests failed => PASS-WITH-TOLERATED. Post-consolidation exact name under the
    //     single verter_protocol::main binary: cases::typeinfo_proto_ts_freshness::<fn>.
    writeFileSync(
      A,
      "    FAIL [   0.012s] verter_protocol::main cases::typeinfo_proto_ts_freshness::typeinfo_ts_bindings_are_byte_equal_to_regenerated_buf_output\n",
    );
    // (b) an allowlisted test PLUS a non-allowlisted test failed => FAIL.
    writeFileSync(
      B,
      "    FAIL [   0.012s] verter_protocol::main cases::typeinfo_proto_ts_freshness::typeinfo_ts_bindings_are_byte_equal_to_regenerated_buf_output\n" +
        "    FAIL [   0.030s] verter_compiler::main template::vmemo::renders_cached\n",
    );
    // (c) a NON-allowlisted test whose name merely CONTAINS an allowlisted substring failed => FAIL.
    writeFileSync(
      C,
      "    FAIL [   0.041s] verter_session::main cases::typeinfo_proto_ts_freshness_lookalike::regresses\n",
    );
    // (d) a NON-allowlisted test whose exact final token is an ENTIRE allowlisted name PLUS a suffix => FAIL.
    writeFileSync(
      D,
      "    FAIL [   0.044s] verter_protocol::main cases::typeinfo_proto_ts_freshness::typeinfo_ts_bindings_are_byte_equal_to_regenerated_buf_output_extra\n",
    );
    const classify = (file) => verdictClassifyNextestFile(file);
    const va = classify(A);
    const vb = classify(B);
    const vc = classify(C);
    const vd = classify(D);
    note(`(a) only-allowlisted               => ${va}  (expect PASS-WITH-TOLERATED)`);
    note(`(b) allowlisted+real               => ${vb}  (expect FAIL)`);
    note(`(c) substring-lookalike            => ${vc}  (expect FAIL)`);
    note(
      `(d) full-name+suffix lookalike     => ${vd}  (expect FAIL — proves exact-equality, not prefix/contains)`,
    );
    let ok = true;
    if (va !== "PASS-WITH-TOLERATED") {
      fail(`(vii a) only-allowlisted => '${va}', expected PASS-WITH-TOLERATED`);
      ok = false;
    }
    if (vb !== "FAIL") {
      fail(`(vii b) allowlisted+real => '${vb}', expected FAIL`);
      ok = false;
    }
    if (vc !== "FAIL") {
      fail(
        `(vii c) substring-lookalike => '${vc}', expected FAIL (proves exact-name, not substring)`,
      );
      ok = false;
    }
    if (vd !== "FAIL") {
      fail(
        `(vii d) full-allowlisted-name+suffix => '${vd}', expected FAIL (a prefix/contains match would wrongly tolerate it; exact-equality must reject)`,
      );
      ok = false;
    }
    if (ok) {
      pass(
        "(vii) ALLOWLIST: exact-name — only-allowlisted=>PASS-WITH-TOLERATED, +real=>FAIL, substring-lookalike=>FAIL, full-name+suffix=>FAIL",
      );
    }
  }

  // --------------------------------------------------------------------------------------------------
  // (viii) WHOLE-GATE TIMEOUT. The --timeout is a WHOLE-gate budget for the ENTIRE multi-step sequence,
  //        NOT per-step (per-step would allow ~N×--timeout). We drive the REAL multi-step seam via the
  //        self-test runner (`--st-seam`): THREE separate steps, each a 4s argv-tagged sleep (12s of work
  //        if each got the full budget). Under a WHOLE-gate --timeout 6s the sequence MUST TIMEOUT after
  //        ~6-9s having run only ~1.5 steps, NOT ~12s (which would be per-step). It must also reap the
  //        running step's group. ALSO assert the inverse: a fitting sequence PASSes (discrimination).
  // --------------------------------------------------------------------------------------------------
  process.stderr.write("\n(viii) WHOLE-GATE TIMEOUT (cumulative budget bound)\n");
  posix_viii: {
    if (IS_WINDOWS) {
      skip(
        "(viii) WHOLE-GATE TIMEOUT — POSIX seam with sleep/exec-a stand-ins (no Windows equivalent)",
      );
      break posix_viii;
    }
    let ok = true;
    const lk = freshLock();
    const tgt = freshTmpDir("gatetest-target-");
    const stepsOver = [
      `seqA|exec -a sleep_${RUN_TAG}_seqA sleep 4`,
      `seqB|exec -a sleep_${RUN_TAG}_seqB sleep 4`,
      `seqC|exec -a sleep_${RUN_TAG}_seqC sleep 4`,
    ].join("\n");
    const t0 = Date.now();
    const r = runSeam(stepsOver, { VERTER_GATE_LOCK: lk, VERTER_GATE_TARGET_DIR: tgt }, [
      "--timeout",
      "6s",
      "--stall",
      "600s",
    ]);
    const elapsed = Math.round((Date.now() - t0) / 1000);
    await delay(2000);
    note(
      `over-budget: rc=${r.code} after ${elapsed}s (3 steps x4s=12s of work under a 6s whole-gate budget)`,
    );
    if (r.code !== EXIT_TIMEOUT) {
      fail(
        `(viii) over-budget: expected TIMEOUT (${EXIT_TIMEOUT}) at the whole-gate budget, got ${r.code}`,
      );
      ok = false;
    }
    if (elapsed >= 11) {
      fail(
        `(viii) over-budget: took ${elapsed}s — the whole-gate budget did NOT bound the sequence near 6s (ran ~full 12s; per-step not whole-gate)`,
      );
      ok = false;
    }
    const seqLeft =
      countArgvSleeps(`${RUN_TAG}_seqA`) +
      countArgvSleeps(`${RUN_TAG}_seqB`) +
      countArgvSleeps(`${RUN_TAG}_seqC`);
    if (seqLeft !== 0) {
      fail(`(viii) over-budget: ${seqLeft} step sleeps survived the whole-gate TIMEOUT reap`);
      spawnSync("pkill", ["-9", "-f", `sleep_${RUN_TAG}_seq`], { stdio: "ignore" });
      ok = false;
    }
    // Inverse (discrimination): a sequence whose cumulative time FITS the budget must PASS, not TIMEOUT.
    const lk2 = freshLock();
    const tgt2 = freshTmpDir("gatetest-target-");
    const stepsFit = [
      `fitA|exec -a sleep_${RUN_TAG}_fitA sleep 1`,
      `fitB|exec -a sleep_${RUN_TAG}_fitB sleep 1`,
      `fitC|exec -a sleep_${RUN_TAG}_fitC sleep 1`,
    ].join("\n");
    const rf = runSeam(stepsFit, { VERTER_GATE_LOCK: lk2, VERTER_GATE_TARGET_DIR: tgt2 }, [
      "--timeout",
      "30s",
      "--stall",
      "600s",
    ]);
    note(`within-budget: rc=${rf.code} (3 steps x1s=3s under a 30s budget)`);
    if (rf.code !== EXIT_PASS) {
      fail(
        `(viii) within-budget: expected PASS (${EXIT_PASS}), got ${rf.code} — the budget wrongly tripped a fitting sequence`,
      );
      ok = false;
    }
    spawnSync("pkill", ["-9", "-f", `sleep_${RUN_TAG}_fit`], { stdio: "ignore" });
    if (ok) {
      pass(
        `(viii) WHOLE-GATE TIMEOUT: 3-step 12s-of-work TIMED OUT at ~${elapsed}s (whole-gate 6s budget, not 12s) and was reaped; a 3s sequence under 30s PASSed`,
      );
    }
  }

  // --------------------------------------------------------------------------------------------------
  // (ix) SURFACE-1 NON-`FAIL` FAILURE CLASSIFICATION. A crashing/leaking test renders under a NON-`FAIL`
  //      nextest status (SIGABRT/SIGSEGV/LEAK/TIMEOUT/…) and a nextest setup/harness error exits non-zero
  //      with NO `FAIL [` line at all. Both MUST fail the gate; the pre-fix classifier (which printed PASS
  //      whenever no `FAIL [` line was present, and tolerated when only a tolerated `FAIL` name parsed)
  //      swallowed them. We drive BOTH the content classifier (`classifyNextestFailures`, no exit code) AND
  //      the LIVE-aggregation analyzer (`analyzeNextestSurface(text, code)`, the EXACT code runGate's
  //      SURFACE-1 path runs) IN-PROCESS, so the testable and live paths are proven to AGREE. Each crash
  //      fixture asserts FAIL; the tolerated baseline still asserts PASS-WITH-TOLERATED (discrimination).
  // --------------------------------------------------------------------------------------------------
  process.stderr.write(
    "\n(ix) SURFACE-1 non-FAIL failure classification (crash/leak/setup-error)\n",
  );
  {
    const fixDir = freshTmpDir("gatetest-nxfix-");
    const sigabrt = join(fixDir, "sigabrt.log");
    const leakPlusTolerated = join(fixDir, "leak_plus_tolerated.log");
    const setupError = join(fixDir, "setup_error.log");
    const tolerated = join(fixDir, "tolerated.log");
    // A SIGABRT crash with NO `FAIL [` line; the summary still counts it as failed and the run exits
    // non-zero. Pre-fix: classified PASS (no FAIL line). Post-fix: FAIL.
    writeFileSync(
      sigabrt,
      "    PASS [   0.010s] verter_compiler template::renders\n" +
        "    SIGABRT [   0.204s] verter_other crash::aborts_in_drop\n" +
        "     Summary [   1.230s] 2 tests run: 1 passed, 1 failed, 0 skipped\n",
    );
    // A tolerated `FAIL` PLUS an unaccounted LEAK: summary failed=2 but only 1 `FAIL` name parses, so the
    // unaccounted shortfall trips. Pre-fix: classified PASS-WITH-TOLERATED (only the tolerated FAIL name
    // was checked). Post-fix: FAIL.
    writeFileSync(
      leakPlusTolerated,
      "    FAIL [   0.204s] verter_protocol::main cases::typeinfo_proto_ts_freshness::typeinfo_ts_bindings_are_byte_equal_to_regenerated_buf_output\n" +
        "    LEAK [   0.300s] verter_other::main resource::leaks_a_handle\n" +
        "     Summary [   1.500s] 3 tests run: 1 passed, 2 failed, 0 skipped\n",
    );
    // A nextest harness/setup error: non-zero exit, NO `FAIL [` line, NO Summary line. Pre-fix: PASS.
    // Post-fix: FAIL (the `code !== 0 && no FAIL name` arm).
    writeFileSync(setupError, "error: creating test list failed\nCaused by: harness error\n");
    // The real tolerated baseline shape (the 2 env FAILs, summary failed=2): still PASS-WITH-TOLERATED.
    writeFileSync(
      tolerated,
      "    FAIL [   0.204s] verter_protocol::main cases::typeinfo_proto_ts_freshness::typeinfo_ts_bindings_are_byte_equal_to_regenerated_buf_output\n" +
        "    FAIL [   0.207s] verter_protocol::main cases::typeinfo_proto_ts_freshness::proto_ts_bindings_byte_pinned_repo_wide\n" +
        "     Summary [  62.968s] 15543 tests run: 15541 passed, 2 failed, 547 skipped\n",
    );
    const classify = (file) => verdictClassifyNextestFile(file);
    const classifyRun = (code, file) => verdictNextestRunFile(code, file);
    let ok = true;
    // Classifier (no exit code) — classifies the LOG CONTENT. SIGABRT/LEAK carry a content signal (a
    // non-`FAIL` status line + a summary failure count) so the no-code classifier catches them. A pure
    // setup/harness error has NO content markers and is indistinguishable from a clean log WITHOUT the exit
    // code, so it is asserted ONLY on the live-aggregation hook below (which has the code).
    const cSig = classify(sigabrt);
    const cLeak = classify(leakPlusTolerated);
    const cTol = classify(tolerated);
    note(`classify: sigabrt=${cSig} leak+tol=${cLeak} tolerated=${cTol}`);
    if (cSig !== "FAIL") {
      fail(
        `(ix) classifier: SIGABRT crash => '${cSig}', expected FAIL (a non-FAIL status must not pass)`,
      );
      ok = false;
    }
    if (cLeak !== "FAIL") {
      fail(`(ix) classifier: tolerated-FAIL + unaccounted LEAK => '${cLeak}', expected FAIL`);
      ok = false;
    }
    if (cTol !== "PASS-WITH-TOLERATED") {
      fail(
        `(ix) classifier: tolerated baseline => '${cTol}', expected PASS-WITH-TOLERATED (discrimination)`,
      );
      ok = false;
    }
    // LIVE-aggregation hook (with exit code) — the exact code runGate runs. nextest exits 100 on test
    // failures, non-100 on internal errors; a crash run is non-zero either way. This path consults the run
    // exit code, so it ALSO catches a content-less setup/harness error (nonzero exit, no FAIL line).
    const rSig = classifyRun(101, sigabrt);
    const rLeak = classifyRun(100, leakPlusTolerated);
    const rSetup = classifyRun(1, setupError);
    const rTol = classifyRun(100, tolerated);
    note(`live-agg: sigabrt=${rSig} leak+tol=${rLeak} setup-error=${rSetup} tolerated=${rTol}`);
    if (rSig !== "FAIL") {
      fail(`(ix) live-agg: SIGABRT (exit 101) => '${rSig}', expected FAIL`);
      ok = false;
    }
    if (rLeak !== "FAIL") {
      fail(`(ix) live-agg: tolerated-FAIL + LEAK (exit 100) => '${rLeak}', expected FAIL`);
      ok = false;
    }
    if (rSetup !== "FAIL") {
      fail(`(ix) live-agg: setup error (exit 1, no FAIL line) => '${rSetup}', expected FAIL`);
      ok = false;
    }
    if (rTol !== "PASS-WITH-TOLERATED") {
      fail(
        `(ix) live-agg: tolerated baseline (exit 100) => '${rTol}', expected PASS-WITH-TOLERATED`,
      );
      ok = false;
    }
    if (ok) {
      pass(
        "(ix) SURFACE-1: SIGABRT crash, unaccounted LEAK, and setup/harness error ALL => FAIL on both the " +
          "classifier and the live-aggregation hook; tolerated baseline => PASS-WITH-TOLERATED (discriminating)",
      );
    }
  }

  // --------------------------------------------------------------------------------------------------
  // (x) FAIL-CLOSED MUTEX vs an alive holder with EMPTY/uncheckable start-identity. We hand-craft a
  //     lockdir whose owner.json names the HARNESS's own (alive) pid with an EMPTY processStartIdentity,
  //     then run a gate against it. PID reuse is NOT proven (no stored identity to compare), so the gate
  //     MUST REFUSE (126), never reclaim a live lock. Pre-fix: an empty stored identity fell through to
  //     "identity mismatch" and reclaimed the LIVE lock (two concurrent gates). A control case — a lock
  //     owned by a DEFINITELY-DEAD pid — must still reclaim+PASS, so the refusal is specific to the alive
  //     holder, not a blanket refusal (discrimination).
  // --------------------------------------------------------------------------------------------------
  process.stderr.write("\n(x) FAIL-CLOSED MUTEX (alive holder, empty start-identity)\n");
  posix_x: {
    if (IS_WINDOWS) {
      skip(
        "(x) FAIL-CLOSED MUTEX — POSIX pid liveness + process spawn (CIM start-identity path is static-reviewed)",
      );
      break posix_x;
    }
    let ok = true;
    // A DEAD pid number for the control/reclaim cases (a very high, almost-certainly-unused pid).
    let deadCandidate = 999_999;
    while (pidAlive(deadCandidate) && deadCandidate > 100_000) deadCandidate--;
    // Case A: alive pid (this harness) + empty identity, in a GATE-OWNED lockdir (sentinel + matching repo)
    // => REFUSE (126). The sentinel + matching repo make this a genuine fail-closed-IDENTITY refusal, not a
    // sentinel/foreign-repo refusal — so it proves the identity rule specifically.
    const lkA = freshLock();
    mkdirSync(lkA, { recursive: true });
    writeSentinel(lkA, "crafted-live-empty-ident", REPO_REALPATH);
    writeFileSync(
      join(lkA, "owner.json"),
      JSON.stringify({
        token: "crafted-live-empty-ident",
        pid: process.pid, // the harness itself — definitely alive
        repoRealpath: REPO_REALPATH, // matches => not a foreign-repo refusal
        targetDir: freshTmpDir("gatetest-target-"),
        createdAtMs: Date.now() - 60_000, // older than the init grace so it is not treated as "initializing"
        processStartIdentity: "", // EMPTY — PID reuse cannot be proven
      }),
    );
    const tgtA = freshTmpDir("gatetest-target-");
    const rA = runContainedCmd(`echo never_runs_${RUN_TAG}`, {
      VERTER_GATE_LOCK: lkA,
      VERTER_GATE_TARGET_DIR: tgtA,
    });
    note(
      `alive-pid + empty-identity holder (gate-owned) => rc=${rA.code} (expect LOCK-REFUSED ${EXIT_LOCK_REFUSED})`,
    );
    if (rA.code !== EXIT_LOCK_REFUSED) {
      fail(
        `(x) FAIL-CLOSED: alive holder with empty start-identity returned ${rA.code}, expected LOCK-REFUSED ` +
          `(${EXIT_LOCK_REFUSED}) — an empty/uncheckable identity must NEVER be read as proof of PID reuse`,
      );
      ok = false;
    }
    // The crafted lockdir must SURVIVE (the gate refused, it did NOT reclaim a live lock).
    if (!existsSync(lkA)) {
      fail(
        "(x) FAIL-CLOSED: the live holder's lockdir was RECLAIMED (vanished) — fail-open regression",
      );
      ok = false;
    }
    rmSync(dirname(lkA), { recursive: true, force: true });
    // Case B (control / discrimination): a DEAD pid + empty identity, in a GATE-OWNED lockdir (sentinel +
    // matching repo) => reclaim + PASS. Proves the fail-closed rule is not a blanket refusal — a genuinely
    // dead, gate-owned lock is still reclaimable.
    const lkB = freshLock();
    mkdirSync(lkB, { recursive: true });
    writeSentinel(lkB, "crafted-dead", REPO_REALPATH);
    writeFileSync(
      join(lkB, "owner.json"),
      JSON.stringify({
        token: "crafted-dead",
        pid: deadCandidate,
        repoRealpath: REPO_REALPATH, // matches => reclaimable by THIS repo
        targetDir: freshTmpDir("gatetest-target-"),
        createdAtMs: Date.now() - 60_000,
        processStartIdentity: "",
      }),
    );
    note(
      `control: dead-pid ${deadCandidate} (alive=${pidAlive(deadCandidate)}) + empty identity, gate-owned => expect reclaim+PASS`,
    );
    const tgtB = freshTmpDir("gatetest-target-");
    const rB = runContainedCmd(`echo reclaimed_x_${RUN_TAG}`, {
      VERTER_GATE_LOCK: lkB,
      VERTER_GATE_TARGET_DIR: tgtB,
    });
    note(`dead-pid holder (gate-owned) => rc=${rB.code} (expect PASS ${EXIT_PASS})`);
    if (rB.code !== EXIT_PASS) {
      fail(
        `(x) CONTROL: a DEAD-holder lock with empty identity returned ${rB.code}, expected PASS (${EXIT_PASS}) ` +
          "— the fail-closed rule must NOT blanket-refuse; a dead, gate-owned lock is still reclaimable",
      );
      ok = false;
    }
    rmSync(dirname(lkB), { recursive: true, force: true });
    if (ok) {
      pass(
        "(x) FAIL-CLOSED MUTEX: alive holder + empty start-identity => REFUSED (126) and the lock survived; " +
          "a dead, gate-owned holder + empty identity => reclaimed + PASSed (discriminating, not a blanket refusal)",
      );
    }
  }

  // --------------------------------------------------------------------------------------------------
  // (x-safe) SAFE LOCK RECLAIM — never rm an arbitrary non-gate directory. The hard A3 invariant: a lockdir
  //          is renamed/removed ONLY if it carries the gate-owned sentinel (proving the gate created it).
  //          Two discriminating cases:
  //            (a) a DEAD-holder lockdir with a valid owner.json but NO sentinel — models a mis-set
  //                VERTER_GATE_LOCK pointing at a pre-existing dir that merely LOOKS like a lock. The gate
  //                MUST REFUSE (126) and the dir MUST SURVIVE (never deleted), even though the holder is
  //                dead and old. Pre-fix: the dead+old dir was renamed+rm'd (arbitrary directory deletion).
  //            (b) a DEAD-holder lockdir with a sentinel but a FOREIGN repoRealpath — models a different
  //                checkout sharing a lock path. The gate MUST REFUSE (126) and the dir MUST SURVIVE (it is
  //                not ours to delete). Both place a real decoy file inside the dir and assert it remains.
  // --------------------------------------------------------------------------------------------------
  process.stderr.write("\n(x-safe) SAFE LOCK RECLAIM (never delete a non-gate / foreign dir)\n");
  posix_xsafe: {
    if (IS_WINDOWS) {
      skip(
        "(x-safe) SAFE LOCK RECLAIM — POSIX pid liveness + process spawn (the reclaim logic itself is platform-shared; this exercises the runner subprocess)",
      );
      break posix_xsafe;
    }
    let ok = true;
    let deadCandidate = 999_999;
    while (pidAlive(deadCandidate) && deadCandidate > 100_000) deadCandidate--;

    // (a) Dead holder, valid owner.json, NO sentinel => REFUSE + SURVIVE (the headline A3 case).
    const lkNoSentinel = freshLock();
    mkdirSync(lkNoSentinel, { recursive: true });
    const decoyA = join(lkNoSentinel, "DO_NOT_DELETE_me.txt");
    writeFileSync(decoyA, "a precious pre-existing file a mis-set env var must never delete\n");
    writeFileSync(
      join(lkNoSentinel, "owner.json"),
      JSON.stringify({
        token: "crafted-no-sentinel",
        pid: deadCandidate, // dead
        repoRealpath: REPO_REALPATH, // even with a MATCHING repo, the missing sentinel must block reclaim
        targetDir: freshTmpDir("gatetest-target-"),
        createdAtMs: Date.now() - 60_000, // old (past the init grace)
        processStartIdentity: "",
      }),
    );
    const tgtNS = freshTmpDir("gatetest-target-");
    const rNS = runContainedCmd(`echo never_runs_${RUN_TAG}`, {
      VERTER_GATE_LOCK: lkNoSentinel,
      VERTER_GATE_TARGET_DIR: tgtNS,
    });
    note(
      `dead holder, NO sentinel => rc=${rNS.code} (expect LOCK-REFUSED ${EXIT_LOCK_REFUSED}); dir survives?`,
    );
    if (rNS.code !== EXIT_LOCK_REFUSED) {
      fail(
        `(x-safe a) a dead holder lockdir WITHOUT the gate sentinel returned ${rNS.code}, expected ` +
          `LOCK-REFUSED (${EXIT_LOCK_REFUSED}) — an unmarked directory must NEVER be reclaimed/deleted`,
      );
      ok = false;
    }
    if (!existsSync(lkNoSentinel) || !existsSync(decoyA)) {
      fail(
        `(x-safe a) the unmarked lockdir was DELETED (dir=${existsSync(lkNoSentinel)} decoy=${existsSync(decoyA)}) ` +
          "— A3 violation: the gate rm'd an arbitrary directory it did not create",
      );
      ok = false;
    }
    rmSync(dirname(lkNoSentinel), { recursive: true, force: true });

    // (b) Dead holder, sentinel present, FOREIGN repoRealpath => REFUSE + SURVIVE.
    const lkForeign = freshLock();
    mkdirSync(lkForeign, { recursive: true });
    writeSentinel(lkForeign, "crafted-foreign", "/some/other/checkout/of/verter");
    const decoyB = join(lkForeign, "owned_by_other_repo.txt");
    writeFileSync(decoyB, "another checkout's lock — not ours to delete\n");
    writeFileSync(
      join(lkForeign, "owner.json"),
      JSON.stringify({
        token: "crafted-foreign",
        pid: deadCandidate, // dead
        repoRealpath: "/some/other/checkout/of/verter", // FOREIGN — not this repo
        targetDir: freshTmpDir("gatetest-target-"),
        createdAtMs: Date.now() - 60_000,
        processStartIdentity: "",
      }),
    );
    const tgtF = freshTmpDir("gatetest-target-");
    const rF = runContainedCmd(`echo never_runs_${RUN_TAG}`, {
      VERTER_GATE_LOCK: lkForeign,
      VERTER_GATE_TARGET_DIR: tgtF,
    });
    note(
      `dead holder, sentinel + FOREIGN repo => rc=${rF.code} (expect LOCK-REFUSED ${EXIT_LOCK_REFUSED})`,
    );
    if (rF.code !== EXIT_LOCK_REFUSED) {
      fail(
        `(x-safe b) a dead foreign-repo lockdir returned ${rF.code}, expected LOCK-REFUSED ` +
          `(${EXIT_LOCK_REFUSED}) — another repo's lock is not ours to reclaim/delete`,
      );
      ok = false;
    }
    if (!existsSync(lkForeign) || !existsSync(decoyB)) {
      fail(
        `(x-safe b) the foreign-repo lockdir was DELETED (dir=${existsSync(lkForeign)} decoy=${existsSync(decoyB)}) ` +
          "— A3 violation: the gate rm'd another repo's lock",
      );
      ok = false;
    }
    rmSync(dirname(lkForeign), { recursive: true, force: true });

    if (ok) {
      pass(
        "(x-safe) SAFE RECLAIM: a dead-holder dir WITHOUT the gate sentinel and a dead foreign-repo dir " +
          "BOTH => REFUSED (126) and SURVIVED (never deleted); only a gate-owned same-repo dir is reclaimed",
      );
    }
  }

  // --------------------------------------------------------------------------------------------------
  // (P-1) FOREIGN-SENTINEL RECLAIM REFUSE ON THE owner==null PATH (HEADLINE A3 HOLE). A lockdir carrying
  //       ONLY the gate sentinel — whose stored repo realpath is FOREIGN (differs from ours) — and NO
  //       owner.json, PAST the init grace (so it is treated as a crashed mid-init lock, the `_reclaim(null)`
  //       path), must be REFUSED (126) and its decoy file MUST SURVIVE. This is the exact hole the prior
  //       code had: `_reclaim(null)` keyed only on the sentinel's PRESENCE (proving the gate created some
  //       dir) but did NOT validate the sentinel's stored repo realpath, so past the init grace it
  //       renamed/removed a FOREIGN checkout's mid-init sentinel-only lock. The fix validates the sentinel
  //       repo realpath on EVERY reclaim path including owner==null. We craft a foreign sentinel, NO
  //       owner.json, and backdate the lockdir mtime past the init grace, then assert REFUSE + SURVIVE.
  //       Discriminating control (already covered by scenario (ii)): a SAME-REPO sentinel-only crashed
  //       mid-init lock IS reclaimable — so this is not a blanket refusal of all owner-less locks.
  // --------------------------------------------------------------------------------------------------
  process.stderr.write(
    "\n(P-1) FOREIGN-SENTINEL reclaim refuse on the owner==null path (no owner.json)\n",
  );
  posix_p1: {
    if (IS_WINDOWS) {
      skip(
        "(P-1) FOREIGN-SENTINEL owner-less reclaim refuse — uses POSIX touch -t + the runner subprocess (the sentinel-repo validation is platform-shared)",
      );
      break posix_p1;
    }
    let ok = true;
    // A foreign-sentinel lockdir with NO owner.json. To reach the `_reclaim(null)` path (not the init-grace
    // refusal), the lockdir's mtime must be OLDER than the 5s init grace. We create it, write the foreign
    // sentinel + a decoy, then backdate the dir's mtime well past the grace via `touch -t`.
    const lkForeignNoOwner = freshLock();
    mkdirSync(lkForeignNoOwner, { recursive: true });
    // Sentinel stores a FOREIGN repo realpath (NOT this checkout). NO owner.json is written.
    writeSentinel(lkForeignNoOwner, "crafted-foreign-no-owner", "/some/other/checkout/of/verter");
    const decoyFN = join(lkForeignNoOwner, "DECOY_foreign_mid_init.txt");
    writeFileSync(decoyFN, "a foreign checkout's mid-init lock — never ours to delete\n");
    // Backdate the lockdir mtime past the 5s init grace so the gate treats it as crashed-mid-init, not
    // initializing. `touch -d '60 seconds ago'` (GNU) / `touch -A -000100` is non-portable; use a fixed old
    // timestamp via `touch -t` (POSIX: [[CC]YY]MMDDhhmm[.ss]). 2020-01-01 00:00 is comfortably past grace.
    try {
      execFileSync("touch", ["-t", "202001010000.00", lkForeignNoOwner], { stdio: "ignore" });
    } catch {
      // Fallback: if `touch -t` is unavailable, the dir was just created (age < grace) and the gate would
      // refuse with the init-grace reason instead — still a refusal (126), still no deletion, so the
      // SURVIVE assertion holds; only the "reached the _reclaim(null) path" nuance is softened.
      note("touch -t unavailable; relying on init-grace refusal (still 126 + survive)");
    }
    const tgtFN = freshTmpDir("gatetest-target-");
    const rFN = runContainedCmd(`echo never_runs_${RUN_TAG}`, {
      VERTER_GATE_LOCK: lkForeignNoOwner,
      VERTER_GATE_TARGET_DIR: tgtFN,
    });
    note(
      `foreign sentinel + NO owner.json + past init-grace => rc=${rFN.code} (expect LOCK-REFUSED ${EXIT_LOCK_REFUSED}); decoy survives?`,
    );
    if (rFN.code !== EXIT_LOCK_REFUSED) {
      fail(
        `(P-1) a FOREIGN-sentinel, owner-less, past-grace lockdir returned ${rFN.code}, expected LOCK-REFUSED ` +
          `(${EXIT_LOCK_REFUSED}) — the owner==null reclaim path must validate the sentinel repo realpath and ` +
          "refuse a foreign checkout's mid-init lock, never delete it",
      );
      ok = false;
    }
    if (!existsSync(lkForeignNoOwner) || !existsSync(decoyFN)) {
      fail(
        `(P-1) the foreign owner-less lockdir was DELETED (dir=${existsSync(lkForeignNoOwner)} ` +
          `decoy=${existsSync(decoyFN)}) — A3 hole: \`_reclaim(null)\` renamed/removed a foreign checkout's ` +
          "mid-init sentinel-only lock",
      );
      ok = false;
    }
    rmSync(dirname(lkForeignNoOwner), { recursive: true, force: true });
    if (ok) {
      pass(
        "(P-1) FOREIGN-SENTINEL owner-less REFUSE: a foreign-repo sentinel-only lockdir with NO owner.json, " +
          "past the init grace, => REFUSED (126) and its decoy SURVIVED (the owner==null reclaim path validates " +
          "the sentinel repo realpath; a foreign mid-init lock is never reclaimed/deleted)",
      );
    }
  }

  // --------------------------------------------------------------------------------------------------
  // (xi) SURFACE-2 ZERO-SUITES / PARTIAL-FILTER SETUP GATE. If the verter_session lib/test suite filter
  //      finds nothing (a filter regression / archive-shape change) the gate must FAIL SETUP (127), NOT
  //      pass on surface 1 alone. We drive the REAL selectSessionSuites() gate IN-PROCESS. Zero session
  //      suites => 127; a lib-only filter (missing the integration `test` kind) => 127; a proper 1-lib +
  //      N-test listing => OK/0 (discrimination). Pre-fix: runGate had NO zero-suite guard — an empty
  //      filter produced an empty loop and reached the green aggregate verdict.
  // --------------------------------------------------------------------------------------------------
  process.stderr.write("\n(xi) SURFACE-2 zero-suites / partial-filter setup gate\n");
  {
    const fixDir = freshTmpDir("gatetest-s2fix-");
    const zero = join(fixDir, "zero.json");
    const libOnly = join(fixDir, "lib_only.json");
    const proper = join(fixDir, "proper.json");
    writeFileSync(
      zero,
      JSON.stringify([
        { "package-name": "verter_compiler", kind: "lib" },
        { "package-name": "verter_other", kind: "test" },
      ]),
    );
    writeFileSync(
      libOnly,
      JSON.stringify([
        { "package-name": "verter_session", kind: "lib" },
        { "package-name": "verter_session", kind: "bin" },
      ]),
    );
    writeFileSync(
      proper,
      JSON.stringify([
        { "package-name": "verter_session", kind: "lib" },
        { "package-name": "verter_session", kind: "test" },
        { "package-name": "verter_session", kind: "test" },
        { "package-name": "verter_session", kind: "bin" },
      ]),
    );
    const surface2 = (file) => verdictSurface2(JSON.parse(readFileSync(file, "utf8")));
    let ok = true;
    const zr = surface2(zero);
    const lr = surface2(libOnly);
    const pr = surface2(proper);
    note(
      `zero-suites rc=${zr.code} (expect 127); lib-only rc=${lr.code} (expect 127); proper rc=${pr.code} out='${pr.out}' (expect 0)`,
    );
    if (zr.code !== 127) {
      fail(`(xi) zero verter_session suites returned ${zr.code}, expected SETUP failure (127)`);
      ok = false;
    }
    if (lr.code !== 127) {
      fail(
        `(xi) lib-only (no integration test kind) returned ${lr.code}, expected SETUP failure (127)`,
      );
      ok = false;
    }
    if (pr.code !== 0) {
      fail(
        `(xi) a proper 1-lib + 2-test listing returned ${pr.code}, expected OK (0) — over-strict regression`,
      );
      ok = false;
    }
    if (ok) {
      pass(
        "(xi) SURFACE-2 GATE: zero session suites => 127, lib-only (missing test kind) => 127, " +
          "1-lib+2-test => 0 (discriminating; a silent surface-2 skip is now a SETUP failure)",
      );
    }
  }

  // --------------------------------------------------------------------------------------------------
  // (xii) WINDOWS `.exe` PROVENANCE-SWEEP MATCHER (pure regex/classify unit — exercised on this POSIX host
  //       via the matcher's `windows` flag; NO real Windows needed). The sweep predicate is
  //       `isBuildTool(cmd) && targetDirMatches(cmd, targetDir, windows)`. A Windows `rustc.exe` /
  //       `cargo-nextest.exe` command line referencing the RUNNER target dir MUST MATCH (be swept); a
  //       repo-root-only dev `cargo.exe` MUST NOT (be spared). Pre-fix: the matcher required whitespace/end
  //       after the tool name, so `rustc.exe` / `cargo-nextest.exe` never matched — a runner-owned build
  //       child on Windows escaped the sweep.
  // --------------------------------------------------------------------------------------------------
  process.stderr.write("\n(xii) Windows .exe provenance-sweep matcher (pure unit)\n");
  {
    const RT = "C:\\Users\\dev\\repo\\target\\gate-runner";
    const sweep = (plat, targetDir, cmd) => verdictSweepMatch(plat, targetDir, cmd);
    // The discriminating positives: standalone rustc.exe / cargo-nextest.exe (pre-fix NOMATCH) referencing
    // the runner target dir.
    const rustcExe = sweep(
      "windows",
      RT,
      "C:\\Users\\dev\\.rustup\\toolchains\\stable\\bin\\rustc.exe --out-dir C:\\Users\\dev\\repo\\target\\gate-runner\\debug\\deps lib.rs",
    );
    const nextestExe = sweep(
      "windows",
      RT,
      "cargo-nextest.exe run --target-dir C:\\Users\\dev\\repo\\target\\gate-runner",
    );
    // The mixed-case runner-target reference (Windows case-insensitive) must still match.
    const mixedCase = sweep(
      "windows",
      RT,
      "C:\\USERS\\DEV\\.CARGO\\BIN\\CARGO.EXE NEXTEST RUN --TARGET-DIR C:\\USERS\\DEV\\REPO\\TARGET\\GATE-RUNNER",
    );
    // The negative: a dev cargo.exe that references ONLY the repo root (NOT the runner target) must be
    // spared.
    const devRepoOnly = sweep(
      "windows",
      RT,
      "C:\\Users\\dev\\.cargo\\bin\\cargo.exe build --manifest-path C:\\Users\\dev\\repo\\Cargo.toml",
    );
    // An unrelated cargocult.exe referencing the runner target must NOT match (word-boundary holds).
    const cargocult = sweep(
      "windows",
      RT,
      "C:\\tools\\cargocult.exe --out C:\\Users\\dev\\repo\\target\\gate-runner",
    );
    note(
      `rustc.exe=${rustcExe} cargo-nextest.exe=${nextestExe} mixed-case=${mixedCase} dev-repo-only=${devRepoOnly} cargocult.exe=${cargocult}`,
    );
    let ok = true;
    if (rustcExe !== "MATCH") {
      fail(
        `(xii) Windows standalone rustc.exe + runner target => '${rustcExe}', expected MATCH (.exe suffix)`,
      );
      ok = false;
    }
    if (nextestExe !== "MATCH") {
      fail(
        `(xii) Windows cargo-nextest.exe + runner target => '${nextestExe}', expected MATCH (.exe suffix)`,
      );
      ok = false;
    }
    if (mixedCase !== "MATCH") {
      fail(
        `(xii) Windows mixed-case CARGO.EXE + runner target => '${mixedCase}', expected MATCH (case-normalized)`,
      );
      ok = false;
    }
    if (devRepoOnly !== "NOMATCH") {
      fail(
        `(xii) Windows dev cargo.exe referencing ONLY the repo root => '${devRepoOnly}', expected NOMATCH (spared)`,
      );
      ok = false;
    }
    if (cargocult !== "NOMATCH") {
      fail(
        `(xii) Windows cargocult.exe => '${cargocult}', expected NOMATCH (word-boundary must hold)`,
      );
      ok = false;
    }
    if (ok) {
      pass(
        "(xii) WINDOWS .exe SWEEP: rustc.exe / cargo-nextest.exe / mixed-case CARGO.EXE referencing the " +
          "runner target MATCH; a repo-root-only dev cargo.exe and cargocult.exe do NOT (discriminating)",
      );
    }
  }

  // --------------------------------------------------------------------------------------------------
  // (xiii) WATCHDOG REASON SURVIVES A TRAPPED-SIGTERM EXIT-0. The whole-gate reap sends SIGTERM before
  //        SIGKILL; a process can TRAP SIGTERM and exit(0). A REAL timeout that fires the reap, hits a LIVE
  //        process group, and is then trapped-exit-0'd MUST still report TIMEOUT (124) — the trapped clean
  //        exit must NOT mask the watchdog verdict. We run a custom command that installs `trap 'exit 0'
  //        TERM`, backgrounds an argv-tagged `sleep 600`, and waits; under `--timeout 4s` the watchdog
  //        fires, signals the live group, bash traps SIGTERM and exits 0 (within ~1 trap latency). The gate
  //        MUST return 124. Pre-fix (the `code === 0` clears `reason` logic) returned 0 — a real timeout
  //        masked as PASS. We assert rc=124 (discriminates) AND that the tagged sleep was reaped.
  // --------------------------------------------------------------------------------------------------
  process.stderr.write("\n(xiii) WATCHDOG reason survives a trapped-SIGTERM exit-0\n");
  posix_xiii: {
    if (IS_WINDOWS) {
      skip(
        "(xiii) WATCHDOG trapped-SIGTERM-exit-0 — POSIX SIGTERM trap + sleep stand-in (no Windows equivalent)",
      );
      break posix_xiii;
    }
    const lk = freshLock();
    const tgt = freshTmpDir("gatetest-target-");
    // bash installs a TERM trap that exits 0, backgrounds an argv-tagged sleep 600, and waits for it. On
    // the watchdog's SIGTERM the wait returns, the trap runs `exit 0` — a clean exit AFTER a real timeout.
    const trapCmd =
      `trap 'exit 0' TERM; ( exec -a sleep_${RUN_TAG}_trapsig sleep 600 ) & ` + `c=$!; wait $c`;
    const t0 = Date.now();
    const r = runContainedCmd(trapCmd, { VERTER_GATE_LOCK: lk, VERTER_GATE_TARGET_DIR: tgt }, [
      "--timeout",
      "4s",
      "--stall",
      "600s",
    ]);
    const elapsed = Math.round((Date.now() - t0) / 1000);
    await delay(2000);
    let ok = true;
    if (r.code !== EXIT_TIMEOUT) {
      fail(
        `(xiii) trapped-SIGTERM-exit-0 returned ${r.code}, expected TIMEOUT (${EXIT_TIMEOUT}) — a process that ` +
          `traps SIGTERM and exit(0)s after a REAL timeout must NOT mask the watchdog verdict (pre-fix masked it to 0)`,
      );
      ok = false;
    }
    if (elapsed >= 60) {
      fail(`(xiii) took ${elapsed}s — the watchdog did not bound the trapped run near 4s`);
      ok = false;
    }
    const trapLeft = countArgvSleeps(`${RUN_TAG}_trapsig`);
    if (trapLeft !== 0) {
      fail(
        `(xiii) ${trapLeft} argv-tagged sleep survived the trapped-SIGTERM reap — the group was not torn down`,
      );
      spawnSync("pkill", ["-9", "-f", `sleep_${RUN_TAG}_trapsig`], { stdio: "ignore" });
      ok = false;
    }
    if (ok) {
      pass(
        `(xiii) WATCHDOG: a trapped-SIGTERM exit-0 after a real --timeout still reports TIMEOUT (124) in ` +
          `~${elapsed}s and the group was reaped (the verdict is keyed on a signaled-LIVE reap, not on code===0)`,
      );
    }
    // teardown.
    for (let w = 0; w < 40; w++) {
      if (!existsSync(lk)) break;
      await delay(100);
    }
    const left = countArgvSleeps(`${RUN_TAG}_trapsig`);
    if (!existsSync(lk) && left === 0) {
      pass(
        "(v/xiii) TEARDOWN: lockdir released and 0 stray tagged sleeps after trapped-SIGTERM test",
      );
    } else {
      fail(`(v/xiii) TEARDOWN: lockdir-exists=${existsSync(lk)} stray_sleeps=${left}`);
    }
  }

  // --------------------------------------------------------------------------------------------------
  // (xiv) SURFACE-1 SUMMARY-REQUIRED FAILURE ACCOUNTING. A non-zero nextest exit must be EXACTLY accounted
  //       for by a PARSED `Summary` line, not merely by the tolerated `FAIL [` names. With one or two
  //       tolerated `FAIL [` lines and a MISSING/unparseable Summary, the prior `unaccounted =
  //       summary.failed - failNames.length` went NEGATIVE (summary.failed defaulted to 0) so the tripwire
  //       never fired and the gate returned PASS-WITH-TOLERATED — swallowing whatever caused the non-zero
  //       exit. The fix: `parseNextestSummary()` reports `found`, and a non-zero exit FAILS unless a Summary
  //       was found AND its `failed` count EXACTLY equals the parsed `FAIL` name count. We drive the LIVE
  //       aggregation hook (which has the exit code). Discriminators: a tolerated `FAIL` + NO Summary at
  //       exit 100 and exit 1 => FAIL (pre-fix PASS-WITH-TOLERATED); the WITH-Summary tolerated baseline
  //       still => PASS-WITH-TOLERATED (no over-strict regression); and a CODE-0 no-Summary tolerated log
  //       stays PASS-WITH-TOLERATED (a clean run is never forced to FAIL by the new requirement).
  // --------------------------------------------------------------------------------------------------
  process.stderr.write("\n(xiv) SURFACE-1 summary-required failure accounting\n");
  {
    const fixDir = freshTmpDir("gatetest-s1sum-");
    const tolNoSummary = join(fixDir, "tolerated_no_summary.log");
    const tolWithSummary = join(fixDir, "tolerated_with_summary.log");
    // One tolerated FAIL line, NO Summary line. A non-zero exit cannot be proven accounted-for => FAIL.
    writeFileSync(
      tolNoSummary,
      "    FAIL [   0.012s] verter_protocol::main cases::typeinfo_proto_ts_freshness::typeinfo_ts_bindings_are_byte_equal_to_regenerated_buf_output\n",
    );
    // The same tolerated FAIL line WITH a matching Summary (failed=1 == 1 parsed FAIL name) => accounted =>
    // PASS-WITH-TOLERATED. Proves the requirement is summary-PRESENCE + exact-count, not a blanket fail.
    writeFileSync(
      tolWithSummary,
      "    FAIL [   0.012s] verter_protocol::main cases::typeinfo_proto_ts_freshness::typeinfo_ts_bindings_are_byte_equal_to_regenerated_buf_output\n" +
        "     Summary [  62.968s] 15543 tests run: 15542 passed, 1 failed, 547 skipped\n",
    );
    const classifyRun = (code, file) => verdictNextestRunFile(code, file);
    let ok = true;
    const rNoSum100 = classifyRun(100, tolNoSummary);
    const rNoSum1 = classifyRun(1, tolNoSummary);
    const rWithSum = classifyRun(100, tolWithSummary);
    const rNoSum0 = classifyRun(0, tolNoSummary);
    note(
      `tolerated+no-Summary: exit100=${rNoSum100} exit1=${rNoSum1}; tolerated+Summary exit100=${rWithSum}; ` +
        `tolerated+no-Summary exit0=${rNoSum0}`,
    );
    if (rNoSum100 !== "FAIL") {
      fail(
        `(xiv) tolerated FAIL + NO Summary at exit 100 => '${rNoSum100}', expected FAIL — a non-zero exit with no ` +
          `parseable Summary cannot prove the failures are accounted for (pre-fix swallowed it as PASS-WITH-TOLERATED)`,
      );
      ok = false;
    }
    if (rNoSum1 !== "FAIL") {
      fail(`(xiv) tolerated FAIL + NO Summary at exit 1 => '${rNoSum1}', expected FAIL`);
      ok = false;
    }
    if (rWithSum !== "PASS-WITH-TOLERATED") {
      fail(
        `(xiv) tolerated FAIL + matching Summary (failed=1) at exit 100 => '${rWithSum}', expected ` +
          `PASS-WITH-TOLERATED — an exact summary-accounted tolerated failure must still tolerate (no over-strict regression)`,
      );
      ok = false;
    }
    if (rNoSum0 !== "PASS-WITH-TOLERATED") {
      fail(
        `(xiv) tolerated FAIL + NO Summary at exit 0 => '${rNoSum0}', expected PASS-WITH-TOLERATED — a CLEAN ` +
          `(exit 0) run is never forced to FAIL by the summary requirement (the requirement gates non-zero exits only)`,
      );
      ok = false;
    }
    if (ok) {
      pass(
        "(xiv) SURFACE-1 SUMMARY-REQUIRED: tolerated FAIL + missing Summary at non-zero exit (100 and 1) => FAIL; " +
          "a summary-accounted tolerated failure and a clean exit-0 stay PASS-WITH-TOLERATED (discriminating)",
      );
    }
  }

  // --------------------------------------------------------------------------------------------------
  // (xv) WINDOWS SWEEP: QUOTED EXECUTABLE PATHS + PATH-TOKEN TARGET MATCH. The provenance sweep predicate
  //      `isBuildTool(cmd) && targetDirMatches(cmd, targetDir, windows)` must (a) recognize a QUOTED full
  //      path to a build tool — `"C:\Users\Name With Space\.cargo\bin\cargo.exe" …` (the standard Windows
  //      form when the path has spaces) — where the opening `"` blocked the prior boundary class, and (b)
  //      match the runner target dir on a path-SEGMENT boundary so a SIBLING `…\target\gate-runner2` does
  //      NOT spuriously match `…\target\gate-runner`. Pure regex/classify unit on this POSIX host via the
  //      matcher's `windows` flag. Discriminators (each FAILs against the round-1 raw-includes /
  //      no-quote-boundary code): quoted cargo.exe / rustc.exe with the ONLY build-tool token quoted =>
  //      MATCH; sibling gate-runner2 => NOMATCH.
  // --------------------------------------------------------------------------------------------------
  process.stderr.write("\n(xv) Windows sweep: quoted exec paths + path-token target match\n");
  {
    const RT = "C:\\Users\\dev\\repo\\target\\gate-runner";
    const sweep = (plat, targetDir, cmd) => verdictSweepMatch(plat, targetDir, cmd);
    // (a) Quoted cargo.exe — the ONLY build-tool token is the quoted exe (the closing `"` follows it, and
    //     `.cargo` in the path is `.`-prefixed so it is not a bare token). Pre-fix: NOMATCH (the opening `"`
    //     blocked the boundary). Post-fix: MATCH.
    const quotedCargo = sweep(
      "windows",
      RT,
      '"C:\\Users\\Name With Space\\.cargo\\bin\\cargo.exe" --version C:\\Users\\dev\\repo\\target\\gate-runner\\debug',
    );
    // (b) Quoted rustc.exe referencing the runner target deps. Pre-fix: NOMATCH. Post-fix: MATCH.
    const quotedRustc = sweep(
      "windows",
      RT,
      '"C:\\Users\\dev\\.rustup\\toolchains\\stable\\bin\\rustc.exe" --out-dir C:\\Users\\dev\\repo\\target\\gate-runner\\debug\\deps lib.rs',
    );
    // (c) Sibling runner target `gate-runner2` — a DIFFERENT runner's dir whose path CONTAINS `gate-runner`
    //     as a prefix. Pre-fix raw `includes`: MATCH (false positive — would sweep an unrelated runner).
    //     Post-fix path-token boundary: NOMATCH.
    const siblingDir = sweep(
      "windows",
      RT,
      "cargo.exe build --target-dir C:\\Users\\dev\\repo\\target\\gate-runner2\\debug",
    );
    // (d) POSIX sibling-dir false positive must ALSO be closed (the boundary check is platform-shared).
    const posixSibling = sweep(
      "posix",
      "/home/dev/repo/target/gate-runner",
      "cargo build --target-dir /home/dev/repo/target/gate-runner2/debug",
    );
    // (e) Discrimination floor: the EXACT runner target dir still matches (we did not break the positive).
    const exactDir = sweep(
      "windows",
      RT,
      "cargo-nextest.exe run --target-dir C:\\Users\\dev\\repo\\target\\gate-runner",
    );
    note(
      `quoted-cargo=${quotedCargo} quoted-rustc=${quotedRustc} sibling=${siblingDir} ` +
        `posix-sibling=${posixSibling} exact=${exactDir}`,
    );
    let ok = true;
    if (quotedCargo !== "MATCH") {
      fail(
        `(xv) quoted "…\\cargo.exe" => '${quotedCargo}', expected MATCH (a quote is an exec-name boundary)`,
      );
      ok = false;
    }
    if (quotedRustc !== "MATCH") {
      fail(
        `(xv) quoted "…\\rustc.exe" => '${quotedRustc}', expected MATCH (a quote is an exec-name boundary)`,
      );
      ok = false;
    }
    if (siblingDir !== "NOMATCH") {
      fail(
        `(xv) sibling …\\target\\gate-runner2 => '${siblingDir}', expected NOMATCH (a raw substring match would ` +
          `wrongly sweep a SIBLING runner's processes; the target must match on a path-segment boundary)`,
      );
      ok = false;
    }
    if (posixSibling !== "NOMATCH") {
      fail(
        `(xv) POSIX sibling …/target/gate-runner2 => '${posixSibling}', expected NOMATCH (boundary is platform-shared)`,
      );
      ok = false;
    }
    if (exactDir !== "MATCH") {
      fail(
        `(xv) exact runner target dir => '${exactDir}', expected MATCH (the positive must still hold)`,
      );
      ok = false;
    }
    if (ok) {
      pass(
        "(xv) WINDOWS SWEEP: quoted cargo.exe / rustc.exe referencing the runner target MATCH; a sibling " +
          "gate-runner2 (Windows + POSIX) does NOT; the exact runner target still does (discriminating)",
      );
    }
  }

  // --------------------------------------------------------------------------------------------------
  // (U-P0) NO PRODUCTION-GATE BYPASS MODE + (xix) STUB-INVOKED. The production gate CLI (`gate.mjs`) must
  //        have NO mode that returns the success contract (exit 0 as a gate PASS) without running the real
  //        gate. We assert TWO things:
  //          (1) EVERY removed mode EXITS NON-ZERO (unknown-flag / usage, code 127), with NO output that
  //              looks like a gate PASS: the `--internal-selftest-seam` seam (with AND without a step list,
  //              incl. the empty-step case that used to return EXIT_PASS doing no work); each `--selftest-*`
  //              classifier hook; the `-- <cmd>` custom-command path. Even with the legacy
  //              VERTER_GATE_SELFTEST(_STEPS) env set, NO `node gate.mjs <anything>` returns 0-as-gate-pass.
  //          (2) STUB-INVOKED (xix): the REAL gate path (no flag) with a failing `cargo` stub first on PATH
  //              that WRITES A MARKER FILE on invocation proves the gate ACTUALLY RAN the archive step (the
  //              marker exists) and treated the build failure as a gate FAILURE (exit 1) — NOT a no-op
  //              success. We assert the marker file EXISTS (the stub was invoked), not merely that the exit
  //              was non-zero (which an unrelated failure could satisfy vacuously).
  //        Discriminating control: a removed flag exits 127 while `--help` (a legitimate non-gate mode)
  //        still exits 0. The reusable seam itself is exercised by scenario (viii) via the self-test runner.
  // --------------------------------------------------------------------------------------------------
  process.stderr.write("\n(U-P0/xix) NO production-gate bypass mode + stub-invoked\n");
  {
    let ok = true;

    // (1) Every removed mode must EXIT NON-ZERO (never 0-as-gate-pass). We probe the production gate.mjs CLI
    // directly. Each removed flag is an unknown argument => usage exit 127. The empty-step seam case — which
    // used to return EXIT_PASS doing no work — is now just an unknown `--internal-selftest-seam` flag.
    const removedModes = [
      { argv: ["--internal-selftest-seam"], env: {}, why: "seam flag (no steps)" },
      {
        argv: ["--internal-selftest-seam"],
        env: { VERTER_GATE_SELFTEST: "1", VERTER_GATE_SELFTEST_STEPS: "" },
        why: "seam flag + EMPTY steps (the old EXIT_PASS-no-work path)",
      },
      {
        argv: ["--internal-selftest-seam"],
        env: { VERTER_GATE_SELFTEST_STEPS: `a|echo ${RUN_TAG}` },
        why: "seam flag + steps",
      },
      {
        argv: ["--selftest-classify-nextest", "/nonexistent"],
        env: {},
        why: "classify-nextest hook",
      },
      {
        argv: ["--selftest-classify-nextest-run", "0", "/nonexistent"],
        env: {},
        why: "classify-nextest-run hook",
      },
      { argv: ["--selftest-surface2", "/nonexistent"], env: {}, why: "surface2 hook" },
      { argv: ["--selftest-libtest", "0", "bin", "/nonexistent"], env: {}, why: "libtest hook" },
      {
        argv: ["--selftest-sweep-match", "posix", "/x", "--", "cargo"],
        env: {},
        why: "sweep-match hook",
      },
      { argv: ["--", "true"], env: {}, why: "custom-command path (`-- true`)" },
      {
        argv: ["--", `echo ${RUN_TAG}`],
        env: { VERTER_GATE_SELFTEST: "1" },
        why: "custom-command path + legacy env",
      },
    ];
    for (const m of removedModes) {
      const r = runGate(m.argv, m.env);
      if (r.code === EXIT_PASS) {
        fail(
          `(U-P0) BYPASS: \`node gate.mjs ${m.argv.join(" ")}\` (${m.why}) returned 0-as-gate-pass — a ` +
            "production-CLI mode must NEVER return the success contract without running the real gate.",
        );
        ok = false;
      } else if (r.code !== 127) {
        // Not a bypass, but a removed mode should be a clean USAGE error (127), not some other code.
        fail(
          `(U-P0) \`node gate.mjs ${m.argv.join(" ")}\` (${m.why}) returned ${r.code}; expected USAGE (127) ` +
            "for a removed/unknown mode (it did NOT return 0, which is the load-bearing property).",
        );
        ok = false;
      }
    }
    // Discriminating control: --help (a legitimate non-gate mode) still exits 0.
    const help = runGate(["--help"], {});
    if (help.code !== EXIT_PASS) {
      fail(
        `(U-P0) CONTROL: --help returned ${help.code}, expected 0 (the legit non-gate mode must work)`,
      );
      ok = false;
    }

    // The removed-mode argv probes + --help control above are PLATFORM-INDEPENDENT — they run on Windows
    // too. Record their result now so a Windows run still asserts the no-bypass property.
    if (ok) {
      pass(
        "(U-P0) NO PRODUCTION-GATE BYPASS MODE: every removed production-CLI mode (the seam flag incl. the " +
          "empty-step case, all --selftest-* hooks, `-- <cmd>`) exits 127 (never 0-as-gate-pass), even with the " +
          "legacy VERTER_GATE_SELFTEST env; --help (the legit non-gate mode) still exits 0 (discriminating)",
      );
    }

    // (2) STUB-INVOKED (xix) — POSIX-only: the stub is a `#!/usr/bin/env bash` script. A failing cargo stub
    // that WRITES A MARKER FILE on invocation. The REAL gate path (no flag) must invoke it (marker exists)
    // and map the build failure to exit 1.
    posix_xix: {
      if (IS_WINDOWS) {
        skip(
          "(xix) STUB-INVOKED — POSIX bash cargo-stub + PATH override (no portable Windows cargo-stub stand-in here)",
        );
        break posix_xix;
      }
      let okStub = true;
      const stubDir = freshTmpDir("gatetest-cargostub-");
      const marker = join(stubDir, `cargo_invoked_${RUN_TAG}.marker`);
      const stubPath = join(stubDir, "cargo");
      // The stub records that it ran (touch the marker) and then fails — modelling a build the gate must
      // treat as a real failure. `printf >> marker` so a multi-invocation still leaves the marker present.
      writeFileSync(
        stubPath,
        `#!/usr/bin/env bash\nprintf 'invoked %s\\n' "$*" >> "${marker}"\nexit 3\n`,
        { mode: 0o755 },
      );
      try {
        spawnSync("chmod", ["+x", stubPath], { stdio: "ignore" });
      } catch {
        /* ignore */
      }
      const stubPATH = `${stubDir}:${process.env.PATH || ""}`;
      const lk = freshLock();
      const tgt = freshTmpDir("gatetest-target-");
      const rNormal = runGate(["--timeout", "120s", "--stall", "60s"], {
        PATH: stubPATH,
        // Legacy env set: it must have ZERO effect (no divert to a no-op).
        VERTER_GATE_SELFTEST: "1",
        VERTER_GATE_SELFTEST_STEPS: `a|echo ${RUN_TAG}`,
        VERTER_GATE_LOCK: lk,
        VERTER_GATE_TARGET_DIR: tgt,
      });
      const stubWasInvoked = existsSync(marker);
      note(
        `real gate + failing cargo stub => rc=${rNormal.code} (expect build-fail 1); stub-invoked marker exists=${stubWasInvoked}`,
      );
      if (!stubWasInvoked) {
        fail(
          "(xix) STUB-INVOKED: the failing cargo stub was NOT invoked (no marker file) — the gate did NOT " +
            "run the real archive step. A non-zero exit WITHOUT the stub running would be a vacuous pass; the " +
            "gate must actually invoke cargo.",
        );
        okStub = false;
      }
      if (rNormal.code === EXIT_PASS) {
        fail(
          "(xix) the real gate path returned PASS (0) with a FAILING cargo stub — it did not run/observe the " +
            "build (gate-bypass).",
        );
        okStub = false;
      } else if (rNormal.code !== EXIT_FAIL) {
        fail(
          `(xix) the real gate path returned ${rNormal.code}; expected 1 (a stub-cargo build failure maps to ` +
            "a gate FAILURE, not 124/125/127).",
        );
        okStub = false;
      }
      if (okStub) {
        pass(
          "(xix) STUB-INVOKED: the real gate path with a failing cargo stub ACTUALLY INVOKED cargo (marker " +
            "present — proving the gate ran the archive step, not a no-op) and mapped the build failure to " +
            "exit 1 (discriminating; a non-zero exit without the stub running would be vacuous)",
        );
      }
    }
  }

  // --------------------------------------------------------------------------------------------------
  // (U-P1) STRICT ARGV ON THE NON-GATE EXIT-0 MODES (--help / --prepare are MUTUALLY EXCLUSIVE). The two
  //        modes that legitimately exit 0 without running the gate must NOT be reachable with junk argv —
  //        otherwise a stray flag rides an exit-0 mode. We assert (PLATFORM-INDEPENDENT — pure argv probes
  //        of the production CLI):
  //          * `--help --bad-flag` => USAGE (127), NOT 0 (pre-fix: help broke the parse loop immediately and
  //            IGNORED the trailing token, exiting 0 — the bug this scenario discriminates).
  //          * a bare `--help` => 0 (the legit non-gate mode still works — discriminating control).
  //          * `--prepare trailingjunk` => 127 (a positional after --prepare is rejected).
  //          * `--prepare --selftest-x` => 127 (an unknown flag after --prepare is rejected).
  //          * `--prepare --no-fail-fast` => 127 (a GATE-ONLY flag after --prepare is rejected — pre-fix it
  //            was SILENTLY ACCEPTED and the warm-pass ran, the strongest discriminator for the prepare
  //            mutual-exclusion).
  // --------------------------------------------------------------------------------------------------
  process.stderr.write(
    "\n(U-P1) strict argv on the non-gate exit-0 modes (--help/--prepare exclusive)\n",
  );
  {
    let ok = true;
    const helpBad = runGate(["--help", "--bad-flag"], {});
    if (helpBad.code !== 127) {
      fail(
        `(U-P1) --help --bad-flag returned ${helpBad.code}, expected USAGE (127) — --help must NOT swallow a ` +
          "trailing token and exit 0 (it is mutually exclusive: a bare --help only).",
      );
      ok = false;
    }
    const helpBare = runGate(["--help"], {});
    if (helpBare.code !== EXIT_PASS) {
      fail(
        `(U-P1) bare --help returned ${helpBare.code}, expected 0 (the legit non-gate help mode must still work)`,
      );
      ok = false;
    }
    // --prepare mutual-exclusion: a positional, an unknown flag, AND a gate-only flag each => 127.
    const prepareCases = [
      { argv: ["--prepare", "trailingjunk"], why: "positional after --prepare" },
      { argv: ["--prepare", "--selftest-x"], why: "unknown flag after --prepare" },
      {
        argv: ["--prepare", "--no-fail-fast"],
        why: "gate-only flag after --prepare (pre-fix: accepted)",
      },
    ];
    for (const c of prepareCases) {
      const r = runGate(c.argv, {});
      if (r.code === EXIT_PASS) {
        fail(
          `(U-P1) \`gate.mjs ${c.argv.join(" ")}\` (${c.why}) returned 0 — --prepare must reject junk argv, ` +
            "never reach its exit-0 warm-pass with stray arguments.",
        );
        ok = false;
      } else if (r.code !== 127) {
        fail(
          `(U-P1) \`gate.mjs ${c.argv.join(" ")}\` (${c.why}) returned ${r.code}; expected USAGE (127)`,
        );
        ok = false;
      }
    }
    if (ok) {
      pass(
        "(U-P1) STRICT ARGV: --help --bad-flag => 127 (bare --help => 0); --prepare trailingjunk / " +
          "--prepare --selftest-x / --prepare --no-fail-fast each => 127 — the exit-0 non-gate modes are " +
          "mutually exclusive and unreachable with junk argv (discriminating)",
      );
    }
  }

  // --------------------------------------------------------------------------------------------------
  // (U-P2-prep) --prepare SUCCESS OUTPUT IS NOT A GATE PASS. --prepare exits 0 on success, but it is a
  //        WARM-PASS, never a gate verdict — so a CI `grep PASS` of its output must find NOTHING that looks
  //        like a verdict. We drive the REAL prepare success-output producer (`preparedSuccessLines`,
  //        imported in-process — the exact strings runPrepare logs on the success path) and assert:
  //          * the success marker is `PREPARED_NOT_GATE` (PREPARE_SUCCESS_MARKER), present in the output.
  //          * NO produced line contains the token `PASS` (a CI grep cannot mistake prepare for a verdict).
  //        Pre-fix the success output contained "...it is NOT a gate PASS." (a literal PASS token a CI grep
  //        would match), so this scenario FAILS against the pre-fix code and PASSES post-fix. Cargo-free.
  // --------------------------------------------------------------------------------------------------
  process.stderr.write(
    "\n(U-P2-prep) --prepare success output is NOT a gate PASS (no PASS token)\n",
  );
  {
    let ok = true;
    // A representative successful prepare (10 suites archived/listed, all warmed, 0 failures, 0 missing).
    const lines = preparedSuccessLines(10, 10, 0, 0);
    const joined = lines.join("\n");
    note(`prepare success marker = ${PREPARE_SUCCESS_MARKER}`);
    if (PREPARE_SUCCESS_MARKER !== "PREPARED_NOT_GATE") {
      fail(
        `(U-P2-prep) the prepare success marker is '${PREPARE_SUCCESS_MARKER}', expected 'PREPARED_NOT_GATE'`,
      );
      ok = false;
    }
    if (!joined.includes(PREPARE_SUCCESS_MARKER)) {
      fail(
        `(U-P2-prep) the prepare success output does NOT contain the '${PREPARE_SUCCESS_MARKER}' marker`,
      );
      ok = false;
    }
    if (joined.includes("PASS")) {
      const bad = lines.find((l) => l.includes("PASS"));
      fail(
        `(U-P2-prep) the prepare success output contains the token 'PASS' — a CI grep PASS would mistake it ` +
          `for a gate verdict. Offending line: ${bad}`,
      );
      ok = false;
    }
    if (ok) {
      pass(
        "(U-P2-prep) --prepare success output uses the PREPARED_NOT_GATE marker and contains NO 'PASS' token " +
          "(a CI grep PASS cannot confuse the warm-pass with a gate verdict; discriminating — pre-fix it " +
          "printed 'NOT a gate PASS' which a grep would match)",
      );
    }
  }

  // --------------------------------------------------------------------------------------------------
  // (xvii) SIGNAL TEARDOWN REAPS THE ACTIVE CHILD TREE. A SIGINT/SIGTERM to ONLY the gate-process pid (NOT
  //        the whole process group) must reap the active step's WHOLE tree (the argv-tagged sleep) BEFORE
  //        releasing the lock. Pre-fix, the signal handler ran only the provenance sweep — which skips a
  //        plain `sleep` (not a build tool, no runner-target reference) — so the test child SURVIVED while
  //        the lock was released, letting a second gate start over a still-running test. We spawn the
  //        self-test runner DETACHED (its own group — it composes the SAME teardown lifecycle gate.mjs
  //        uses), running a wrapper that backgrounds an argv-tagged `sleep 600` and waits; once the lock is
  //        held and the sleep is live we `kill(runnerPid, SIGTERM)` — the RUNNER PID ONLY (a positive pid,
  //        never the negative group) — and assert: 0 surviving argv-tagged sleeps (the tree was reaped) AND
  //        the lockdir was released. Discriminates: the pre-fix sweep-only path leaves the sleep alive.
  // --------------------------------------------------------------------------------------------------
  process.stderr.write(
    "\n(xvii) SIGNAL teardown reaps the active child tree (SIGTERM to the gate-process pid only)\n",
  );
  posix_xvii: {
    if (IS_WINDOWS) {
      skip(
        "(xvii) SIGNAL teardown reaps active tree — POSIX SIGTERM-to-pid + process-group reap (no Windows stand-in)",
      );
      break posix_xvii;
    }
    let ok = true;
    const lk = freshLock();
    const tgt = freshTmpDir("gatetest-target-");
    // A wrapper that backgrounds ONE argv-tagged sleep 600 and waits. The sleep is a non-build-tool child
    // with NO runner-target reference, so ONLY a real tree-reap (not the provenance sweep) can kill it.
    const childCmd = `( exec -a sleep_${RUN_TAG}_sigchild sleep 600 ) & c=$!; wait $c`;
    const gate = spawnContainedCmd(
      childCmd,
      { VERTER_GATE_LOCK: lk, VERTER_GATE_TARGET_DIR: tgt },
      ["--timeout", "600s", "--stall", "600s"],
    );
    // Wait for the lock to be held AND the argv-tagged sleep to be live.
    const held = await waitLockHeld(lk);
    let sawChild = false;
    for (let w = 0; w < 60; w++) {
      if (countArgvSleeps(`${RUN_TAG}_sigchild`) > 0) {
        sawChild = true;
        break;
      }
      await delay(100);
    }
    if (!held || !sawChild) {
      fail(
        `(xvii) setup: lock-held=${held} child-live=${sawChild} — could not stage the signal-reap test`,
      );
      ok = false;
    } else {
      note(
        `gate pid=${gate.pid} holds the lock and the argv-tagged child is live; sending SIGTERM to the GATE PID ONLY`,
      );
      // SIGTERM the GATE PID ONLY — a positive pid, NOT the negative process group. If teardown relied on
      // the OS delivering the signal to the whole group (or on the group-wide reap the watchdog does), this
      // would not exercise the bug. We target the single pid so only the gate's OWN handler can reap the
      // tree.
      try {
        process.kill(gate.pid, "SIGTERM");
      } catch {
        /* ignore */
      }
      // Give the handler time to reap (TERM→grace→KILL) + release the lock.
      let released = false;
      for (let w = 0; w < 80; w++) {
        if (!existsSync(lk)) {
          released = true;
          break;
        }
        await delay(100);
      }
      await delay(1000);
      const survivors = countArgvSleeps(`${RUN_TAG}_sigchild`);
      note(
        `after SIGTERM-to-pid: surviving argv-tagged children=${survivors} lock-released=${released}`,
      );
      if (survivors !== 0) {
        fail(
          `(xvii) ${survivors} argv-tagged child(ren) SURVIVED a SIGTERM to the gate pid — the signal teardown ` +
            "did NOT reap the active step's tree (the provenance sweep alone skips a plain sleep). A second gate " +
            "could start over a still-running test.",
        );
        spawnSync("pkill", ["-9", "-f", `sleep_${RUN_TAG}_sigchild`], { stdio: "ignore" });
        ok = false;
      }
      if (!released) {
        fail(
          "(xvii) the lockdir was NOT released after the signal teardown — the mutex stayed held",
        );
        ok = false;
      }
    }
    if (ok) {
      pass(
        "(xvii) SIGNAL TEARDOWN: a SIGTERM to the gate-process pid ONLY reaped the active step's whole tree " +
          "(0 surviving children) and released the lock — no orphan survives, lock freed only after reaping",
      );
    }
    // Belt-and-suspenders cleanup.
    spawnSync("pkill", ["-9", "-f", `sleep_${RUN_TAG}_sigchild`], { stdio: "ignore" });
  }

  // --------------------------------------------------------------------------------------------------
  // (xx) TEARDOWN VERIFIED REAP — death is CONFIRMED, not merely signaled. The reap must POLL past SIGKILL
  //      and confirm the tree is actually gone before treating teardown as clean (not just fire the kill
  //      and return). We stage a child that TRAPS + IGNORES SIGTERM (`trap '' TERM`) so the grace-window
  //      SIGTERM does NOTHING — only the subsequent SIGKILL ends it. Under a whole-gate `--timeout`, the
  //      watchdog must SIGTERM (no-op), then SIGKILL, then VERIFY the group is dead. We assert: the gate
  //      returns TIMEOUT (124) AND, after it returns, ZERO argv-tagged children survive — i.e. the reap did
  //      not return while a SIGTERM-immune child was still live. If the reap only issued the kill without a
  //      verification poll, a slow-to-die child could outlive the gate's return; the confirmed-dead poll
  //      closes that. Discriminating: a SIGTERM-immune child that survived would leave a non-zero survivor
  //      count.
  // --------------------------------------------------------------------------------------------------
  process.stderr.write("\n(xx) TEARDOWN verified reap (confirmed-dead poll past SIGKILL)\n");
  posix_xx: {
    if (IS_WINDOWS) {
      skip(
        "(xx) TEARDOWN verified reap — POSIX SIGTERM-trap + SIGKILL + group-death poll (no Windows stand-in)",
      );
      break posix_xx;
    }
    let ok = true;
    const lk = freshLock();
    const tgt = freshTmpDir("gatetest-target-");
    // A wrapper that IGNORES SIGTERM, backgrounds ONE argv-tagged sleep 600 (also SIGTERM-immune via the
    // inherited trap is not guaranteed for the child sleep, so we make the WRAPPER ignore TERM and `wait`;
    // the negative-PGID SIGTERM hits the whole group — the wrapper ignores it, the sleep may or may not —
    // but the SIGKILL ends both). The point: SIGTERM does not end the wrapper, so only the verified SIGKILL
    // path tears the tree down.
    const immuneCmd = `trap '' TERM; ( exec -a sleep_${RUN_TAG}_immune sleep 600 ) & c=$!; wait $c`;
    const t0 = Date.now();
    const r = runContainedCmd(immuneCmd, { VERTER_GATE_LOCK: lk, VERTER_GATE_TARGET_DIR: tgt }, [
      "--timeout",
      "4s",
      "--stall",
      "600s",
    ]);
    const elapsed = Math.round((Date.now() - t0) / 1000);
    await delay(1500);
    const survivors = countArgvSleeps(`${RUN_TAG}_immune`);
    note(
      `SIGTERM-immune wrapper under --timeout 4s => rc=${r.code} after ${elapsed}s; surviving children=${survivors}`,
    );
    if (r.code !== EXIT_TIMEOUT) {
      fail(`(xx) SIGTERM-immune wrapper => rc=${r.code}, expected TIMEOUT (${EXIT_TIMEOUT})`);
      ok = false;
    }
    if (elapsed >= 60) {
      fail(`(xx) took ${elapsed}s — the watchdog did not bound the immune run near 4s`);
      ok = false;
    }
    if (survivors !== 0) {
      fail(
        `(xx) ${survivors} argv-tagged child(ren) SURVIVED after the gate returned — the reap did NOT confirm ` +
          "the SIGTERM-immune tree dead before returning (it must SIGKILL then POLL past the kill until ESRCH).",
      );
      spawnSync("pkill", ["-9", "-f", `sleep_${RUN_TAG}_immune`], { stdio: "ignore" });
      ok = false;
    }
    if (ok) {
      pass(
        "(xx) TEARDOWN VERIFIED REAP: a SIGTERM-immune child under --timeout was SIGKILL'd and CONFIRMED dead " +
          "(0 survivors after the gate returned) in ~" +
          elapsed +
          "s — the reap polls past SIGKILL, it does not merely issue the kill (discriminating)",
      );
    }
    // teardown lockdir wait + belt-and-suspenders cleanup.
    for (let w = 0; w < 40; w++) {
      if (!existsSync(lk)) break;
      await delay(100);
    }
    spawnSync("pkill", ["-9", "-f", `sleep_${RUN_TAG}_immune`], { stdio: "ignore" });
  }

  // --------------------------------------------------------------------------------------------------
  // (xviii) SURFACE-2 CRASHED/ABNORMAL LIBTEST IS A HARD FAIL. A direct-libtest failure is tolerated ONLY
  //         under NORMAL libtest failure semantics: exit 101 + a parsed `test result: FAILED` summary whose
  //         `failed` count EXACTLY equals the parsed FAILED names + every name allowlisted. We drive the REAL
  //         `analyzeLibtestSurface` IN-PROCESS. Discriminators —
  //         each FAILs the pre-fix "non-zero exit + ≥1 tolerated FAILED line => tolerate" logic:
  //           (a) a SIGABRT crash (exit 134) whose output names a TOLERATED test + an abort message but NO
  //               `test result:` summary => HARD FAIL (a signal is never tolerated);
  //           (b) a clean exit-101 with a tolerated FAILED name but a MISSING summary => HARD FAIL
  //               (unaccounted — the run did not complete normally);
  //           (c) exit 101 + a tolerated FAILED name + a summary whose failed count (2) EXCEEDS the parsed
  //               FAILED names (1) => HARD FAIL (an unaccounted extra failure hides in the count);
  //           (d) exit 101 + a tolerated FAILED name + a matching summary (failed=1) => PASS-WITH-TOLERATED
  //               (the proper tolerated shape still tolerates — no over-strict regression);
  //           (e) a NON-tolerated FAILED name under a clean 101 + matching summary => HARD FAIL;
  //           (f) a clean run (exit 0, no FAILED) => PASS.
  // --------------------------------------------------------------------------------------------------
  process.stderr.write("\n(xviii) SURFACE-2 crashed/abnormal libtest hard-fail\n");
  {
    const fixDir = freshTmpDir("gatetest-libtest-");
    const TOL =
      "cases::typeinfo_proto_ts_freshness::typeinfo_ts_bindings_are_byte_equal_to_regenerated_buf_output";
    const BIN = "verter_protocol::main";
    const sigabrt = join(fixDir, "sigabrt.log");
    const noSummary = join(fixDir, "no_summary.log");
    const countMismatch = join(fixDir, "count_mismatch.log");
    const properTol = join(fixDir, "proper_tolerated.log");
    const realFail = join(fixDir, "real_fail.log");
    const cleanPass = join(fixDir, "clean_pass.log");
    // (a) A SIGABRT: a tolerated FAILED name printed, then an abort with NO `test result:` summary. The
    //     process exit for a SIGABRT is 134 (128+6). Pre-fix: tolerated (≥1 tolerated FAILED line). Post-fix:
    //     HARD FAIL (signal exit, never tolerated; also no summary).
    writeFileSync(
      sigabrt,
      `running 3 tests\ntest ${TOL} ... FAILED\nerror: test failed, to rerun pass ` +
        `'-p verter_protocol --test main'\n\nthread 'main' panicked / SIGABRT: process abort\n`,
    );
    // (b) exit 101 but NO `test result:` summary (a truncated/aborted run). Post-fix: HARD FAIL (unaccounted).
    writeFileSync(noSummary, `running 1 test\ntest ${TOL} ... FAILED\n`);
    // (c) exit 101 + tolerated name + summary failed=2 but only 1 parsed FAILED name => unaccounted extra.
    writeFileSync(
      countMismatch,
      `running 5 tests\ntest ${TOL} ... FAILED\n\ntest result: FAILED. 3 passed; 2 failed; 0 ignored\n`,
    );
    // (d) the PROPER tolerated shape: exit 101 + tolerated name + matching summary (failed=1).
    writeFileSync(
      properTol,
      `running 5 tests\ntest ${TOL} ... FAILED\n\ntest result: FAILED. 4 passed; 1 failed; 0 ignored\n`,
    );
    // (e) a NON-tolerated failing test under an otherwise-normal 101 + matching summary => HARD FAIL.
    writeFileSync(
      realFail,
      `running 5 tests\ntest cases::some_module::a_real_regression ... FAILED\n\n` +
        `test result: FAILED. 4 passed; 1 failed; 0 ignored\n`,
    );
    // (f) a clean run: exit 0, no FAILED, summary ok => PASS.
    writeFileSync(cleanPass, `running 5 tests\n\ntest result: ok. 5 passed; 0 failed; 0 ignored\n`);

    const libtest = (code, binaryId, file) => verdictLibtestFile(code, binaryId, file);
    const vSig = libtest(134, BIN, sigabrt);
    const vNoSum = libtest(101, BIN, noSummary);
    const vMism = libtest(101, BIN, countMismatch);
    const vProper = libtest(101, BIN, properTol);
    const vReal = libtest(101, BIN, realFail);
    const vClean = libtest(0, BIN, cleanPass);
    note(
      `sigabrt(134)=${vSig} no-summary(101)=${vNoSum} count-mismatch(101)=${vMism} ` +
        `proper-tolerated(101)=${vProper} real-fail(101)=${vReal} clean(0)=${vClean}`,
    );
    let ok = true;
    if (vSig !== "FAIL") {
      fail(
        `(xviii a) SIGABRT (exit 134) with a tolerated name + abort => '${vSig}', expected FAIL (a signal/crash is never tolerated)`,
      );
      ok = false;
    }
    if (vNoSum !== "FAIL") {
      fail(
        `(xviii b) exit 101 + tolerated name but NO 'test result:' summary => '${vNoSum}', expected FAIL (unaccounted)`,
      );
      ok = false;
    }
    if (vMism !== "FAIL") {
      fail(
        `(xviii c) exit 101 + summary failed=2 but 1 parsed FAILED name => '${vMism}', expected FAIL (unaccounted extra)`,
      );
      ok = false;
    }
    if (vProper !== "PASS-WITH-TOLERATED") {
      fail(
        `(xviii d) exit 101 + tolerated name + matching summary (failed=1) => '${vProper}', expected PASS-WITH-TOLERATED (no over-strict regression)`,
      );
      ok = false;
    }
    if (vReal !== "FAIL") {
      fail(
        `(xviii e) a NON-tolerated FAILED name under a clean 101 + matching summary => '${vReal}', expected FAIL`,
      );
      ok = false;
    }
    if (vClean !== "PASS") {
      fail(`(xviii f) a clean run (exit 0, no FAILED) => '${vClean}', expected PASS`);
      ok = false;
    }
    if (ok) {
      pass(
        "(xviii) SURFACE-2 CRASH GATE: SIGABRT(134), exit-101-without-summary, and summary-count-mismatch ALL " +
          "=> FAIL even with a tolerated name; a proper exit-101 + matching summary + allowlisted name tolerates; " +
          "a real non-tolerated failure FAILs; a clean run PASSes (discriminating)",
      );
    }
  }

  // --------------------------------------------------------------------------------------------------
  // HONEST PLATFORM-COVERAGE GAP — stated both ways, so neither a POSIX nor a Windows green run overclaims:
  //   - On a NON-Windows host: the Windows RUNTIME process-management path — `taskkill /PID <pid> /T /F`
  //     tree kill, CIM CreationDate start-identity, and the lock/timeout/stall behavior under a real Windows
  //     process group — is NOT exercised (no portable `sleep`/argv-rename stand-in for Windows here). Only
  //     the Windows sweep-MATCHER regex units (xii, xv) run (via the matcher's `windows` flag). It is
  //     covered by static review; it is NOT counted as a passing Windows runtime process-management test.
  //   - On a WINDOWS host: the POSIX process-management scenarios above each emit a TRUE skip (counted in
  //     SKIP, never in PASS), so a green Windows run does NOT falsely imply POSIX process-group coverage.
  //     The platform-independent classifier / verdict / sweep-matcher / suite-selection / removed-mode-argv
  //     scenarios DO run on Windows and ARE counted.
  // --------------------------------------------------------------------------------------------------
  if (!IS_WINDOWS) {
    process.stderr.write(
      "\n[honest-skip] Windows RUNTIME process-management selftests (taskkill /T tree kill, CIM start-\n" +
        "identity, the Windows lock/timeout/stall path) are NOT run on this non-Windows host — there is no\n" +
        "portable Windows sleep/argv-rename stand-in here. Only the Windows sweep-MATCHER regex units (xii,\n" +
        "xv) are exercised (via the matcher's `windows` flag). The Windows runtime path is covered by static\n" +
        "review; it is NOT counted as a passing process-management test on a non-Windows run.\n",
    );
  } else {
    process.stderr.write(
      "\n[honest-skip] On this Windows host the POSIX process-management scenarios were SKIPPED (counted in\n" +
        "SKIP, NOT in PASS) — there is no portable Windows sleep/argv-rename stand-in. The platform-\n" +
        "independent classifier/verdict/sweep-matcher/suite-selection/removed-mode-argv scenarios DID run.\n",
    );
  }

  finish();
}

function finish() {
  process.stderr.write("\n=== SELF-TEST SUMMARY ===\n");
  for (const r of RESULTS) process.stderr.write(`${r}\n`);
  process.stderr.write("-------------------------\n");
  process.stderr.write(`PASS=${PASS_COUNT}  FAIL=${FAIL_COUNT}  SKIP=${SKIP_COUNT}\n`);
  if (FAIL_COUNT === 0) {
    process.stderr.write("ALL SELF-TESTS PASSED\n");
    process.exit(0);
  } else {
    process.stderr.write(`SELF-TESTS FAILED (${FAIL_COUNT} failing)\n`);
    process.exit(1);
  }
}

main().catch((e) => {
  fail(`harness threw: ${e && e.stack ? e.stack : e}`);
  finish();
});
