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
//   (F1)   FRESHNESS SHIM RESOLVER — `resolveLocalBinShim` / `resolvePathShim` / `findPathEnvKey` with an
//                           INJECTED fake fs/env: both shims present resolve; a missing shim => null; the
//                           Windows suffix set resolves a `.CMD` shim (POSIX-host-driven via the matcher's
//                           `windows` flag); (h) [P1] Windows reads the PATH var by CASE-INSENSITIVE key, so
//                           a `PaTh`/`path`-cased env RESOLVES (matching Rust `var_os("PATH")` fold) while
//                           POSIX stays case-EXACT (a `PaTh` key => null); `findPathEnvKey` tie-breaks
//                           PATH>Path>any-case and rejects a non-string value. (h) fails pre-fix: the old
//                           `env.Path ?? env.PATH` read MISSED a `PaTh` key ⇒ null (the silent fail-open).
//   (F2)   FRESHNESS PREFLIGHT — `preflightFreshnessTooling` with an INJECTED `runInstall`, BUF-ABSENCE-ONLY
//                           tolerance: (a) both present up front => allowed:false action:"already-present",
//                           runInstall NOT called; (b) missing then install succeeds + re-resolve finds them
//                           => allowed:false action:"installed", runInstall called once; (c) buf+pnpm absent
//                           => allowed:true action:"tolerate-genuinely-absent" (install NOT attempted); (d)
//                           pnpm present + install non-zero (lockfile mismatch) + still missing => allowed:false
//                           action:"setup-fail"; (e) install non-zero BUT tools resolvable on PATH =>
//                           allowed:false action:"setup-fail" (a launched non-zero install fails loud
//                           regardless of PATH resolvability — the bypass-regression precedence); (f) install
//                           exit-0 but tools still missing => allowed:false action:"setup-fail"; (i) [FAIL-OPEN
//                           CLOSURE] buf PRESENT + oxfmt ABSENT + pnpm ABSENT => allowed:false
//                           action:"setup-fail" (oxfmt is required when buf runs; NEVER tolerate) — fails
//                           pre-fix; (j) buf+pnpm absent => tolerate (buf is the skip-determining tool); (k)
//                           exit-0 + buf present + oxfmt still absent => setup-fail; (l) pnpm absent +
//                           buf+oxfmt on PATH => path-fallback; (m) [P3 PATH-ONLY] pnpm matchable ONLY at
//                           node_modules/.bin (NOT on PATH) + buf absent => allowed:true
//                           action:"tolerate-genuinely-absent", runInstall 0 — `resolvePnpm` resolves pnpm via
//                           PATH (the way `pnpmInstallCommand` launches it), so a local-only shim does NOT
//                           count; fails pre-fix (the local shim resolved ⇒ the install was attempted).
//                           (install-reaching cases b/d/e/f/h/k put pnpm on PATH, matching the launch.)
//   (F3)   VERDICT GATING — the freshness-pair name with `freshnessToleranceAllowed=false` => FAIL on BOTH
//                           the nextest classifier and the libtest analyzer; with `=true` =>
//                           PASS-WITH-TOLERATED; a CRASH (signal/non-101) whose only name is the freshness
//                           pair => FAIL regardless of the flag; a non-allowlisted failure => FAIL
//                           regardless. The durable invariant: tolerance is consulted ONLY when allowed.
//   (F5)   CARGO-ENV PATH SANITIZATION — `buildCargoEnv` sanitizes PATH/Path to its CWD-INDEPENDENT
//                           ABSOLUTE components ONLY (empty, dot-only `.`/`./`/`.\`, non-dot relative, `..`-
//                           relative, Windows drive-relative `C:foo`, and Windows root-relative `\x`/`/x` are
//                           ALL dropped) so the EXECUTED cargo/nextest/libtest tests and the verdict preflight
//                           resolver resolve every tool from the SAME absolute-only PATH (the CLOSED cwd-
//                           independent invariant, no preflight-vs-test disagreement):
//                           an all-relative PATH "SAFE::.:./:OTHER:" => key DELETED; a PATH with NO absolute
//                           component (`:`/`:.`/`.`/all-relative) DELETES the key (not assigns "") so Rust's
//                           `var_os("PATH")?` early-returns None (a present "" is split_paths("") == [""], a
//                           live CWD source); absolute dirs kept — a leading-`/` (incl. bare root `/`, `/.`)
//                           on POSIX, a drive-rooted `C:\x`/`C:\.`, a UNC `\\srv\share`, or a device path on
//                           Windows; a missing PATH stays missing; the `;`-delimited Windows shape covered via
//                           `sanitizePathValue(_, true)`. (h) [P1] On Windows the PATH var is identified by
//                           CASE-INSENSITIVE key (`buildCargoEnv(env, target, true)`), so a `PaTh`-cased env
//                           is sanitized/DELETED while POSIX (`windows:false`) leaves a `PaTh` var untouched.
//                           (j) F-D absolute-only decider cases; (k) the load-bearing preflight-vs-child
//                           agreement. Discriminates: pre-fix buildCargoEnv left PATH unchanged, assigned ""
//                           on all-implicit, KEPT non-dot relative / `..` / drive-relative / Windows root-
//                           relative entries, and left a `PaTh` key unsanitized.
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
  ensureRequiredWindowsDebugSidecars,
  isBuildTool,
  targetDirMatches,
  preparedSuccessLines,
  PREPARE_SUCCESS_MARKER,
  // freshness-tooling preflight + platform-aware shim resolution (verdict-gating authority)
  resolveLocalBinShim,
  resolvePathShim,
  findPathEnvKey,
  resolveExecutableShim,
  // the platform-PARAMETERIZED PATH primitives the resolver/sanitizer share: the literal delimiter selected
  // by the `windows` flag (host-independent, NOT the ambient `node:path.delimiter`) and the NON-NORMALIZING
  // PATH-component appender that mirrors the Rust child's lexical `dir.join(tool)`.
  pathDelimiterFor,
  appendPathComponentRaw,
  resolvePnpm,
  preflightFreshnessTooling,
  pnpmInstallCommand,
  // cargo env builder + the PATH sanitizer — exercised by (F5) for the CWD-INDEPENDENT ABSOLUTE-ONLY PATH
  // sanitization that aligns the verdict preflight resolver with the executed test PATH (empty, dot-only,
  // non-dot relative, `..`-relative, and Windows drive-relative / root-relative entries are all dropped) —
  // the CLOSED cwd-independent invariant, no preflight-vs-test disagreement.
  buildCargoEnv,
  sanitizePathValue,
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

// All three in-process verdict helpers thread `freshnessToleranceAllowed` to the REAL classifiers. The
// allowlist consultation is GATED by that flag (see `TOLERATED_TEST_NAMES` in gate-internals.mjs): when it
// is false, an allowlisted freshness-pair FAIL is a HARD regression, not tolerated. These helpers default
// the flag to `true` so the EXISTING allowlist/tolerance scenarios (vii / ix / xiv / xviii — which assert
// the tolerated baseline shape) keep exercising the tolerance-ALLOWED behavior; the NEW verdict-gating
// scenario passes `false` explicitly to prove the gate (the same pair flips PASS-WITH-TOLERATED → FAIL).

// nextest classifier (log content only, no exit code) — mirrors the old `--selftest-classify-nextest`.
function verdictClassifyNextest(text, freshnessToleranceAllowed = true) {
  const cls = classifyNextestFailures(text, freshnessToleranceAllowed);
  if (cls === "regression") return "FAIL";
  if (cls === "tolerated") return "PASS-WITH-TOLERATED";
  return "PASS";
}
function verdictClassifyNextestFile(file, freshnessToleranceAllowed = true) {
  return verdictClassifyNextest(readFileSync(file, "utf8"), freshnessToleranceAllowed);
}

// nextest LIVE-aggregation verdict (with exit code) — mirrors the old `--selftest-classify-nextest-run`.
function verdictNextestRun(code, text, freshnessToleranceAllowed = true) {
  const r = analyzeNextestSurface(text, code, freshnessToleranceAllowed);
  if (r.failures.length > 0) return "FAIL";
  if (r.toleratedCount > 0) return "PASS-WITH-TOLERATED";
  return "PASS";
}
function verdictNextestRunFile(code, file, freshnessToleranceAllowed = true) {
  return verdictNextestRun(code, readFileSync(file, "utf8"), freshnessToleranceAllowed);
}

// SURFACE-2 libtest verdict — mirrors the old `--selftest-libtest`.
function verdictLibtest(code, binaryId, text, freshnessToleranceAllowed = true) {
  const r = analyzeLibtestSurface(text, code, binaryId, freshnessToleranceAllowed);
  if (r.verdict === "fail") return "FAIL";
  if (r.verdict === "tolerated") return "PASS-WITH-TOLERATED";
  return "PASS";
}
function verdictLibtestFile(code, binaryId, file, freshnessToleranceAllowed = true) {
  return verdictLibtest(code, binaryId, readFileSync(file, "utf8"), freshnessToleranceAllowed);
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

// Holder command for the HOLDER-SURVIVAL probe (scenario (i)): a long-lived leaf process whose argv (a)
// is recognized as a build tool by the REAL `isBuildTool` predicate (argv[0] renamed to `cargo` via
// `exec -a`) and (b) literally references the SHARED runner target dir — so it is EXACTLY the kind of
// process `provenanceSweep(target)` TERM/KILLs. `exec -a` replaces the wrapping bash, so the only
// surviving process is the renamed leaf. node holds the process open indefinitely; the trailing
// `<target>` arg is an inert positional arg the eval script ignores. Pre-fix, a LOCK-REFUSED second gate
// swept this leaf dead; post-fix (sweep gated on `acquired`) it must survive.
function buildToolHolderCmd(sharedTarget) {
  return `exec -a cargo "${process.execPath}" -e "setInterval(()=>{}, 1e9)" "${sharedTarget}"`;
}

// Count LIVE processes that the REAL provenance-sweep predicate would match for a given target dir:
// `isBuildTool(cmd) && targetDirMatches(cmd, target)`. POSIX-only (uses `ps`); used by the holder-
// survival probe to assert the holder's build-tool leaf was NOT swept by a non-acquiring gate.
function countSweepMatchesForTarget(sharedTarget) {
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
    const cmd = trimmed.slice(sp + 1);
    if (!cmd) continue;
    // The harness itself (and the `ps` we just spawned) must never count: key strictly on the leaf's
    // build-tool-name + shared-target-dir signature, which only the crafted holder leaf carries.
    if (isBuildTool(cmd) && targetDirMatches(cmd, sharedTarget, false)) n++;
  }
  return n;
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

    // ----------------------------------------------------------------------------------------------
    // (i-survival) HOLDER SURVIVES A REFUSED GATE — the safety property the LOCK-REFUSED path exists
    // to uphold. Two gates sharing the DEFAULT runner target dir (the real-world case: both resolve to
    // <repo>/target/gate-runner) must be isolated — a gate that REFUSES the lock must touch NO other
    // process. The holder runs a build-tool-named leaf (argv[0]=`cargo`) referencing the SHARED target
    // dir, i.e. EXACTLY what `provenanceSweep(target)` matches. We then launch a second gate sharing
    // that target; it refuses (LOCK-REFUSED) and tears down. PRE-FIX teardown ran the sweep on the
    // `!acquired` path and KILLED the holder's leaf (this assertion FAILS). POST-FIX the sweep is gated
    // on `acquired`, so the non-acquiring gate leaves the holder's leaf ALIVE (this assertion PASSES).
    // This is the discriminating RED-prove for F1; the existing separate-target scenario above never
    // exercised it (the sweep on a different target dir could not match the holder's process).
    survival_i: {
      const lk2 = freshLock();
      const sharedTgt = freshTmpDir("gatetest-shared-target-");
      const holder2 = spawnContainedCmd(buildToolHolderCmd(sharedTgt), {
        VERTER_GATE_LOCK: lk2,
        VERTER_GATE_TARGET_DIR: sharedTgt,
      });
      const acq2 = await waitLockHeld(lk2);
      if (!acq2) {
        fail("(i-survival) holder never acquired the lock within 6s");
        break survival_i;
      }
      // Wait (bounded) until the holder's build-tool leaf is visible to the sweep predicate — i.e. the
      // `exec -a cargo node …` has replaced the wrapping bash and `ps` reports it. If it never appears
      // the probe cannot discriminate, so fail rather than falsely pass.
      let holderLeaves = 0;
      for (let w = 0; w < 60; w++) {
        holderLeaves = countSweepMatchesForTarget(sharedTgt);
        if (holderLeaves >= 1) break;
        await delay(100);
      }
      if (holderLeaves < 1) {
        fail(
          "(i-survival) holder's build-tool leaf never became sweep-visible — cannot discriminate",
        );
      } else {
        note(`holder build-tool leaf is sweep-visible (matches=${holderLeaves})`);
        // A SECOND gate sharing the SAME target dir. It must REFUSE the held lock and, on teardown,
        // touch nothing — leaving the holder's leaf alive.
        const refused = runContainedCmd(singleSleepCmd("survivor_second_i", 5), {
          VERTER_GATE_LOCK: lk2,
          VERTER_GATE_TARGET_DIR: sharedTgt,
        });
        // Give any (pre-fix) sweep its TERM→grace→KILL window to land before we sample survival.
        await delay(2000);
        const stillAlive = countSweepMatchesForTarget(sharedTgt);
        if (refused.code !== EXIT_LOCK_REFUSED) {
          fail(
            `(i-survival) second gate returned ${refused.code}, expected LOCK-REFUSED (${EXIT_LOCK_REFUSED})`,
          );
        } else if (stillAlive >= 1) {
          pass(
            "(i-survival) HOLDER SURVIVAL: a LOCK-REFUSED gate left the holder's build-tool leaf ALIVE " +
              `(matches=${stillAlive}) — a non-acquiring gate touched no other process`,
          );
        } else {
          fail(
            "(i-survival) HOLDER SURVIVAL: the LOCK-REFUSED gate KILLED the holder's build-tool leaf " +
              "(0 sweep matches survive) — a non-acquiring gate must touch NO other process",
          );
        }
      }
      // Cleanup: kill the holder's group + the build-tool leaf, then release its lock dir if lingering.
      try {
        process.kill(-holder2.pid, "SIGKILL");
      } catch {
        try {
          holder2.kill("SIGKILL");
        } catch {
          /* ignore */
        }
      }
      spawnSync("pkill", ["-9", "-f", sharedTgt], { stdio: "ignore" });
      await delay(300);
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
  // (xi-b) WINDOWS ARCHIVE DEBUG-SIDECAR COMPLETENESS. cargo-nextest archives the executable test
  //      artifact but currently omits its hashed PDB. The allocation-site audit deliberately verifies
  //      named caller attribution, so the canonical archived surface must restore that sidecar from the
  //      runner-owned build tree before nextest launches the test. This drives the real helper with an
  //      injected filesystem: the required verter_napi PDB is copied to the matching extracted path;
  //      a missing source PDB is a loud setup error; non-Windows runs perform no copy.
  // --------------------------------------------------------------------------------------------------
  process.stderr.write("\n(xi-b) WINDOWS archive debug-sidecar completeness\n");
  {
    const suite = {
      "binary-id": "verter_napi",
      "binary-path": "C:\\gate\\extract\\target\\debug\\deps\\verter_napi-deadbeef.exe",
    };
    const source = "C:\\gate\\runner\\debug\\deps\\verter_napi-deadbeef.pdb";
    const destination = "C:\\gate\\extract\\target\\debug\\deps\\verter_napi-deadbeef.pdb";
    const copied = [];
    const present = new Set([source]);
    const result = ensureRequiredWindowsDebugSidecars({
      allSuites: [suite],
      runnerTarget: "C:\\gate\\runner",
      extractDir: "C:\\gate\\extract",
      windows: true,
      existsFn: (path) => present.has(path),
      copyFileFn: (from, to) => {
        copied.push([from, to]);
        present.add(to);
      },
    });
    const missing = ensureRequiredWindowsDebugSidecars({
      allSuites: [suite],
      runnerTarget: "C:\\gate\\runner",
      extractDir: "C:\\gate\\extract",
      windows: true,
      existsFn: () => false,
      copyFileFn: () => {
        throw new Error("copy must not run without the source PDB");
      },
    });
    const nonWindowsCopies = [];
    const nonWindows = ensureRequiredWindowsDebugSidecars({
      allSuites: [suite],
      runnerTarget: "C:\\gate\\runner",
      extractDir: "C:\\gate\\extract",
      windows: false,
      existsFn: () => true,
      copyFileFn: (...args) => nonWindowsCopies.push(args),
    });
    if (
      result.error ||
      result.copied !== 1 ||
      copied.length !== 1 ||
      copied[0][0] !== source ||
      copied[0][1] !== destination ||
      !missing.error ||
      nonWindows.error ||
      nonWindows.copied !== 0 ||
      nonWindowsCopies.length !== 0
    ) {
      fail(
        `(xi-b) sidecar helper mismatch: result=${JSON.stringify(result)} copied=${JSON.stringify(copied)} ` +
          `missing=${JSON.stringify(missing)} nonWindows=${JSON.stringify(nonWindows)} ` +
          `nonWindowsCopies=${JSON.stringify(nonWindowsCopies)}`,
      );
    } else {
      pass(
        "(xi-b) WINDOWS archive debug sidecar: required verter_napi PDB is copied beside the extracted " +
          "test binary, missing source fails setup, and non-Windows execution is a no-op",
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
  // (F1) FRESHNESS SHIM RESOLVER — platform-aware `node_modules/.bin` + PATH resolution with an INJECTED
  //      fake filesystem/env (no real `pnpm install`, no real fs). Mirrors the Rust freshness test's
  //      `resolve_executable_shim` / `locate_*_binary` semantics. Discriminators (each requires the new
  //      exports, which do NOT exist pre-change — so the whole scenario fails to even import pre-change):
  //        (a) POSIX: both extensionless shims present => resolve to the exact paths.
  //        (b) POSIX: a missing shim => null.
  //        (c) Windows (driven via the `windows` flag on this POSIX host): only a `.CMD` form on disk =>
  //            resolves the `.CMD` (the extensionless POSIX script is NOT runnable on Windows).
  //        (d) Windows: the extensionless form present but NONE of the .CMD/.cmd/.exe/.bat forms => null
  //            (Windows must not return the un-runnable extensionless shell script).
  //        (e) PATH fallback: a tool absent from node_modules/.bin but present in a PATH dir resolves.
  // --------------------------------------------------------------------------------------------------
  process.stderr.write("\n(F1) freshness shim resolver (injected fake fs/env)\n");
  {
    let ok = true;
    // The LOCAL `node_modules/.bin` resolver (`resolveLocalBinShim`) builds its base path with `node:path`
    // `join`, whose separator follows the HOST (this suite runs on macOS/Linux/Windows). For those local-bin
    // cases (a-d) we therefore derive the fixture key from the SAME host `join` the resolver uses (NOT a
    // hardcoded `/` or `\`), so the key and the resolver agree per host. The `windows` flag here drives the
    // SUFFIX selection (the `.CMD`/`.exe`/… Windows forms). NOTE: the `windows` flag ALSO selects the PATH
    // separator inside `appendPathComponentRaw` — that is exercised by the PATH-fallback cases (e), whose
    // candidates are built with `appendPathComponentRaw(dir, tool, windows)` (literal `/` for POSIX, `\` for
    // Windows), host-independently — NOT with host `join`.
    const REPO = "/repo"; // an absolute-ish root; the host's `join` produces the host-native form
    const baseBuf = join(REPO, "node_modules", ".bin", "buf");
    const baseOxfmt = join(REPO, "node_modules", ".bin", "oxfmt");
    // (a) POSIX-mode (windows=false): both extensionless shims present => resolve to the join'd base paths.
    {
      const present = new Set([baseBuf, baseOxfmt]);
      const ex = (p) => present.has(p);
      const buf = resolveLocalBinShim(REPO, "buf", ex, false);
      const oxfmt = resolveLocalBinShim(REPO, "oxfmt", ex, false);
      if (buf !== baseBuf || oxfmt !== baseOxfmt) {
        fail(
          `(F1 a) both-present resolve => buf='${buf}' oxfmt='${oxfmt}', expected '${baseBuf}'/'${baseOxfmt}'`,
        );
        ok = false;
      }
    }
    // (b) missing => null (the resolver does not fabricate a path).
    {
      const ex = () => false;
      const buf = resolveLocalBinShim(REPO, "buf", ex, false);
      if (buf !== null) {
        fail(`(F1 b) missing shim => '${buf}', expected null`);
        ok = false;
      }
    }
    // (c) Windows-mode (windows=true): only the `.CMD` form on disk resolves it (the extensionless POSIX
    //     script is not a runnable PE image on Windows). The expected key is the join'd base + ".CMD".
    {
      const cmdKey = `${baseBuf}.CMD`;
      const present = new Set([cmdKey]);
      const ex = (p) => present.has(p);
      const buf = resolveLocalBinShim(REPO, "buf", ex, true);
      if (buf !== cmdKey) {
        fail(`(F1 c) Windows .CMD shim => '${buf}', expected '${cmdKey}' (suffix resolution)`);
        ok = false;
      }
    }
    // (d) Windows-mode: the extensionless form present but NONE of the .CMD/.cmd/.exe/.bat forms => null
    //     (Windows must not return the un-runnable extensionless POSIX shell script).
    {
      const present = new Set([baseBuf]); // only the extensionless script, no runnable Windows form
      const ex = (p) => present.has(p);
      const buf = resolveLocalBinShim(REPO, "buf", ex, true);
      if (buf !== null) {
        fail(
          `(F1 d) Windows extensionless-only => '${buf}', expected null (the POSIX shell script is not a runnable PE image)`,
        );
        ok = false;
      }
    }
    // (e) PATH fallback resolves a tool present in a PATH dir but absent from node_modules/.bin. The
    //     windows-mode resolved key is built with `appendPathComponentRaw(dir, tool, true)` — the SAME
    //     NON-NORMALIZING `\`-joining the resolver now uses (mirroring the Windows Rust child's lexical
    //     dir.join) — so the fake-fs entry and the resolver's candidate match on every host. The POSIX-mode
    //     entries are built with `appendPathComponentRaw(dir, tool, false)` — the literal-`/` append the
    //     `windows:false` resolver itself uses — so the expected byte path matches the resolver's candidate
    //     on EVERY host (host `node:path.join` would emit `\` on a Windows host and diverge).
    {
      const pathDir = join(REPO, "tools", "bin");
      const onPath = `${appendPathComponentRaw(pathDir, "oxfmt", true)}.exe`; // Windows-mode resolves the .exe form
      const present = new Set([onPath]);
      const ex = (p) => present.has(p);
      const fakeEnv = { PATH: pathDir };
      const resolved = resolvePathShim("oxfmt", fakeEnv, ex, true);
      if (resolved !== onPath) {
        fail(`(F1 e) PATH-fallback (windows) resolve => '${resolved}', expected '${onPath}'`);
        ok = false;
      }
      // POSIX-mode PATH fallback: the extensionless form on PATH resolves directly.
      const onPathPosix = appendPathComponentRaw(pathDir, "oxfmt", false);
      const exPosix = (p) => p === onPathPosix;
      const resolvedPosix = resolvePathShim("oxfmt", fakeEnv, exPosix, false);
      if (resolvedPosix !== onPathPosix) {
        fail(
          `(F1 e) PATH-fallback (posix) resolve => '${resolvedPosix}', expected '${onPathPosix}'`,
        );
        ok = false;
      }
      // Sanity: the same env with NOTHING present => null (the fallback does not fabricate a path).
      const none = resolvePathShim("oxfmt", fakeEnv, () => false, true);
      if (none !== null) {
        fail(`(F1 e) PATH-fallback with nothing present => '${none}', expected null`);
        ok = false;
      }
    }
    // (f) [FIX-B — POSIX reads PATH case-EXACTLY] The Rust freshness test reads `std::env::var_os("PATH")`,
    //     which is CASE-SENSITIVE — it reads `PATH` and NEVER `Path`. So on POSIX a `Path`-only env (no
    //     `PATH`) must yield NO PATH (resolve null), matching Rust's `var_os("PATH") => None`, so the JS
    //     resolver and the Rust test AGREE on a `Path`-only env. On Windows env vars are case-INSENSITIVE,
    //     so `Path` and `PATH` name the SAME variable and either spelling resolves. DISCRIMINATES: pre-fix
    //     `resolvePathShim` read `env.PATH || env.Path`, so a POSIX `Path`-only env RESOLVED via the `Path`
    //     fallback (a JS-resolves / Rust-skips asymmetry) — the `=== null` assertion FAILS pre-fix.
    {
      const dir = join(REPO, "tools", "bin");
      const onPath = appendPathComponentRaw(dir, "buf", false); // extensionless POSIX shim (literal-`/` append, host-independent)
      const exPosix = (p) => p === onPath;
      // POSIX, Path-only (no PATH): the resolver does NOT consult Path => null (matches Rust None).
      const posixPathOnly = resolvePathShim("buf", { Path: dir }, exPosix, false);
      if (posixPathOnly !== null) {
        fail(
          `(F1 f) POSIX Path-only env => '${posixPathOnly}', expected null (POSIX reads PATH only, ` +
            `matching Rust's var_os("PATH") => None)`,
        );
        ok = false;
      }
      // POSIX, PATH present: resolves normally.
      const posixPath = resolvePathShim("buf", { PATH: dir }, exPosix, false);
      if (posixPath !== onPath) {
        fail(`(F1 f) POSIX PATH env => '${posixPath}', expected '${onPath}'`);
        ok = false;
      }
      // Windows is case-insensitive: a `Path`-only env DOES resolve (Path and PATH are the same var). The
      // windows-mode candidate is `\`-joined (appendPathComponentRaw), matching the Windows resolver.
      const onPathWin = `${appendPathComponentRaw(dir, "buf", true)}.CMD`;
      const exWin = (p) => p === onPathWin;
      const winPathOnly = resolvePathShim("buf", { Path: dir }, exWin, true);
      if (winPathOnly !== onPathWin) {
        fail(
          `(F1 f) Windows Path-only env => '${winPathOnly}', expected '${onPathWin}' ` +
            `(Windows env is case-insensitive, so Path === PATH)`,
        );
        ok = false;
      }
    }
    // (h) [P1 — WINDOWS NON-CANONICAL PATH CASING] On Windows env-var names fold case-INSENSITIVELY, so
    //     Rust's `std::env::var_os("PATH")` resolves a PATH var stored under ANY casing — INCLUDING
    //     `PaTh`/`path` — not just `PATH`/`Path`. The pre-fix `resolvePathShim` read `env.Path ?? env.PATH`
    //     (TWO exact spellings) and MISSED a `PaTh` key, so the JS preflight saw "buf absent ⇒ TOLERATE"
    //     while the Rust test's `var_os("PATH")` FOUND the `PaTh` value, resolved `buf`, and could FAIL on
    //     stale bindings — a silent fail-open. Post-fix `findPathEnvKey(env, true)` matches the PATH var by
    //     case-insensitive key, so the resolver reads exactly what Rust reads. DISCRIMINATES: pre-fix the
    //     `env.Path ?? env.PATH` read finds NEITHER `Path` nor `PATH` in `{ PaTh: dir }` ⇒ returns null;
    //     post-fix it resolves via the `PaTh` key, so the `!== null`/`=== onPathWin` assertion FAILS pre-fix.
    {
      const dir = join(REPO, "tools", "bin");
      // The windows-mode candidate is `\`-joined (appendPathComponentRaw), matching the Windows resolver.
      const onPathWin = `${appendPathComponentRaw(dir, "buf", true)}.CMD`;
      const exWin = (p) => p === onPathWin;
      // The KEY discriminating assertion: a `PaTh`-cased key resolves on Windows (pre-fix => null miss).
      const winMixedCase = resolvePathShim("buf", { PaTh: dir }, exWin, true);
      if (winMixedCase !== onPathWin) {
        fail(
          `(F1 h) Windows PaTh-cased env => '${winMixedCase}', expected '${onPathWin}' (Windows env names ` +
            `fold case-insensitively, so var_os("PATH") reads a PaTh key — pre-fix env.Path ?? env.PATH ` +
            `MISSED it ⇒ null, a silent fail-open)`,
        );
        ok = false;
      }
      // A lowercase `path` key also resolves (the same case-insensitive fold).
      const winLower = resolvePathShim("buf", { path: dir }, exWin, true);
      if (winLower !== onPathWin) {
        fail(`(F1 h) Windows lowercase path-cased env => '${winLower}', expected '${onPathWin}'`);
        ok = false;
      }
      // POSIX PARITY GUARD: on POSIX the SAME `PaTh` key is NOT the PATH var (Rust var_os is case-EXACT
      // there), so the resolver must NOT find it => null. This proves the case-insensitive match is
      // Windows-ONLY and POSIX stays case-exact (a POSIX PaTh fallback would be a fail-open asymmetry).
      const posixMixedCase = resolvePathShim("buf", { PaTh: dir }, exWin, false);
      if (posixMixedCase !== null) {
        fail(
          `(F1 h) POSIX PaTh-cased env => '${posixMixedCase}', expected null (POSIX var_os("PATH") is ` +
            `case-exact — a PaTh key is NOT the PATH var on POSIX)`,
        );
        ok = false;
      }
      // findPathEnvKey direct: Windows matches PaTh; POSIX returns null for the same env (case-exact).
      if (findPathEnvKey({ PaTh: dir }, true) !== "PaTh") {
        fail(
          `(F1 h) findPathEnvKey({PaTh}, windows=true) => ${JSON.stringify(findPathEnvKey({ PaTh: dir }, true))}, expected "PaTh"`,
        );
        ok = false;
      }
      if (findPathEnvKey({ PaTh: dir }, false) !== null) {
        fail(
          `(F1 h) findPathEnvKey({PaTh}, windows=false) => ${JSON.stringify(findPathEnvKey({ PaTh: dir }, false))}, expected null (POSIX case-exact)`,
        );
        ok = false;
      }
      // Deterministic tie-break: an exact PATH wins over a Path/PaTh when several casings coexist.
      if (findPathEnvKey({ PaTh: "a", Path: "b", PATH: "c" }, true) !== "PATH") {
        fail(`(F1 h) findPathEnvKey tie-break: expected exact "PATH" to win over Path/PaTh`);
        ok = false;
      }
      if (findPathEnvKey({ PaTh: "a", Path: "b" }, true) !== "Path") {
        fail(
          `(F1 h) findPathEnvKey tie-break: expected "Path" to win over PaTh when no exact PATH`,
        );
        ok = false;
      }
      // A non-string value at a case-insensitive PATH key is NOT treated as the PATH var (no forged key).
      if (findPathEnvKey({ PaTh: 123 }, true) !== null) {
        fail(`(F1 h) findPathEnvKey: a non-string PaTh value must not match (got non-null)`);
        ok = false;
      }
    }
    // (g) [P3] IS-FILE DEFAULT (mirrors the Rust `Path::is_file()`): a DIRECTORY at the shim path must NOT
    //     count as present. This drives the resolver through the LIVE `defaultIsFile` DEFAULT (NO injected
    //     predicate) against a REAL on-disk fixture — so it exercises the ACTUAL `defaultIsFile` →
    //     `statSync().isFile()` change, not the injection plumbing. We create a real temp `node_modules/.bin`
    //     with `buf` as a real DIRECTORY and `oxfmt` as a real FILE, then call `resolveLocalBinShim` with
    //     ONLY (repoRoot, tool) — the predicate defaults to `defaultIsFile`. DISCRIMINATES against the
    //     pre-fix tree: pre-fix the resolver defaulted to a bare `existsSync`, which returns TRUE for a
    //     directory, so `resolveLocalBinShim(tmp, "buf")` RESOLVED the directory (NOT null) — the assertion
    //     fails pre-fix. Post-fix `defaultIsFile` rejects the directory ⇒ null ⇒ passes. POSIX-mode
    //     (windows=false) so the extensionless `buf`/`oxfmt` names ARE the shim names on every host.
    {
      const fxRoot = mkdtempSync(join(tmpdir(), "gate-isfile-"));
      try {
        const bin = join(fxRoot, "node_modules", ".bin");
        mkdirSync(bin, { recursive: true });
        // `buf` is a real DIRECTORY (the corrupt-shim case); `oxfmt` is a real FILE (the positive control).
        mkdirSync(join(bin, "buf"));
        writeFileSync(join(bin, "oxfmt"), "#!/bin/sh\n");

        // Live default predicate: NO `isFileFn` argument — `resolveLocalBinShim` falls back to `defaultIsFile`.
        const bufResolved = resolveLocalBinShim(fxRoot, "buf", undefined, false);
        if (bufResolved !== null) {
          fail(
            `(F1 g) POSIX directory-at-shim-path (live defaultIsFile) => '${bufResolved}', expected null — a ` +
              `directory is not a runnable shim; pre-fix the bare existsSync default RESOLVED it (NOT null), so ` +
              `this assertion FAILS pre-fix and PASSES post-fix (exercises the real defaultIsFile change)`,
          );
          ok = false;
        }
        // Positive control: a real FILE still resolves through the SAME live default — proving the rejection
        // is the is-file predicate discriminating, not an unconditional null.
        const oxfmtResolved = resolveLocalBinShim(fxRoot, "oxfmt", undefined, false);
        const expectedOxfmt = join(bin, "oxfmt");
        if (oxfmtResolved !== expectedOxfmt) {
          fail(
            `(F1 g) POSIX file-at-shim-path (live defaultIsFile) => '${oxfmtResolved}', expected '${expectedOxfmt}' ` +
              `(a real file still resolves through the default predicate)`,
          );
          ok = false;
        }
      } finally {
        rmSync(fxRoot, { recursive: true, force: true });
      }
    }
    // (i) [FIX-A — PLATFORM-DELIMITER SPLIT] `resolvePathShim` must split the PATH value on the
    //     PLATFORM-CORRECT delimiter (`;` when `windows===true`), NOT the host `node:path` delimiter. On a
    //     POSIX host the host delimiter is `:`, so before the fix a `windows:true` resolve split a
    //     `;`-separated PATH on `:` and treated the WHOLE "dirA;dirB" as a SINGLE directory. We build a
    //     `windows:true` SEMICOLON-delimited PATH with TWO entries where the shim (`buf.CMD`) exists ONLY in
    //     the SECOND (non-first) entry, and inject `isFileFn` so it works on a POSIX host. DISCRIMINATES:
    //       - POST-FIX the resolver splits on `;`, walks `dirA` (no shim) then `dirB`, and resolves
    //         `<dirB>/buf.CMD`.
    //       - PRE-FIX the resolver splits the whole "dirA;dirB" string on `:` ⇒ ONE dir literally
    //         "dirA;dirB"; `<dirA;dirB>/buf.CMD` is not in the fake fs ⇒ returns null.
    //     The single-entry colon-free Windows cases (e)/(f)/(h) above pass vacuously for `;`-splitting; this
    //     is the case that genuinely exercises multi-entry `;`-separated Windows PATH resolution.
    //     This case is tracked under its OWN `okI` flag and emits its OWN pass/fail so it is an independently
    //     visible self-test (it does not ride the broader F1 pass).
    {
      let okI = true;
      const dirA = join(REPO, "tools", "binA");
      const dirB = join(REPO, "tools", "binB");
      // the shim lives ONLY in the SECOND entry; the windows-mode candidate is `\`-joined
      // (appendPathComponentRaw), matching the Windows resolver's now-lexical dir.join
      const onPathWin = `${appendPathComponentRaw(dirB, "buf", true)}.CMD`;
      const exWin = (p) => p === onPathWin;
      const semicolonPath = `${dirA};${dirB}`; // a genuine two-entry Windows PATH (`;`-separated)
      const resolved = resolvePathShim("buf", { PATH: semicolonPath }, exWin, true);
      if (resolved !== onPathWin) {
        fail(
          `(F1 i) Windows ;-separated two-entry PATH ${JSON.stringify(semicolonPath)} (shim only in the ` +
            `SECOND entry) => ${JSON.stringify(resolved)}, expected ${JSON.stringify(onPathWin)} — the ` +
            `resolver must split on the platform delimiter ";" (pre-fix it split the whole string on ":" ` +
            `⇒ a single dir "dirA;dirB" ⇒ the shim in the second entry is NOT found ⇒ null, a fail-open)`,
        );
        okI = false;
      }
      // CONTROL: a single-entry `;`-PATH with the shim PRESENT still resolves (proves the assertion above
      // discriminates on the SPLIT, not on the shim being unreachable for some unrelated reason).
      const onlyB = resolvePathShim("buf", { PATH: dirB }, exWin, true);
      if (onlyB !== onPathWin) {
        fail(
          `(F1 i) control: single-entry Windows PATH ${JSON.stringify(dirB)} => ${JSON.stringify(onlyB)}, ` +
            `expected ${JSON.stringify(onPathWin)} (the shim IS present in dirB)`,
        );
        okI = false;
      }
      if (okI) {
        pass(
          "(F1 i) [FIX-A] resolvePathShim splits a windows:true PATH on the platform delimiter ';': a " +
            "two-entry ';'-separated PATH with the shim only in the SECOND entry resolves it (pre-fix the " +
            "host ':' split treated 'dirA;dirB' as one dir and missed the second entry ⇒ null)",
        );
      }
    }
    // Cross-check the lower-level resolver's Windows suffix ORDER directly (.CMD wins over .cmd/.exe). This
    // resolver appends `.<ext>` to the base WITHOUT `join`, so a separator-free base is host-independent.
    {
      const present = new Set(["t.cmd", "t.exe", "t.CMD"]);
      const ex = (p) => present.has(p);
      const r = resolveExecutableShim("t", ex, true);
      if (r !== "t.CMD") {
        fail(`(F1) Windows suffix order: expected .CMD to win, got '${r}'`);
        ok = false;
      }
    }
    if (ok) {
      pass(
        "(F1) FRESHNESS SHIM RESOLVER: POSIX both-present resolve / missing => null; Windows resolves a .CMD " +
          "shim and rejects an extensionless-only (un-runnable) shim; PATH fallback resolves a PATH-only tool; " +
          "Windows reads the PATH var by CASE-INSENSITIVE key (a PaTh/path-cased env resolves, matching Rust " +
          "var_os) while POSIX stays case-EXACT (a PaTh key => null); findPathEnvKey tie-breaks PATH>Path>any " +
          "and rejects a non-string value; " +
          "the LIVE defaultIsFile default (no injected predicate) rejects a REAL on-disk DIRECTORY at a shim " +
          "path while a real FILE still resolves (mirrors Rust Path::is_file — discriminating: pre-fix the bare " +
          "existsSync default resolved the directory as present)",
      );
    }
  }

  // --------------------------------------------------------------------------------------------------
  // (F2) FRESHNESS PREFLIGHT — `preflightFreshnessTooling` with an INJECTED `runInstall` (no real pnpm) and
  //      an INJECTED fake fs/env, under the BUF-ABSENCE-ONLY tolerance model + the POSITIVE-PNPM-PROBE model
  //      (the codex ruling). Two orthogonal axes:
  //        - TOLERANCE keys on `buf` SPECIFICALLY (the skip-determining tool): tolerance is allowed ONLY when
  //          `buf` is not resolvable (local + PATH) — exactly the condition under which the Rust byte-pin test
  //          SKIPS. `oxfmt` is a REQUIRED-WHEN-buf-PRESENT canonical tool: a missing `oxfmt` with `buf`
  //          available is a LOUD setup-fail (a degraded un-oxfmt'd byte-compare can false-positive), NEVER
  //          tolerate, NEVER a degraded run. So `oxfmt` absence NEVER grants tolerance.
  //        - WHETHER TO ATTEMPT THE INSTALL keys on a POSITIVE platform-aware `pnpm` resolver fact determined
  //          BEFORE the install. pnpm is resolved via PATH ONLY (with the WINDOWS_SHIM_EXTS suffix on Windows),
  //          to MATCH how `pnpmInstallCommand` actually launches it: the RESOLVED pnpm path (POSIX: the
  //          resolved binary directly; Windows: the resolved path quoted through `cmd.exe` with
  //          windowsVerbatimArguments) — a local `node_modules/.bin/pnpm` shim the launcher never invokes
  //          does NOT count. NOT inferred from the install's spawnError. So each fake here sets pnpm's
  //          PATH-resolvability explicitly: a case that must REACH the install puts pnpm on the fake PATH; a
  //          no-install case leaves it off PATH (and asserts runInstall is NOT called). Case (m) below proves
  //          the PATH-only rule directly: a pnpm matchable ONLY at a node_modules/.bin path (not on PATH) is
  //          NOT resolvable, so the install is NOT attempted.
  //      NOTE: because `runInstall` is injected here, F2 does NOT exercise the REAL install LAUNCH command (the
  //      Windows `.cmd`-via-`spawn` defect) — that decision is `pnpmInstallCommand`, covered by (F4). Each case
  //      discriminates via the action/allowed/runInstall-count triple.
  //        (a) both local shims present up front => { allowed:false, action:"already-present" } AND
  //            runInstall NOT called (pnpm is not even probed).
  //        (b) missing up front, pnpm RESOLVABLE, install SUCCEEDS and the re-resolve finds both =>
  //            { allowed:false, action:"installed" } AND runInstall called EXACTLY once.
  //        (c) POSIX pnpm NOT resolvable (positive probe fails) + `buf` STILL missing => { allowed:true,
  //            action:"tolerate-genuinely-absent" } AND runInstall called 0 (the install is NOT attempted).
  //        (d) pnpm RESOLVABLE, install LAUNCHED but exited non-zero (frozen-lockfile mismatch) and tools
  //            still missing => { allowed:false, action:"setup-fail" } (LOUD; EXIT_USAGE 127), called once.
  //        (e) THE BYPASS REGRESSION: pnpm RESOLVABLE, install LAUNCHED, non-zero exit BUT buf+oxfmt
  //            resolvable on PATH => { allowed:false, action:"setup-fail" } — a launched non-zero install
  //            FAILS LOUD regardless of PATH resolvability (precedence over the resolve branch). Pre-fix this
  //            returned "path-fallback" and the gate ran on a frozen-lockfile mismatch.
  //        (f) pnpm RESOLVABLE, install EXIT 0 but the shims still did not appear (no PATH fallback) =>
  //            { allowed:false, action:"setup-fail" } AND called once (an exit-0 install that produced
  //            nothing is a setup failure, not tolerated).
  //        (g) [P1] WINDOWS pnpm GENUINELY ABSENT (no pnpm.CMD/.cmd/.exe/.bat on PATH, no local pnpm) +
  //            tools missing + a runInstall that returns { code:1, spawnError:false } => { allowed:true,
  //            action:"tolerate-genuinely-absent" } AND runInstall called 0. DISCRIMINATES: pre-fix inferred
  //            absence from spawnError, so on Windows with pnpm absent it CALLED runInstall (count 1) and —
  //            spawnError being false (cmd.exe launched) — returned setup-fail, NOT tolerate.
  //        (h) [P1] WINDOWS pnpm RESOLVABLE (pnpm.CMD on PATH) + install non-zero (+ buf/oxfmt on PATH) =>
  //            { allowed:false, action:"setup-fail" } AND runInstall called 1 (launched-non-zero precedence).
  //        (i) [P1] THE FAIL-OPEN CLOSURE — `buf` PRESENT (PATH) + `oxfmt` ABSENT + pnpm ABSENT =>
  //            { allowed:false, action:"setup-fail" } AND runInstall called 0. The Rust test would RUN (buf is
  //            present) but the bindings would not be canonically formatted — a degraded un-oxfmt'd byte-
  //            compare that can false-positive; FAIL LOUD, do NOT tolerate. DISCRIMINATES against the pre-fix
  //            "either tool missing ⇒ tolerate" code, which returned { allowed:true,
  //            action:"tolerate-genuinely-absent" } here — the exact fail-open this change closes.
  //        (j) [P1] `buf` ABSENT + pnpm ABSENT => { allowed:true, action:"tolerate-genuinely-absent" } AND
  //            runInstall called 0 (buf is the skip-determining tool; the Rust test would skip).
  //        (k) [P1] pnpm PRESENT, install EXIT 0, `buf` now present BUT `oxfmt` STILL absent =>
  //            { allowed:false, action:"setup-fail" } AND runInstall called 1 (an exit-0 install that produced
  //            `buf` but not the required `oxfmt` is a setup failure, never tolerated).
  //        (l) [P1] pnpm ABSENT + buf+oxfmt BOTH on PATH (PATH-only, NOT in node_modules) =>
  //            { allowed:false, action:"path-fallback" } AND runInstall called 0 (the tools exist on PATH, so
  //            a freshness FAIL is a real regression; the install is not attempted because pnpm is absent).
  // --------------------------------------------------------------------------------------------------
  process.stderr.write("\n(F2) freshness preflight (injected runInstall + fake fs)\n");
  {
    let ok = true;
    const REPO = "/repo";
    // Derive the fake-fs keys the SAME WAY the resolver builds its candidates, host-independently — NOT via
    // host `node:path.join` for the PATH keys (which would emit `\` on a Windows host and mismatch the
    // `windows:false` resolver's literal-`/` candidate). Concretely:
    //   - POSIX-mode (windows:false) PATH keys are built with `appendPathComponentRaw(dir, tool, false)` (the
    //     literal-`/` append the `windows:false` resolver, `resolvePathShim`/`resolvePnpm`, uses) — host-
    //     independent on macOS/Linux/Windows.
    //   - The LOCAL `node_modules/.bin/<tool>` keys (`binBuf`/`binOxfmt`/`binPnpm`) stay host `join(...)`,
    //     matching `resolveLocalBinShim`, which itself uses host `join(repoRoot, "node_modules", ".bin", tool)`
    //     on EVERY host — so the key and the resolver agree per host.
    //   - Windows-mode (windows:true) PATH keys are built with `appendPathComponentRaw(dir, tool, true)` (the
    //     `\`-append the Windows resolver uses) + the `.CMD` suffix.
    // The POSIX-mode cases (windows:false) key buf/oxfmt on the extensionless `node_modules/.bin/<tool>` form
    // (the Rust freshness test prefers those version-locked shims). pnpm, however, is resolved via PATH ONLY
    // (to match the bare-`pnpm` launch), so a POSIX case that must REACH the install (b/d/e/f/k) puts pnpm on a
    // fake PATH dir (`pathPnpm` under `INSTALL_PATHBIN`) — NOT in node_modules — so the positive PATH probe
    // passes. The tolerate case (c) leaves pnpm off PATH so the probe fails and the install is NOT attempted.
    // Case (m) deliberately puts pnpm ONLY at `binPnpm` (node_modules/.bin, NOT on PATH) to prove that does
    // NOT count.
    const binBuf = join(REPO, "node_modules", ".bin", "buf");
    const binOxfmt = join(REPO, "node_modules", ".bin", "oxfmt");
    const binPnpm = join(REPO, "node_modules", ".bin", "pnpm");
    // A PATH dir carrying pnpm for the install-reaching POSIX cases (PATH-resolvable pnpm, matching the launch).
    const INSTALL_PATHBIN = "/opt/installbin";
    const pathPnpm = appendPathComponentRaw(INSTALL_PATHBIN, "pnpm", false);
    const installEnv = { PATH: INSTALL_PATHBIN }; // PATH carrying pnpm so the positive probe passes
    const noEnv = { PATH: "" }; // no PATH fallback in these cases unless stated
    // A mutable "filesystem" + a runInstall that can flip it (modelling install populating node_modules).
    const makeEx = (set) => (p) => set.has(p);

    // (a) both present up front => already-present, runInstall NOT called (pnpm is NOT even probed).
    {
      const present = new Set([binBuf, binOxfmt]);
      let calls = 0;
      const runInstall = async () => {
        calls++;
        return { code: 0, reason: "", spawnError: false };
      };
      const r = await preflightFreshnessTooling({
        repoRoot: REPO,
        env: noEnv,
        runInstall,
        existsSyncFn: makeEx(present),
        windows: false,
      });
      if (r.freshnessToleranceAllowed !== false || r.action !== "already-present") {
        fail(
          `(F2 a) both-present => allowed=${r.freshnessToleranceAllowed} action='${r.action}', expected false/already-present`,
        );
        ok = false;
      }
      if (calls !== 0) {
        fail(`(F2 a) runInstall called ${calls}x with both tools already present — must be 0`);
        ok = false;
      }
    }

    // (b) missing then install SUCCEEDS (populates node_modules) => installed, allowed:false, called once.
    //     pnpm IS resolvable (on PATH — matching the bare-`pnpm` launch) so the positive pnpm probe passes and
    //     the install runs.
    {
      const present = new Set([pathPnpm]); // pnpm on PATH so the probe passes; buf/oxfmt absent up front
      let calls = 0;
      const runInstall = async () => {
        calls++;
        present.add(binBuf);
        present.add(binOxfmt); // the "install" populated the shims
        return { code: 0, reason: "", spawnError: false };
      };
      const r = await preflightFreshnessTooling({
        repoRoot: REPO,
        env: installEnv,
        runInstall,
        existsSyncFn: makeEx(present),
        windows: false,
      });
      if (r.freshnessToleranceAllowed !== false || r.action !== "installed") {
        fail(
          `(F2 b) missing-then-installed => allowed=${r.freshnessToleranceAllowed} action='${r.action}', expected false/installed`,
        );
        ok = false;
      }
      if (calls !== 1) {
        fail(`(F2 b) runInstall called ${calls}x, expected exactly 1`);
        ok = false;
      }
    }

    // (c) POSIX genuinely-absent: pnpm NOT resolvable (positive probe FAILS) + `buf` still missing =>
    //     tolerate-genuinely-absent, allowed:true, AND runInstall called 0 (the install is NOT attempted
    //     because pnpm is absent). Under the buf-absence-only model the tolerate branch keys on `buf` being
    //     unresolvable (the empty set has no buf); under the positive-probe model it is reached by pnpm being
    //     unresolvable, NOT by an install spawnError — so the fake leaves pnpm AND buf off the (empty) PATH
    //     and out of node_modules, and the runInstall counter MUST stay 0.
    {
      const present = new Set(); // no pnpm, no buf/oxfmt
      let calls = 0;
      const runInstall = async () => {
        calls++;
        return { code: 127, reason: "", spawnError: true };
      };
      const r = await preflightFreshnessTooling({
        repoRoot: REPO,
        env: noEnv,
        runInstall,
        existsSyncFn: makeEx(present),
        windows: false,
      });
      if (r.freshnessToleranceAllowed !== true || r.action !== "tolerate-genuinely-absent") {
        fail(
          `(F2 c) pnpm-absent+still-missing => allowed=${r.freshnessToleranceAllowed} action='${r.action}', expected true/tolerate-genuinely-absent`,
        );
        ok = false;
      }
      if (calls !== 0) {
        fail(
          `(F2 c) runInstall called ${calls}x with pnpm genuinely absent — must be 0 (the install is not attempted when pnpm is unresolvable)`,
        );
        ok = false;
      }
    }

    // (d) pnpm present, install LAUNCHED, non-zero exit, tools still missing => setup-fail, allowed:false,
    //     runInstall called 1. pnpm must be PATH-resolvable for the install to run at all.
    {
      const present = new Set([pathPnpm]); // pnpm on PATH so the probe passes; buf/oxfmt stay missing
      let calls = 0;
      const runInstall = async () => {
        calls++;
        return { code: 1, reason: "", spawnError: false };
      };
      const r = await preflightFreshnessTooling({
        repoRoot: REPO,
        env: installEnv,
        runInstall,
        existsSyncFn: makeEx(present),
        windows: false,
      });
      if (r.freshnessToleranceAllowed !== false || r.action !== "setup-fail") {
        fail(
          `(F2 d) install-nonzero+still-missing => allowed=${r.freshnessToleranceAllowed} action='${r.action}', expected false/setup-fail`,
        );
        ok = false;
      }
      if (calls !== 1) {
        fail(`(F2 d) runInstall called ${calls}x, expected exactly 1`);
        ok = false;
      }
    }

    // (e) THE BYPASS REGRESSION (codex precedence). pnpm present + install LAUNCHED, NON-ZERO exit
    //     (frozen-lockfile mismatch: { code:1, spawnError:false }) BUT buf+oxfmt are independently
    //     resolvable on PATH (NOT in node_modules). A LAUNCHED non-zero install must take PRECEDENCE over
    //     the resolve-based branch and FAIL LOUD as setup REGARDLESS of PATH resolvability — never silently
    //     proceed as "path-fallback". DISCRIMINATES against the pre-fix code: pre-fix the `allResolved`
    //     branch fired FIRST (the tools are on PATH) and returned { action:"path-fallback", allowed:false }
    //     WITHOUT ever inspecting `installRes.code`, so the gate ran on a frozen-lockfile mismatch. Post-fix
    //     the launched-non-zero check sits BEFORE the resolve branch and returns setup-fail. pnpm is on the
    //     same PATH dir so the positive probe passes; node_modules shims are absent.
    {
      const PATHBIN = "/opt/fakebin";
      const pathBuf = appendPathComponentRaw(PATHBIN, "buf", false);
      const pathOxfmt = appendPathComponentRaw(PATHBIN, "oxfmt", false);
      const pathPnpm = appendPathComponentRaw(PATHBIN, "pnpm", false);
      const present = new Set([pathBuf, pathOxfmt, pathPnpm]); // on PATH only; node_modules shims absent
      const pathEnv = { PATH: PATHBIN };
      let calls = 0;
      const runInstall = async () => {
        calls++;
        return { code: 1, reason: "", spawnError: false };
      };
      const r = await preflightFreshnessTooling({
        repoRoot: REPO,
        env: pathEnv,
        runInstall,
        existsSyncFn: makeEx(present),
        windows: false,
      });
      if (r.freshnessToleranceAllowed !== false || r.action !== "setup-fail") {
        fail(
          `(F2 e) install-nonzero + tools-on-PATH => allowed=${r.freshnessToleranceAllowed} action='${r.action}', ` +
            `expected false/setup-fail (a launched non-zero install must FAIL LOUD regardless of PATH ` +
            `resolvability; pre-fix this returned path-fallback and the gate ran on a frozen-lockfile mismatch)`,
        );
        ok = false;
      }
      if (calls !== 1) {
        fail(
          `(F2 e) runInstall called ${calls}x, expected exactly 1 (pnpm resolvable => install runs)`,
        );
        ok = false;
      }
    }

    // (f) pnpm present + install LAUNCHED with EXIT 0 but the shims still did NOT appear (and no PATH
    //     fallback) => setup-fail, allowed:false, runInstall called 1. An exit-0 install that did not
    //     produce the tools is still a deterministic setup failure (the final fallthrough), distinct from
    //     the launched-non-zero case (d) and from the pnpm-absent tolerate case (c). DISCRIMINATES: an
    //     exit-0 must NOT be tolerated.
    {
      const present = new Set([pathPnpm]); // pnpm on PATH; install "succeeds" but produces nothing
      let calls = 0;
      const runInstall = async () => {
        calls++;
        return { code: 0, reason: "", spawnError: false };
      };
      const r = await preflightFreshnessTooling({
        repoRoot: REPO,
        env: installEnv,
        runInstall,
        existsSyncFn: makeEx(present),
        windows: false,
      });
      if (r.freshnessToleranceAllowed !== false || r.action !== "setup-fail") {
        fail(
          `(F2 f) install-exit0 + tools-still-missing => allowed=${r.freshnessToleranceAllowed} action='${r.action}', ` +
            `expected false/setup-fail (an exit-0 install that did not produce the tools is a setup failure, not tolerated)`,
        );
        ok = false;
      }
      if (calls !== 1) {
        fail(
          `(F2 f) runInstall called ${calls}x, expected exactly 1 (pnpm resolvable => install runs)`,
        );
        ok = false;
      }
    }

    // (g) [P1] WINDOWS pnpm GENUINELY ABSENT tolerates (the constrained-runner contract, restored on
    //     Windows). windows:true; the fake PATH has NONE of pnpm.CMD/.cmd/.exe/.bat (and no local pnpm); no
    //     buf/oxfmt; the fake runInstall increments a counter and returns { code:1, spawnError:false }
    //     (modelling a cmd.exe wrapper that LAUNCHED + a non-zero exit, the post-cmd.exe-wrapper shape).
    //     EXPECT: tolerate-genuinely-absent / allowed:true / runInstall called 0 — the install is NOT
    //     attempted because the POSITIVE pnpm probe fails. DISCRIMINATES against the pre-fix tree: pre-fix
    //     inferred genuinely-absent from the install spawnError, so on Windows with pnpm absent it CALLED
    //     runInstall (count 1, not 0) and — because spawnError is false here (cmd.exe launched) — classified
    //     setup-fail, NOT tolerate. The post-fix positive probe never runs the install and tolerates.
    {
      const winEnv = { PATH: "" }; // empty PATH: no pnpm.CMD/.cmd/.exe/.bat anywhere
      const present = new Set(); // no local pnpm, no buf/oxfmt
      let calls = 0;
      const runInstall = async () => {
        calls++;
        return { code: 1, reason: "", spawnError: false };
      };
      const r = await preflightFreshnessTooling({
        repoRoot: REPO,
        env: winEnv,
        runInstall,
        existsSyncFn: makeEx(present),
        windows: true,
      });
      if (r.freshnessToleranceAllowed !== true || r.action !== "tolerate-genuinely-absent") {
        fail(
          `(F2 g) WINDOWS pnpm-absent => allowed=${r.freshnessToleranceAllowed} action='${r.action}', ` +
            `expected true/tolerate-genuinely-absent (positive pnpm probe fails ⇒ tolerate; pre-fix called ` +
            `runInstall and mis-classified setup-fail off the cmd.exe-wrapper exit code)`,
        );
        ok = false;
      }
      if (calls !== 0) {
        fail(
          `(F2 g) WINDOWS pnpm-absent: runInstall called ${calls}x — must be 0 (the install is not ` +
            `attempted when the positive pnpm probe fails; pre-fix called it 1x)`,
        );
        ok = false;
      }
    }

    // (h) [P1] WINDOWS deterministic install failure still setup-fails. windows:true; the fake PATH
    //     contains pnpm.CMD (so the positive probe PASSES) PLUS buf.CMD/oxfmt.CMD on PATH (to prove the
    //     launched-non-zero precedence over path-fallback); the fake runInstall returns { code:1,
    //     reason:"", spawnError:false }. EXPECT: setup-fail / allowed:false / runInstall called 1. The PATH
    //     keys are built with the SAME `appendPathComponentRaw(dir, tool, true)` `\`-join + the `.CMD` suffix
    //     the Windows resolver tries first (mirroring the Windows Rust child's lexical dir.join), and the
    //     windows:true resolver splits the PATH on the LITERAL ";" (`pathDelimiterFor(true)`) host-independently,
    //     so the fake is host-portable.
    {
      const WINBIN = "winpath"; // a simple relative PATH component; the windows:true resolver splits on the literal ";" (host-independent)
      const winEnv = { PATH: WINBIN };
      const cmdPnpm = `${appendPathComponentRaw(WINBIN, "pnpm", true)}.CMD`;
      const cmdBuf = `${appendPathComponentRaw(WINBIN, "buf", true)}.CMD`;
      const cmdOxfmt = `${appendPathComponentRaw(WINBIN, "oxfmt", true)}.CMD`;
      const present = new Set([cmdPnpm, cmdBuf, cmdOxfmt]);
      let calls = 0;
      const runInstall = async () => {
        calls++;
        return { code: 1, reason: "", spawnError: false };
      };
      const r = await preflightFreshnessTooling({
        repoRoot: REPO,
        env: winEnv,
        runInstall,
        existsSyncFn: makeEx(present),
        windows: true,
      });
      if (r.freshnessToleranceAllowed !== false || r.action !== "setup-fail") {
        fail(
          `(F2 h) WINDOWS pnpm-present + install-nonzero (+ tools on PATH) => allowed=${r.freshnessToleranceAllowed} ` +
            `action='${r.action}', expected false/setup-fail (a launched non-zero install FAILS LOUD even with ` +
            `buf/oxfmt resolvable on PATH — launched-non-zero takes precedence over path-fallback)`,
        );
        ok = false;
      }
      if (calls !== 1) {
        fail(
          `(F2 h) WINDOWS pnpm-present: runInstall called ${calls}x, expected exactly 1 (positive pnpm probe passes ⇒ install runs)`,
        );
        ok = false;
      }
    }

    // (i) [P1] THE FAIL-OPEN CLOSURE — buf PRESENT (on PATH) + oxfmt ABSENT + pnpm ABSENT => setup-fail,
    //     allowed:false, runInstall called 0. With buf present the Rust byte-pin test RUNS but the
    //     regenerated bindings are not canonically formatted (oxfmt is the conditional formatter) — a
    //     degraded un-oxfmt'd byte-compare that can false-positive; FAIL LOUD, do NOT tolerate, do NOT run
    //     degraded. The PATH carries buf only (not oxfmt, not pnpm); node_modules is empty. DISCRIMINATES
    //     against the pre-fix "either tool missing ⇒ tolerate" code, which classified THIS exact case as
    //     tolerate-genuinely-absent/allowed:true (the fail-open) — so this assertion FAILS pre-fix and PASSES
    //     post-fix. It is the single most important [P1] case.
    {
      const PATHBIN = "/opt/fakebin";
      const pathBuf = appendPathComponentRaw(PATHBIN, "buf", false); // buf on PATH; oxfmt + pnpm intentionally absent
      const present = new Set([pathBuf]);
      const pathEnv = { PATH: PATHBIN };
      let calls = 0;
      const runInstall = async () => {
        calls++;
        return { code: 0, reason: "", spawnError: false };
      };
      const r = await preflightFreshnessTooling({
        repoRoot: REPO,
        env: pathEnv,
        runInstall,
        existsSyncFn: makeEx(present),
        windows: false,
      });
      if (r.freshnessToleranceAllowed !== false || r.action !== "setup-fail") {
        fail(
          `(F2 i) buf-present + oxfmt-absent + pnpm-absent => allowed=${r.freshnessToleranceAllowed} ` +
            `action='${r.action}', expected false/setup-fail (buf present ⇒ the Rust test RUNS but oxfmt is ` +
            `required to canonically format; a missing oxfmt is a LOUD setup-fail, NEVER tolerate — the ` +
            `fail-open closure. Pre-fix "either tool ⇒ tolerate" returned tolerate-genuinely-absent/true here)`,
        );
        ok = false;
      }
      if (calls !== 0) {
        fail(
          `(F2 i) buf-present/oxfmt-absent/pnpm-absent: runInstall called ${calls}x — must be 0 (pnpm absent ⇒ install not attempted)`,
        );
        ok = false;
      }
    }

    // (j) [P1] buf ABSENT + pnpm ABSENT => tolerate-genuinely-absent, allowed:true, runInstall called 0. buf
    //     is the skip-determining tool; with buf unresolvable the Rust byte-pin test SKIPS, so the exact pair
    //     is tolerated REGARDLESS of oxfmt. Neither buf nor pnpm is anywhere (empty set, empty PATH). This
    //     mirrors case (c) but states the buf-specific tolerance gate explicitly (oxfmt absent too, yet the
    //     verdict is tolerate because buf is the gate).
    {
      const present = new Set(); // no buf, no oxfmt, no pnpm
      let calls = 0;
      const runInstall = async () => {
        calls++;
        return { code: 1, reason: "", spawnError: false };
      };
      const r = await preflightFreshnessTooling({
        repoRoot: REPO,
        env: noEnv,
        runInstall,
        existsSyncFn: makeEx(present),
        windows: false,
      });
      if (r.freshnessToleranceAllowed !== true || r.action !== "tolerate-genuinely-absent") {
        fail(
          `(F2 j) buf-absent + pnpm-absent => allowed=${r.freshnessToleranceAllowed} action='${r.action}', ` +
            `expected true/tolerate-genuinely-absent (buf is the skip-determining tool; the Rust test skips)`,
        );
        ok = false;
      }
      if (calls !== 0) {
        fail(
          `(F2 j) buf-absent/pnpm-absent: runInstall called ${calls}x — must be 0 (pnpm absent ⇒ install not attempted)`,
        );
        ok = false;
      }
    }

    // (k) [P1] pnpm PRESENT, install EXIT 0, buf now present BUT oxfmt STILL absent => setup-fail,
    //     allowed:false, runInstall called 1. An exit-0 install that produced buf but not the required oxfmt
    //     is a deterministic setup failure — never tolerated (an exit-0 install is not a genuinely-tooling-
    //     less runner). pnpm starts present so the probe passes and the install runs; the fake install adds
    //     ONLY buf (not oxfmt). DISCRIMINATES: an exit-0 install that left oxfmt missing must FAIL LOUD, not
    //     tolerate (and not "installed" — the both-resolved branch must not fire with oxfmt missing).
    {
      const present = new Set([pathPnpm]); // pnpm on PATH; install "succeeds" but produces only buf
      let calls = 0;
      const runInstall = async () => {
        calls++;
        present.add(binBuf); // exit-0 install produced buf but NOT oxfmt
        return { code: 0, reason: "", spawnError: false };
      };
      const r = await preflightFreshnessTooling({
        repoRoot: REPO,
        env: installEnv,
        runInstall,
        existsSyncFn: makeEx(present),
        windows: false,
      });
      if (r.freshnessToleranceAllowed !== false || r.action !== "setup-fail") {
        fail(
          `(F2 k) install-exit0 + buf-present + oxfmt-still-absent => allowed=${r.freshnessToleranceAllowed} ` +
            `action='${r.action}', expected false/setup-fail (an exit-0 install that produced buf but not the ` +
            `required oxfmt is a setup failure, never tolerated and never "installed")`,
        );
        ok = false;
      }
      if (calls !== 1) {
        fail(
          `(F2 k) runInstall called ${calls}x, expected exactly 1 (pnpm resolvable ⇒ install runs)`,
        );
        ok = false;
      }
    }

    // (l) [P1] pnpm ABSENT + buf+oxfmt BOTH on PATH (PATH-only, NOT in node_modules) => path-fallback,
    //     allowed:false, runInstall called 0. The tools exist on PATH, so a freshness FAIL is a real
    //     regression (tolerance OFF); the install is NOT attempted because pnpm is unresolvable. The fake
    //     keys are PATH dirs ONLY (a node_modules path returns false), so the both-LOCAL-present
    //     short-circuit does NOT fire and the genuine pnpm-absent/path-fallback branch is exercised.
    {
      const PATHBIN = "/opt/fakebin";
      const pathBuf = appendPathComponentRaw(PATHBIN, "buf", false);
      const pathOxfmt = appendPathComponentRaw(PATHBIN, "oxfmt", false); // buf + oxfmt on PATH; pnpm absent everywhere
      const present = new Set([pathBuf, pathOxfmt]);
      const pathEnv = { PATH: PATHBIN };
      let calls = 0;
      const runInstall = async () => {
        calls++;
        return { code: 0, reason: "", spawnError: false };
      };
      const r = await preflightFreshnessTooling({
        repoRoot: REPO,
        env: pathEnv,
        runInstall,
        existsSyncFn: makeEx(present),
        windows: false,
      });
      if (r.freshnessToleranceAllowed !== false || r.action !== "path-fallback") {
        fail(
          `(F2 l) pnpm-absent + buf+oxfmt-on-PATH => allowed=${r.freshnessToleranceAllowed} ` +
            `action='${r.action}', expected false/path-fallback (the tools resolve on PATH ⇒ tolerance OFF; ` +
            `the install is not attempted because pnpm is absent)`,
        );
        ok = false;
      }
      if (calls !== 0) {
        fail(
          `(F2 l) pnpm-absent + tools-on-PATH: runInstall called ${calls}x — must be 0 (pnpm absent ⇒ install not attempted)`,
        );
        ok = false;
      }
    }

    // (m) [P3] PATH-ONLY PNPM RESOLUTION — pnpm matchable ONLY at `node_modules/.bin/pnpm` (NOT on PATH) +
    //     buf ABSENT => tolerate-genuinely-absent, allowed:true, runInstall called 0. `resolvePnpm` must
    //     resolve pnpm THE WAY `pnpmInstallCommand` LAUNCHES IT — the RESOLVED pnpm path (POSIX: the resolved
    //     binary directly; Windows: the resolved path quoted through `cmd.exe` with windowsVerbatimArguments),
    //     a PATH lookup — so a local-only shim the launcher never invokes does NOT count as resolvable. With pnpm
    //     not on PATH and buf unresolvable, the preflight takes the genuinely-absent branch (buf is the
    //     skip-determining tool) and tolerates WITHOUT attempting the install. THE DISCRIMINATING CASE:
    //       - POST-FIX (PATH-only `resolvePnpm`): pnpm is NOT resolvable (it's only at node_modules/.bin, off
    //         PATH) ⇒ the genuinely-absent branch fires ⇒ buf absent ⇒ { allowed:true,
    //         action:"tolerate-genuinely-absent" } AND runInstall 0.
    //       - PRE-FIX (`resolveLocalBinShim(pnpm) || resolvePathShim(pnpm)`): the LOCAL `node_modules/.bin/pnpm`
    //         shim RESOLVED ⇒ pnpm "positively resolved" ⇒ Step 4 ATTEMPTS the install ⇒ runInstall called 1
    //         and (the fake install produces nothing) action "setup-fail" / allowed:false. So both the action
    //         AND the runInstall count differ — the assertion FAILS pre-fix and PASSES post-fix.
    //     The PATH is empty (no pnpm on PATH); buf/oxfmt are nowhere; ONLY `binPnpm` (node_modules/.bin/pnpm)
    //     exists in the fake fs. This is the exact resolve-vs-launch inconsistency [P3] closes.
    {
      const present = new Set([binPnpm]); // pnpm ONLY at node_modules/.bin (NOT on PATH); no buf/oxfmt
      let calls = 0;
      const runInstall = async () => {
        calls++; // pre-fix the local shim resolves ⇒ this IS called; the fake install produces nothing
        return { code: 0, reason: "", spawnError: false };
      };
      const r = await preflightFreshnessTooling({
        repoRoot: REPO,
        env: noEnv, // empty PATH ⇒ no PATH-resolvable pnpm
        runInstall,
        existsSyncFn: makeEx(present),
        windows: false,
      });
      if (r.freshnessToleranceAllowed !== true || r.action !== "tolerate-genuinely-absent") {
        fail(
          `(F2 m) local-only-pnpm (node_modules/.bin, NOT on PATH) + buf-absent => allowed=${r.freshnessToleranceAllowed} ` +
            `action='${r.action}', expected true/tolerate-genuinely-absent (pnpm must be PATH-resolvable to count — ` +
            `the bare-\`pnpm\` launch is a PATH lookup; a local-only shim does NOT count. Pre-fix \`resolvePnpm\` ` +
            `resolved the local shim ⇒ attempted the install ⇒ setup-fail, the resolve-vs-launch inconsistency)`,
        );
        ok = false;
      }
      if (calls !== 0) {
        fail(
          `(F2 m) local-only-pnpm: runInstall called ${calls}x — must be 0 (pnpm is NOT on PATH, so the ` +
            `install is not attempted; pre-fix the local shim resolved and the install WAS attempted, count 1)`,
        );
        ok = false;
      }
    }

    if (ok) {
      pass(
        "(F2) FRESHNESS PREFLIGHT (buf-absence-only + positive-pnpm-probe model): both-present => already-present (runInstall " +
          "NOT called); pnpm-present+install-succeeds => installed (called once); POSIX pnpm-ABSENT => " +
          "tolerate-genuinely-absent (allowed, runInstall NOT called); pnpm-present+install-nonzero => " +
          "setup-fail (LOUD, not tolerated, called once); LAUNCHED-non-zero + tools-on-PATH => setup-fail " +
          "(the bypass-regression precedence); exit-0 + tools-still-missing => setup-fail; WINDOWS pnpm-ABSENT " +
          "=> tolerate-genuinely-absent + runInstall 0 (constrained-runner contract restored: spawnError is " +
          "no longer a tolerance signal); WINDOWS pnpm-present + install-nonzero => setup-fail + runInstall 1; " +
          "[FAIL-OPEN CLOSURE] buf-present + oxfmt-absent + pnpm-absent => setup-fail (NEVER tolerate — oxfmt " +
          "is required when buf runs); buf-absent + pnpm-absent => tolerate (buf is the skip-determining tool); " +
          "exit-0 + buf-present + oxfmt-still-absent => setup-fail; pnpm-absent + buf+oxfmt-on-PATH => " +
          "path-fallback; [P3 PATH-ONLY] local-only-pnpm (node_modules/.bin, NOT on PATH) + buf-absent => " +
          "tolerate-genuinely-absent + runInstall 0 (a local-only shim the launcher never invokes does NOT " +
          "count as resolvable — pre-fix it resolved and attempted the install) — discriminating across the " +
          "codex buf-absence-only + positive-probe outcomes",
      );
    }
  }

  // --------------------------------------------------------------------------------------------------
  // (F4) [P1] PNPM INSTALL LAUNCH COMMAND — the production install command resolution (`pnpmInstallCommand`),
  //      tested IN-PROCESS without actually spawning. F2 above INJECTS `runInstall` into the preflight, so
  //      it never exercises the REAL launch-command decision; this scenario covers exactly that decision.
  //      THE BUG (two layers): (1) on Windows `pnpm` resolves to `pnpm.cmd`, and Node's `spawn(…, { shell:false
  //      })` (what `runContainedStep` uses) CANNOT launch a `.cmd` shim — it spawn-errors. The preflight then
  //      reads that spawnError as "pnpm genuinely absent" and TOLERATES the freshness pair on Windows. The fix
  //      routes Windows through `cmd.exe` (a real `.exe` `spawn` launches directly and that `taskkill /T` reaps
  //      as the tree root), preserving containment. (2) Launching a BARE `pnpm` token through cmd.exe lets
  //      cmd.exe search the CURRENT DIRECTORY first, so a repo-local `pnpm.cmd` could run instead of the
  //      resolver-approved one — a CWD-tool-source hazard + preflight-vs-installer asymmetry. The codex-ruled
  //      fix (Option A) makes the RESOLVED `pnpmPath` the single source of truth: `pnpmInstallCommand` now
  //      takes `pnpmPath` and launches THAT exact binary (Windows: quoted under `cmd.exe /d /s /c` with
  //      `windowsVerbatimArguments`; POSIX: directly), never a bare token.
  //      THE THIRD LAYER (F-C): the Windows command processor must be a VERIFIED ABSOLUTE executable, never a
  //      bare `cmd.exe` token (which cmd.exe's own CWD-first search could resolve to a repo-local imposter).
  //      `pnpmInstallCommand` reads `ComSpec` CASE-INSENSITIVELY and uses it ONLY if absolute AND an existing
  //      file; else forms `<SystemRoot>\System32\cmd.exe` (SystemRoot also case-insensitive) and uses it if
  //      absolute AND existing; else SETUP-FAILS (`{ setupFail: true, detail }`) — never a bare `cmd.exe`. The
  //      is-file predicate is INJECTED so these run on a POSIX host with a fake fs (no real filesystem access).
  //      Each assertion DISCRIMINATES: pre-fix `pnpmInstallCommand()` took no path and returned a BARE "pnpm"
  //      token (and on missing ComSpec returned a bare "cmd.exe"), so a check that `cmd === resolvedPath`
  //      (POSIX) / that args carry the resolved path and NO bare "pnpm" (Windows) / that a missing absolute
  //      processor SETUP-FAILS FAILS pre-fix.
  //        (a) Windows + absolute existing ComSpec => cmd === that ComSpec, args === ["/d","/s","/c",
  //            '""<pnpmPath>" install --frozen-lockfile"'], windowsVerbatimArguments === true (cmd.exe is the
  //            spawned reapable tree root, NOT the .cmd shim; the resolved path — not a bare token — is what runs).
  //        (b) Windows + NO ComSpec AND NO SystemRoot => SETUP-FAIL (`setupFail === true`), NOT a bare "cmd.exe".
  //        (c) POSIX => cmd === the resolved pnpmPath, args === ["install","--frozen-lockfile"] (direct launch
  //            of the resolved binary; no PATH re-search, no command processor).
  //        (d) [P1 CWD hazard — end-to-end resolve→launch] A PATH pnpm and a repo-cwd-local pnpm both exist;
  //            `resolvePnpm` (PATH-only) returns the PATH one; the launch then carries THAT resolved path and
  //            NEVER a bare "pnpm" token NOR the cwd-local path — proving the RESOLVED path (not a CWD search)
  //            decides the binary. Modeled on POSIX (the POSIX-mode `resolvePnpm` splits on the LITERAL `:`
  //            selected by `windows:false` (`pathDelimiterFor(false)`), host-independently)
  //            so it resolves on every host; the property is platform-shared (Windows is covered by (a)'s
  //            resolved-path-quoted-under-cmd.exe + no-bare-token assertions).
  //        (e) [Windows resolved-path verbatim] A directly-supplied resolved Windows `.cmd` path containing a
  //            SPACE is quoted verbatim under cmd.exe (carried in args[3], windowsVerbatimArguments true), with
  //            no bare "pnpm" token — re-asserting the no-bare-token + spaced-path-survives property.
  //        (f) [F-C command-processor resolver — the decider Q3 cases, is-file faked]:
  //            · COMSPEC=C:\Windows\System32\cmd.exe present + faked-existing => honored (cmd === it).
  //            · wrong-case `comspec` / `ComSpec` key => still honored (case-insensitive read).
  //            · relative `ComSpec=cmd.exe` => NOT used; falls through to SystemRoot or setup-fail.
  //            · missing/invalid ComSpec + valid faked-existing SystemRoot=C:\Windows => cmd ===
  //              "C:\Windows\System32\cmd.exe".
  //            · no valid absolute candidate (ComSpec absent/relative AND SystemRoot absent/non-existing) =>
  //              SETUP-FAIL (`setupFail === true`), NEVER a bare "cmd.exe".
  // --------------------------------------------------------------------------------------------------
  process.stderr.write("\n(F4) pnpm install launch command (platform-aware, no spawn)\n");
  {
    let ok = true;
    const WIN_HEAD = ["/d", "/s", "/c"];

    // (a) Windows with an explicit ABSOLUTE EXISTING ComSpec: launch the RESOLVED path quoted under that
    //     cmd.exe. The is-file predicate is FAKED (4th arg) so the absolute ComSpec counts as existing on a
    //     POSIX self-test host (no real filesystem access).
    {
      const comspec = "C:\\Windows\\system32\\cmd.exe";
      const resolvedPnpm = "C:\\safe tools\\pnpm.CMD"; // a resolved path WITH a space — must survive verbatim
      const fakeIsFile = (p) => p === comspec;
      const w = pnpmInstallCommand(resolvedPnpm, true, { ComSpec: comspec }, fakeIsFile);
      if (w.cmd !== comspec) {
        fail(`(F4 a) Windows cmd => '${w.cmd}', expected the ComSpec '${comspec}'`);
        ok = false;
      }
      const headOk =
        Array.isArray(w.args) &&
        w.args.length === 4 &&
        WIN_HEAD.every((tok, i) => w.args[i] === tok);
      if (!headOk) {
        fail(
          `(F4 a) Windows args => ${JSON.stringify(w.args)}, expected ["/d","/s","/c", "<one verbatim string>"] ` +
            `(cmd.exe stays the reapable tree root; the command is ONE pre-quoted verbatim arg)`,
        );
        ok = false;
      }
      const cmdLine = headOk ? w.args[3] : "";
      if (!cmdLine.includes(resolvedPnpm) || !cmdLine.includes("install --frozen-lockfile")) {
        fail(
          `(F4 a) Windows verbatim arg => ${JSON.stringify(cmdLine)}, expected to contain the resolved path ` +
            `'${resolvedPnpm}' AND 'install --frozen-lockfile'`,
        );
        ok = false;
      }
      if (w.args.some((a) => a === "pnpm")) {
        fail(
          `(F4 a) Windows args contain a BARE "pnpm" token => ${JSON.stringify(w.args)} — the bare token must ` +
            `be gone (cmd.exe would otherwise search CWD first; only the resolved path may launch)`,
        );
        ok = false;
      }
      if (w.windowsVerbatimArguments !== true) {
        fail(
          `(F4 a) windowsVerbatimArguments => ${JSON.stringify(w.windowsVerbatimArguments)}, expected true ` +
            `(so Node does not re-quote the pre-quoted command line)`,
        );
        ok = false;
      }
    }

    // (b) Windows with NO ComSpec AND NO SystemRoot in env => SETUP-FAIL, never a bare "cmd.exe". The empty
    //     env has no absolute command processor to verify, so `pnpmInstallCommand` refuses to launch a bare
    //     token. DISCRIMINATES: pre-fix this returned `{ cmd: "cmd.exe", … }` (the bare fallback), so the
    //     `setupFail === true` AND `cmd !== "cmd.exe"` assertions FAIL pre-fix.
    {
      const w = pnpmInstallCommand("C:\\safe\\pnpm.CMD", true, {});
      if (w.setupFail !== true) {
        fail(
          `(F4 b) Windows no-ComSpec/no-SystemRoot => ${JSON.stringify(w)}, expected a setup-fail ` +
            `({ setupFail: true, detail }) — pre-fix returned the bare "cmd.exe" fallback`,
        );
        ok = false;
      }
      if (w.cmd === "cmd.exe") {
        fail(
          `(F4 b) Windows no-ComSpec/no-SystemRoot returned the bare "cmd.exe" token — must SETUP-FAIL instead`,
        );
        ok = false;
      }
    }

    // (c) POSIX => a direct launch of the RESOLVED binary (no command processor, no PATH re-search — the
    //     resolved shim is directly executable on POSIX, so `spawn(…, { shell:false })` launches it).
    {
      const resolvedPnpm = "/safe/bin/pnpm";
      const p = pnpmInstallCommand(resolvedPnpm, false);
      if (p.cmd !== resolvedPnpm) {
        fail(
          `(F4 c) POSIX cmd => '${p.cmd}', expected the resolved path '${resolvedPnpm}' (never bare "pnpm")`,
        );
        ok = false;
      }
      if (p.cmd === "pnpm") {
        fail(`(F4 c) POSIX cmd is the BARE "pnpm" token — must be the resolved path`);
        ok = false;
      }
      const argsOk =
        Array.isArray(p.args) &&
        p.args.length === 2 &&
        p.args[0] === "install" &&
        p.args[1] === "--frozen-lockfile";
      if (!argsOk) {
        fail(
          `(F4 c) POSIX args => ${JSON.stringify(p.args)}, expected ["install","--frozen-lockfile"] (frozen lockfile preserved)`,
        );
        ok = false;
      }
    }

    // (d) [P1 CWD hazard — end-to-end] The resolver-approved PATH pnpm is the launch target — NOT a CWD-local
    //     one. Model on POSIX (the POSIX-mode `resolvePnpm` splits on the LITERAL `:` selected by `windows:false`
    //     (`pathDelimiterFor(false)`), host-independently): a fake PATH carries
    //     `/safe/bin`, whose `pnpm` exists; a fake repo cwd `/repo` ALSO has a `pnpm`. `resolvePnpm` (PATH-only)
    //     returns `/safe/bin/pnpm`; the launch (`pnpmInstallCommand(resolved, false)`) must run THAT path
    //     (cmd === it), never the bare "pnpm" token, never `/repo/pnpm`. DISCRIMINATES: pre-fix
    //     `pnpmInstallCommand()` ignored any resolved path and the POSIX branch emitted `cmd:"pnpm"` — so the
    //     launch would NOT be the resolved path and a CWD-first search could pick `/repo/pnpm`.
    {
      const safeDir = "/safe/bin";
      const safePnpm = `${safeDir}/pnpm`;
      const cwdLocalPnpm = "/repo/pnpm";
      // Fake fs: BOTH the PATH dir's pnpm and the cwd-local pnpm exist as files — but `resolvePnpm` is
      // PATH-only, so it must select the PATH one and never the cwd-local one.
      const fakeIsFile = (p) => p === safePnpm || p === cwdLocalPnpm;
      const fakeEnv = { PATH: safeDir };
      const resolved = resolvePnpm(fakeEnv, fakeIsFile, false);
      if (resolved !== safePnpm) {
        fail(
          `(F4 d) resolvePnpm => ${JSON.stringify(resolved)}, expected the PATH pnpm '${safePnpm}' ` +
            `(PATH-only resolution; the cwd-local '${cwdLocalPnpm}' must never be selected)`,
        );
        ok = false;
      }
      const p = pnpmInstallCommand(resolved, false);
      if (p.cmd !== safePnpm) {
        fail(
          `(F4 d) launch cmd => ${JSON.stringify(p.cmd)}, expected the resolved PATH path '${safePnpm}'`,
        );
        ok = false;
      }
      if (p.cmd === "pnpm" || p.cmd === cwdLocalPnpm) {
        fail(
          `(F4 d) launch cmd is a bare token or the CWD-local pnpm => ${JSON.stringify(p.cmd)} — only the ` +
            `resolver-approved PATH path may launch`,
        );
        ok = false;
      }
      if ([p.cmd, ...p.args].some((a) => a === "pnpm" || a === cwdLocalPnpm)) {
        fail(
          `(F4 d) launch carries a bare "pnpm" token or the cwd-local pnpm => ${JSON.stringify([p.cmd, ...p.args])}`,
        );
        ok = false;
      }
    }

    // (e) [Windows resolved-path verbatim] A directly-supplied resolved Windows `.cmd` path (with a space)
    //     is quoted verbatim under cmd.exe — never a bare token, never split. This case directly supplies an
    //     already-resolved Windows `.cmd` path (with a space) to test the cmd.exe verbatim-quoting /
    //     no-bare-token launch behavior; (a) above already proves the resolved-path launch shape, and this
    //     re-asserts the no-bare-token property on a path containing a space.
    {
      const resolvedWin = "C:\\Program Files\\pnpm\\pnpm.CMD";
      const comspecE = "C:\\Windows\\System32\\cmd.exe";
      const w = pnpmInstallCommand(resolvedWin, true, { ComSpec: comspecE }, (p) => p === comspecE);
      const cmdLine = Array.isArray(w.args) && w.args.length === 4 ? w.args[3] : "";
      if (!cmdLine.includes(resolvedWin)) {
        fail(
          `(F4 e) Windows verbatim arg => ${JSON.stringify(cmdLine)}, expected to carry the spaced resolved ` +
            `path '${resolvedWin}'`,
        );
        ok = false;
      }
      if (w.args.some((a) => a === "pnpm")) {
        fail(
          `(F4 e) Windows args carry a bare "pnpm" token => ${JSON.stringify(w.args)} — must be gone`,
        );
        ok = false;
      }
      if (w.windowsVerbatimArguments !== true) {
        fail(
          `(F4 e) windowsVerbatimArguments => ${JSON.stringify(w.windowsVerbatimArguments)}, expected true`,
        );
        ok = false;
      }
    }

    // (f) [F-C — VERIFIED ABSOLUTE COMMAND-PROCESSOR RESOLVER] The decider Q3 cases, with the is-file
    //     predicate INJECTED/FAKED (4th arg) so Windows-mode runs on a POSIX host with no real fs. The
    //     resolved pnpm path is a fixed Windows `.cmd`; the command processor is what varies. Tracked under
    //     its OWN `okFc` flag with its OWN pass — an independently visible F-C self-test.
    {
      let okFc = true;
      const pnpmCmd = "C:\\safe\\pnpm.CMD";
      const SYS_CMD = "C:\\Windows\\System32\\cmd.exe"; // the canonical <SystemRoot>\System32\cmd.exe
      const COMSPEC_ABS = "C:\\Windows\\System32\\cmd.exe";

      // (f1) COMSPEC absolute + faked-existing => honored.
      {
        const w = pnpmInstallCommand(
          pnpmCmd,
          true,
          { COMSPEC: COMSPEC_ABS },
          (p) => p === COMSPEC_ABS,
        );
        if (w.setupFail || w.cmd !== COMSPEC_ABS) {
          fail(
            `(F4 f1) absolute existing COMSPEC => ${JSON.stringify(w)}, expected cmd === ` +
              `${JSON.stringify(COMSPEC_ABS)} (honored)`,
          );
          okFc = false;
        }
      }

      // (f2) wrong-CASE `comspec` / `ComSpec` keys => still honored (case-insensitive read). Pre-fix read
      //      `env.ComSpec` case-EXACTLY, so a lowercase `comspec` key was INVISIBLE and the code fell to the
      //      bare "cmd.exe" fallback — this honored-via-lowercase-key assertion FAILS pre-fix.
      for (const key of ["comspec", "ComSpec", "CoMsPeC"]) {
        const w = pnpmInstallCommand(
          pnpmCmd,
          true,
          { [key]: COMSPEC_ABS },
          (p) => p === COMSPEC_ABS,
        );
        if (w.setupFail || w.cmd !== COMSPEC_ABS) {
          fail(
            `(F4 f2) wrong-case ComSpec key ${JSON.stringify(key)} => ${JSON.stringify(w)}, expected ` +
              `cmd === ${JSON.stringify(COMSPEC_ABS)} (case-insensitive read)`,
          );
          okFc = false;
        }
      }

      // (f3) RELATIVE `ComSpec=cmd.exe` => NOT used; with no SystemRoot it SETUP-FAILS (never the relative
      //      token). Even though the fake fs would report "cmd.exe" as existing, a relative path is rejected.
      {
        const w = pnpmInstallCommand(pnpmCmd, true, { ComSpec: "cmd.exe" }, () => true);
        if (w.setupFail !== true) {
          fail(
            `(F4 f3) relative ComSpec "cmd.exe" => ${JSON.stringify(w)}, expected SETUP-FAIL (a relative ` +
              `ComSpec must NOT be used) — pre-fix it returned cmd === "cmd.exe"`,
          );
          okFc = false;
        }
        if (w.cmd === "cmd.exe") {
          fail(
            `(F4 f3) relative ComSpec "cmd.exe" was USED as cmd — a relative processor must be refused`,
          );
          okFc = false;
        }
      }

      // (f4) missing/invalid ComSpec + valid faked-existing SystemRoot=C:\Windows => cmd ===
      //      C:\Windows\System32\cmd.exe (the canonical derived path). Faked is-file accepts ONLY the derived
      //      System32 cmd.exe (so the fallback to SystemRoot is what produced it). Tested with ComSpec absent,
      //      and with ComSpec present-but-relative (falls through to SystemRoot).
      for (const env of [
        { SystemRoot: "C:\\Windows" },
        { ComSpec: "cmd.exe", SystemRoot: "C:\\Windows" }, // relative ComSpec ignored => SystemRoot used
        { SYSTEMROOT: "C:\\Windows" }, // wrong-case SystemRoot key => still read
      ]) {
        const w = pnpmInstallCommand(pnpmCmd, true, env, (p) => p === SYS_CMD);
        if (w.setupFail || w.cmd !== SYS_CMD) {
          fail(
            `(F4 f4) env ${JSON.stringify(env)} => ${JSON.stringify(w)}, expected cmd === ` +
              `${JSON.stringify(SYS_CMD)} (derived <SystemRoot>\\System32\\cmd.exe)`,
          );
          okFc = false;
        }
      }

      // (f5) NO valid absolute candidate (ComSpec absent/relative AND SystemRoot absent/non-existing) =>
      //      SETUP-FAIL, NEVER a bare "cmd.exe". Several shapes: empty env; relative ComSpec + no SystemRoot;
      //      SystemRoot present but the derived cmd.exe does NOT exist (faked is-file returns false).
      for (const [env, isFileFn, label] of [
        [{}, () => false, "empty env"],
        [{ ComSpec: "cmd.exe" }, () => true, "relative ComSpec, no SystemRoot"],
        [{ SystemRoot: "C:\\Windows" }, () => false, "SystemRoot set but derived cmd.exe absent"],
        [
          { ComSpec: "..\\cmd.exe", SystemRoot: "Windows" },
          () => true,
          "relative ComSpec + relative SystemRoot",
        ],
      ]) {
        const w = pnpmInstallCommand(pnpmCmd, true, env, isFileFn);
        if (w.setupFail !== true) {
          fail(
            `(F4 f5) no absolute processor (${label}) => ${JSON.stringify(w)}, expected SETUP-FAIL ` +
              `({ setupFail: true }) — pre-fix returned a bare "cmd.exe"`,
          );
          okFc = false;
        }
        if (w.cmd === "cmd.exe") {
          fail(
            `(F4 f5) no absolute processor (${label}) returned bare "cmd.exe" — must SETUP-FAIL`,
          );
          okFc = false;
        }
        if (w.setupFail && typeof w.detail !== "string") {
          fail(
            `(F4 f5) setup-fail (${label}) carries no string detail => ${JSON.stringify(w.detail)}`,
          );
          okFc = false;
        }
      }
      if (okFc) {
        pass(
          "(F4 f) [F-C] pnpmInstallCommand resolves a VERIFIED ABSOLUTE Windows command processor: an " +
            "absolute existing ComSpec (read CASE-INSENSITIVELY) is honored; a RELATIVE ComSpec is refused; " +
            "with no valid ComSpec it derives <SystemRoot>\\System32\\cmd.exe (SystemRoot also case-insensitive) " +
            "and uses it when absolute+existing; with NO valid absolute candidate it SETUP-FAILS " +
            "({ setupFail: true, detail }) — NEVER a bare/relative cmd.exe (discriminating: pre-fix read " +
            'ComSpec case-exactly and returned a bare "cmd.exe" fallback)',
        );
      }
    }

    if (ok) {
      pass(
        "(F4) PNPM INSTALL LAUNCH COMMAND: Windows routes the RESOLVED pnpm path (quoted, windowsVerbatimArguments) " +
          "through a VERIFIED ABSOLUTE command processor (a case-insensitive absolute existing ComSpec, else " +
          "the derived <SystemRoot>\\System32\\cmd.exe, else SETUP-FAIL — never a bare/relative cmd.exe) so the " +
          ".cmd shim runs under a spawnable, taskkill-reapable tree root with NO bare token (no CWD search); " +
          "POSIX launches the resolved binary directly — both preserve --frozen-lockfile; the resolver-approved " +
          "PATH pnpm wins over a cwd-local one (discriminating: pre-fix pnpmInstallCommand emitted a bare " +
          '"pnpm" token, read ComSpec case-exactly, and returned a bare "cmd.exe" fallback)',
      );
    }
  }

  // --------------------------------------------------------------------------------------------------
  // (F5) [P1 FAIL-OPEN CLOSURE] CARGO-ENV PATH SANITIZATION — `buildCargoEnv` sanitizes the PATH var to its
  //      CWD-INDEPENDENT ABSOLUTE components ONLY (empty, dot-only, non-dot relative, `..`-relative, Windows
  //      drive-relative `C:foo`, and Windows root-relative `\x` / `/x` are ALL dropped) so the EXECUTED
  //      cargo/nextest/libtest tests and the verdict preflight resolver resolve every tool from the SAME
  //      absolute-only PATH — the CLOSED cwd-independent invariant, with NO cwd-relative disagreement. THE
  //      BUG (pre-fix): the preflight got the RAW `process.env`, whose
  //      PATH could carry an empty component (`:`) that its `resolvePathShim` SKIPS (so it decides "buf
  //      absent ⇒ tolerate"), while the Rust freshness test resolves `buf` via `split_paths(PATH) +
  //      dir.join("buf")` where `join("", "buf") === "buf"` HONORS that empty component as CWD — so a CWD
  //      `buf` makes the test RUN and possibly FAIL on stale bindings while the gate TOLERATES it (a
  //      silently-tolerated regression); the SAME divergence exists for ANY cwd-relative entry (a relative
  //      `dir.join("buf")` is itself cwd-relative). The fix sanitizes the PATH var in `buildCargoEnv` to its
  //      absolute components and feeds THAT env to the preflight, so neither side resolves a cwd-relative tool.
  //        (a) POSIX: PATH "SAFE::.:./:OTHER:" => "" (key DELETED) — every component is relative or
  //            implicit-CWD ("SAFE"/"OTHER" are non-dot relative, absolute-only drops them), so nothing
  //            survives; the key is deleted (var_os("PATH")? => None). A MIXED absolute+relative PATH keeps
  //            only the absolute dir.
  //        (b) POSIX: a `Path` (capital-P) var is LEFT UNTOUCHED — on POSIX `var_os("PATH")` is case-EXACT,
  //            so `Path` is a DIFFERENT, non-PATH var Rust never reads; sanitizing it would alter an
  //            unrelated env var. (The Windows case-insensitive PATH-key handling is exercised in (h).)
  //        (c) CONSISTENCY MODEL — raw PATH ":" makes the Rust locator's `join("", "buf") === "buf"`
  //            CWD-resolvable; the sanitized PATH is "" and contains NO empty component, so that same
  //            CWD-resolution model CANNOT fire. We assert both the raw-model property and the sanitized "".
  //        (d) UNTOUCHED ENV — a missing PATH stays missing (not created); a normal explicit ABSOLUTE dir
  //            survives verbatim; non-PATH env vars are not rewritten.
  //        (e) WINDOWS-SHAPE — `sanitizePathValue(value, true)` uses `;`: an all-relative "A;;.;.\\;B" => "".
  //        (j) ABSOLUTE-ONLY (F-D): the decider-mandated cases — POSIX
  //            `sanitizePathValue("bin:/abs/bin:../tools:.", false) === "/abs/bin"` (relative/`..`/dot dropped,
  //            absolute kept); Windows-mode drops `tools\x`, `..\x`, `C:foo`, and root-relative `\x` / `/x`
  //            while keeping fully-qualified absolutes (`C:\Windows\System32`, a UNC `\\srv\share`).
  //      DISCRIMINATION: pre-fix `buildCargoEnv` returns PATH UNCHANGED, so `=== ""`/key-deleted would be the
  //      raw value and FAIL; pre-fix `sanitizePathValue` KEPT non-dot relative / `..` / drive-relative
  //      entries, so the (a)/(e)/(j) absolute-only assertions FAIL against the pre-fix predicate. These pass
  //      ONLY post-fix.
  // --------------------------------------------------------------------------------------------------
  process.stderr.write("\n(F5) cargo-env PATH sanitization (cwd-independent absolute-only)\n");
  {
    let ok = true;
    const TGT = "/tmp/selftest-runner-target";
    const POSIX = process.platform !== "win32";

    // (a)+(b) The POSIX-delimiter (`:`) string assertions only hold byte-for-byte on a POSIX host, where
    // the platform `delimiter` buildCargoEnv uses is `:`. On a Windows host these exact strings would split
    // on `;`, so skip them there and rely on the explicit `sanitizePathValue(_, true)` Windows-shape check.
    if (POSIX) {
      // (a) PATH: every component is relative or implicit-CWD ("SAFE"/"OTHER" are NON-DOT RELATIVE — under
      // absolute-only they are DROPPED, not kept) — so nothing survives and the key is DELETED. Pre-fix the
      // predicate KEPT "SAFE"/"OTHER" (it only dropped empty/dot-only), so it returned "SAFE:OTHER" and the
      // key stayed PRESENT — this `key-deleted` assertion FAILS pre-fix.
      const ea = buildCargoEnv({ PATH: "SAFE::.:./:OTHER:" }, TGT);
      if ("PATH" in ea) {
        fail(
          `(F5 a) all-relative PATH "SAFE::.:./:OTHER:" => PATH key still PRESENT (value ${JSON.stringify(ea.PATH)}); ` +
            `expected the key DELETED — every component is relative/implicit-CWD, absolute-only keeps none ` +
            `(pre-fix it kept "SAFE"/"OTHER" and returned "SAFE:OTHER")`,
        );
        ok = false;
      }
      // (a') MIXED absolute + relative: only the ABSOLUTE dir survives; the relative "rel" and the bare "lib"
      // are dropped. Discriminates: pre-fix kept "rel"/"lib", so the result was "rel:/abs/dir:lib".
      const eaMixed = buildCargoEnv({ PATH: "rel:/abs/dir:lib" }, TGT);
      if (eaMixed.PATH !== "/abs/dir") {
        fail(
          `(F5 a') mixed PATH "rel:/abs/dir:lib" sanitized => ${JSON.stringify(eaMixed.PATH)}, expected ` +
            `"/abs/dir" (only the absolute dir kept; the relative "rel"/"lib" dropped) — pre-fix kept them all`,
        );
        ok = false;
      }

      // (b) [P1 — POSIX is case-EXACT] A POSIX `Path` (capital-P) var is LEFT UNTOUCHED. Rust's
      // `var_os("PATH")` is case-SENSITIVE on POSIX, so `Path` is a DIFFERENT, non-PATH var Rust never reads
      // — `buildCargoEnv` must not rewrite it. (Pre-fix `buildCargoEnv` sanitized a POSIX `Path` to "A:B";
      // post-fix it leaves it verbatim — so this `=== "A::.:B"` assertion FAILS pre-fix and PASSES post-fix,
      // and the discriminating "PaTh deleted on Windows" case is (h).) The `windows` default is `false` on a
      // POSIX host, so `findPathEnvKey` returns null for a Path-only env and the key is never touched.
      const eb = buildCargoEnv({ Path: "A::.:B" }, TGT);
      if (eb.Path !== "A::.:B") {
        fail(
          `(F5 b) POSIX Path "A::.:B" must be LEFT UNTOUCHED => ${JSON.stringify(eb.Path)}, expected verbatim ` +
            `"A::.:B" (POSIX var_os("PATH") is case-exact; Path is not the PATH var)`,
        );
        ok = false;
      }

      // (c) [FIX-1 — delete-on-empty] An ALL-implicit-CWD PATH must DELETE the env key, not assign "".
      // The Rust freshness test reads `std::env::var_os("PATH")?`: a PRESENT empty value (`Some("")`) is
      // NOT None, so it reaches `std::env::split_paths("")`, which yields ONE empty PathBuf, and
      // `"".join("buf") == "buf"` (relative ⇒ CWD) resolves a CWD `buf` that RUNS — the fail-open. ONLY a
      // DELETED key makes `var_os("PATH")?` early-return None (the sole genuinely no-CWD-source form;
      // split_paths is never reached). So the property that maps to that early-return is KEY ABSENCE, not a
      // "" value, and not the JS-String.split `"".split(":") === [""]` model (which is FALSE under Rust
      // `split_paths`). Assert the key is absent for every all-implicit shape.
      for (const allImplicit of [":", ":.", "."]) {
        const e = buildCargoEnv({ PATH: allImplicit }, TGT);
        if ("PATH" in e) {
          fail(
            `(F5 c) all-implicit PATH ${JSON.stringify(allImplicit)} => PATH key still PRESENT ` +
              `(value ${JSON.stringify(e.PATH)}); expected the key DELETED so Rust's var_os("PATH")? ` +
              `early-returns None (a present "" is split_paths("") == [""], a live CWD source)`,
          );
          ok = false;
        }
      }
      // A MIXED PATH (an implicit-CWD component AND an explicit dir) keeps the explicit dir — the key
      // survives, the implicit-CWD component is stripped, and NO embedded empty remains.
      const ecMixed = buildCargoEnv({ PATH: ".:/usr/bin" }, TGT);
      if (!("PATH" in ecMixed) || ecMixed.PATH !== "/usr/bin") {
        fail(
          `(F5 c) mixed PATH ".:/usr/bin" => ${JSON.stringify(ecMixed.PATH)} (present=${"PATH" in ecMixed}), ` +
            `expected "/usr/bin" (the explicit dir kept, the leading "." stripped, key retained)`,
        );
        ok = false;
      }
      if (ecMixed.PATH.split(":").some((d) => d === "")) {
        fail(
          `(F5 c) mixed sanitized PATH ${JSON.stringify(ecMixed.PATH)} still carries an EMPTY (CWD-resolvable) ` +
            `component — the Rust \`join("", "buf") === "buf"\` model could still fire`,
        );
        ok = false;
      }
    } else {
      note(
        "(F5 a/b/c) POSIX-delimiter string asserts skipped on Windows host; see (F5 e) for the ; shape",
      );
    }

    // (d) UNTOUCHED ENV — a missing PATH is never synthesized; a normal explicit dir survives verbatim;
    //     unrelated env vars are not rewritten. (Delimiter-agnostic — holds on every host.)
    {
      const edMissing = buildCargoEnv({ FOO: "bar" }, TGT);
      if ("PATH" in edMissing || "Path" in edMissing) {
        fail(
          `(F5 d) buildCargoEnv created PATH/Path from nothing: ${JSON.stringify(Object.keys(edMissing))}`,
        );
        ok = false;
      }
      if (edMissing.FOO !== "bar") {
        fail(
          `(F5 d) unrelated env var FOO was rewritten => ${JSON.stringify(edMissing.FOO)}, expected "bar"`,
        );
        ok = false;
      }
      const singleDir = process.platform === "win32" ? "C:\\Windows\\system32" : "/usr/local/bin";
      const edKept = buildCargoEnv({ PATH: singleDir }, TGT);
      if (edKept.PATH !== singleDir) {
        fail(
          `(F5 d) a single explicit dir was altered => ${JSON.stringify(edKept.PATH)}, expected ${JSON.stringify(singleDir)}`,
        );
        ok = false;
      }
    }

    // (e) WINDOWS-SHAPE — drive the exported sanitizer directly with `windows = true` so the `;` delimiter
    //     path is covered on EVERY host (it never runs as part of buildCargoEnv on a POSIX host). "A"/"B" are
    //     NON-DOT RELATIVE — under absolute-only they are DROPPED — so "A;;.;.\\;B" collapses to "". Pre-fix
    //     the predicate kept "A"/"B" and returned "A;B"; this `=== ""` assertion FAILS pre-fix.
    {
      const ew = sanitizePathValue("A;;.;.\\;B", true);
      if (ew !== "") {
        fail(
          `(F5 e) Windows-shape sanitize "A;;.;.\\;B" => ${JSON.stringify(ew)}, expected "" (A/B relative, dropped)`,
        );
        ok = false;
      }
      // A MIXED Windows PATH with one absolute drive dir keeps ONLY that dir; the relative "A"/"B" drop.
      const ewMixed = sanitizePathValue("A;C:\\abs;B", true);
      if (ewMixed !== "C:\\abs") {
        fail(
          `(F5 e) Windows-shape sanitize "A;C:\\abs;B" => ${JSON.stringify(ewMixed)}, expected "C:\\abs" ` +
            `(only the absolute drive dir kept; relative A/B dropped) — pre-fix kept A/B`,
        );
        ok = false;
      }
      // A fully-implicit-CWD Windows PATH collapses to "".
      const ewEmpty = sanitizePathValue(";.;", true);
      if (ewEmpty !== "") {
        fail(`(F5 e) Windows-shape sanitize ";.;" => ${JSON.stringify(ewEmpty)}, expected ""`);
        ok = false;
      }
    }

    // (h) [P1 — WINDOWS NON-CANONICAL PATH CASING IN buildCargoEnv] On Windows env names fold
    //     case-INSENSITIVELY, so the PATH var the Rust test reads via `var_os("PATH")` may be stored under
    //     `PaTh`. Pre-fix `buildCargoEnv` sanitized only the EXACT `env.PATH`/`env.Path` keys, so a `PaTh`
    //     key was left UNSANITIZED — its implicit-CWD components survived into the executed-test env, the
    //     fail-open. Post-fix `buildCargoEnv(env, target, true)` finds the case-insensitive PATH key via
    //     `findPathEnvKey` and sanitizes/deletes THAT key. We pass `windows:true` so this exercises the
    //     Windows key handling on a POSIX host; the `;` delimiter then applies, matching `sanitizePathValue
    //     (_, true)`. DISCRIMINATES: pre-fix the `PaTh` key is never inspected ⇒ an all-implicit `PaTh:";.;"`
    //     stays PRESENT (key retained, value unchanged); post-fix the key is DELETED (all-implicit ⇒ "").
    {
      // An all-implicit-CWD Windows `PaTh` ⇒ the key is DELETED (Rust var_os("PATH")? early-returns None).
      // Pre-fix the `PaTh` key was untouched ⇒ still present; this is the discriminating assertion.
      const eAllImplicit = buildCargoEnv({ PaTh: ";.;" }, TGT, true);
      const stillHasPathish = Object.keys(eAllImplicit).some((k) => k.toUpperCase() === "PATH");
      if (stillHasPathish) {
        fail(
          `(F5 h) Windows all-implicit PaTh ";.;" => a PATH-ish key REMAINS ` +
            `(${JSON.stringify(Object.keys(eAllImplicit).filter((k) => k.toUpperCase() === "PATH"))}); ` +
            `expected the PaTh key DELETED (all-implicit ⇒ "" ⇒ var_os("PATH")? None) — pre-fix the PaTh ` +
            `key was never inspected so it stayed present unsanitized (the fail-open)`,
        );
        ok = false;
      }
      // A MIXED Windows `PaTh` (implicit `.` + explicit drive dir) ⇒ the value is sanitized and COLLAPSED
      // onto the single canonical `PATH` key: the explicit dir survives, the implicit `.` is stripped, the
      // result lives on `PATH`, and the original-cased `PaTh` key is GONE (the Windows case-variant collapse).
      const eMixed = buildCargoEnv({ PaTh: ".;C:\\tools" }, TGT, true);
      if (eMixed.PATH !== "C:\\tools" || "PaTh" in eMixed) {
        fail(
          `(F5 h) Windows mixed PaTh ".;C:\\tools" => PATH=${JSON.stringify(eMixed.PATH)} ` +
            `(PaTh present=${"PaTh" in eMixed}), expected the sanitized "C:\\tools" on the canonical PATH key ` +
            `with the PaTh key DELETED (the implicit "." stripped, the explicit drive dir kept)`,
        );
        ok = false;
      }
      // SANITY: a normal-cased Windows `Path` also collapses onto canonical `PATH` — the sanitized value
      // lives on `PATH` and the original `Path` key is removed (one canonical PATH-ish key survives).
      const eNormalPath = buildCargoEnv({ Path: ".;C:\\tools" }, TGT, true);
      if (eNormalPath.PATH !== "C:\\tools" || "Path" in eNormalPath) {
        fail(
          `(F5 h) Windows normal-cased Path ".;C:\\tools" => PATH=${JSON.stringify(eNormalPath.PATH)} ` +
            `(Path present=${"Path" in eNormalPath}), expected the sanitized "C:\\tools" on canonical PATH ` +
            `with the Path key DELETED`,
        );
        ok = false;
      }
      // POSIX PARITY: with `windows:false`, the SAME `PaTh` env is NOT the PATH var, so buildCargoEnv leaves
      // it UNTOUCHED (verbatim) — proving the case-insensitive sanitization is Windows-ONLY. The exact POSIX
      // `PATH` path is already covered by (a)–(g) above.
      const ePosixPaTh = buildCargoEnv({ PaTh: ".:/usr/bin" }, TGT, false);
      if (ePosixPaTh.PaTh !== ".:/usr/bin") {
        fail(
          `(F5 h) POSIX PaTh-cased var must be left UNTOUCHED => ${JSON.stringify(ePosixPaTh.PaTh)}, ` +
            `expected verbatim ".:/usr/bin" (POSIX PaTh is NOT the PATH var; only exact PATH is sanitized)`,
        );
        ok = false;
      }
    }

    // (i) [FIX-B — WINDOWS DUPLICATE CASE-VARIANT COLLAPSE] On Windows two PATH-ish keys can COEXIST
    //     (`PATH` ALONGSIDE `Path`/`PaTh`) holding DIFFERENT values; the spawned cargo/nextest child folds
    //     env names case-insensitively and could observe the UNSELECTED variant's value as its effective
    //     PATH. Pre-fix `buildCargoEnv` sanitized ONLY the single `findPathEnvKey` key and LEFT the other
    //     variant untouched — so a `Path` carrying an unsanitized/unselected value survived as an executable
    //     child PATH source the preflight never saw (a fail-open). Post-fix `buildCargoEnv(env, target, true)`
    //     collapses ALL case-variants: it sanitizes the policy-SELECTED value (PATH>Path>any), DELETES every
    //     PATH-ish variant, and writes back exactly ONE canonical `PATH`. We build `{ PATH: <selected,
    //     sanitizes to a real dir>, Path: <unselected "evil" value> }` and assert (a) exactly one PATH-ish
    //     key remains and it is `PATH` (no `Path` survives) and (b) the surviving value is the SANITIZED
    //     policy-selected value — never the unselected variant. DISCRIMINATES: pre-fix the unselected `Path`
    //     SURVIVES (`"Path" in result`), so the "no other PATH-ish key" assertion FAILS pre-fix; post-fix the
    //     `Path` key is deleted ⇒ passes. Tracked under its OWN `okI` flag with its OWN pass (an independently
    //     visible self-test). The POSIX control proves the collapse is Windows-ONLY.
    {
      let okI = true;
      // Policy selects the exact `PATH` (PATH>Path); its value sanitizes (the leading `.` is stripped) to the
      // real explicit dir `C:\real`. The UNSELECTED `Path` carries a DIFFERENT value (`C:\evil`) that, if it
      // survived, the case-folding child would see as an executable PATH source.
      const selectedSanitized = "C:\\real";
      const unselectedEvil = "C:\\evil";
      const result = buildCargoEnv(
        { PATH: ".;C:\\real", Path: unselectedEvil, CARGO_HOME: "keep-me" },
        TGT,
        true,
      );
      const pathish = Object.keys(result).filter((k) => k.toUpperCase() === "PATH");
      // (a) exactly ONE PATH-ish key, and it is the canonical `PATH`; the unselected `Path` is GONE.
      if (pathish.length !== 1 || pathish[0] !== "PATH" || "Path" in result) {
        fail(
          `(F5 i) Windows duplicate PATH/Path => PATH-ish keys=${JSON.stringify(pathish)} ` +
            `(Path present=${"Path" in result}), expected exactly ["PATH"] with the unselected Path key ` +
            `DELETED — pre-fix the unselected Path SURVIVED as a case-folded child PATH source (the fail-open)`,
        );
        okI = false;
      }
      // (b) the surviving canonical value is the SANITIZED policy-selected value — NOT the unselected variant.
      if (result.PATH !== selectedSanitized) {
        fail(
          `(F5 i) surviving canonical PATH => ${JSON.stringify(result.PATH)}, expected the sanitized ` +
            `policy-selected value ${JSON.stringify(selectedSanitized)} (the leading "." stripped); it must ` +
            `NEVER be the unselected ${JSON.stringify(unselectedEvil)}`,
        );
        okI = false;
      }
      // Explicit guard: the unselected "evil" value must not appear on ANY PATH-ish key.
      if (pathish.some((k) => result[k] === unselectedEvil)) {
        fail(
          `(F5 i) the unselected variant value ${JSON.stringify(unselectedEvil)} survived on a PATH-ish key ` +
            `(${JSON.stringify(pathish)}) — a case-folded child could resolve a tool from it`,
        );
        okI = false;
      }
      // Unrelated env var is preserved (the collapse touches ONLY PATH-ish keys).
      if (result.CARGO_HOME !== "keep-me") {
        fail(
          `(F5 i) an unrelated env var was altered by the collapse => CARGO_HOME=${JSON.stringify(result.CARGO_HOME)}, expected "keep-me"`,
        );
        okI = false;
      }
      // POSIX CONTROL (not discriminating for F-B): with `windows:false` the collapse must NOT run — `var_os
      // ("PATH")` is case-EXACT on POSIX, so a legitimate `Path` var is a DIFFERENT variable that must be
      // LEFT UNTOUCHED. Assert the `Path` value survives verbatim and the exact `PATH` is sanitized normally.
      const posix = buildCargoEnv({ PATH: ".:/usr/bin", Path: "/some/posix/path" }, TGT, false);
      if (posix.Path !== "/some/posix/path") {
        fail(
          `(F5 i) POSIX control: a POSIX Path var must be LEFT UNTOUCHED => ${JSON.stringify(posix.Path)}, ` +
            `expected verbatim "/some/posix/path" (POSIX is case-exact; the Windows-only collapse must not fire)`,
        );
        okI = false;
      }
      if (posix.PATH !== "/usr/bin") {
        fail(
          `(F5 i) POSIX control: the exact PATH must still be sanitized => ${JSON.stringify(posix.PATH)}, expected "/usr/bin"`,
        );
        okI = false;
      }
      if (okI) {
        pass(
          "(F5 i) [FIX-B] buildCargoEnv collapses duplicate Windows PATH case-variants onto ONE canonical " +
            "PATH: { PATH, Path } => only PATH survives with the SANITIZED policy-selected value, the " +
            "unselected variant is deleted (pre-fix it survived as a case-folded child PATH source); POSIX " +
            "control leaves a Path var untouched (collapse is Windows-only)",
        );
      }
    }

    // (f) [absolute-root keep vs root-relative drop] The POSIX bare root `/` is a cwd-INDEPENDENT absolute
    //     directory, so it MUST survive (the pre-fix predicate dropped it — a `/`-only component had no
    //     non-empty path SEGMENTS, but having no segments is NOT the same as normalizing to `.`). On Windows
    //     a DRIVE root `C:\` is cwd-independent absolute and KEPT, but a BARE backslash root `\` is
    //     ROOT-RELATIVE (it resolves against the CURRENT drive — drive-current-directory dependent), so the
    //     decider DROPS it. DISCRIMINATES: pre-fix `sanitizePathValue("/", false) === ""` (so `=== "/"`
    //     FAILS pre-fix); and pre-fix the predicate KEPT a bare `\` (its `isRooted` matched `startsWith("\")`),
    //     so the `\ dropped` assertion FAILS pre-fix. `.`/`./`/empty are STILL dropped.
    {
      // POSIX bare root survives verbatim.
      const rootPosix = sanitizePathValue("/", false);
      if (rootPosix !== "/") {
        fail(
          `(F5 f) POSIX bare root "/" sanitized => ${JSON.stringify(rootPosix)}, expected "/" (explicit dir)`,
        );
        ok = false;
      }
      // POSIX root alongside another explicit dir: BOTH kept (the `/` is not stripped as CWD-ish).
      const rootMixPosix = sanitizePathValue("/:/usr/bin", false);
      if (rootMixPosix.split(":")[0] !== "/" || !rootMixPosix.split(":").includes("/usr/bin")) {
        fail(
          `(F5 f) POSIX "/:/usr/bin" sanitized => ${JSON.stringify(rootMixPosix)}, expected the bare root "/" ` +
            `AND "/usr/bin" both kept`,
        );
        ok = false;
      }
      // Windows bare backslash root is ROOT-RELATIVE (current-drive dependent) ⇒ DROPPED (the decider's
      // absolute-only rule drops `\x` / `/x`). Pre-fix it was KEPT; this `=== ""` assertion FAILS pre-fix.
      const rootWinBackslash = sanitizePathValue("\\", true);
      if (rootWinBackslash !== "") {
        fail(
          `(F5 f) Windows bare root "\\" sanitized => ${JSON.stringify(rootWinBackslash)}, expected "" ` +
            `(root-relative \\x is current-drive dependent — DROPPED, not cwd-independent absolute)`,
        );
        ok = false;
      }
      // Windows root-relative `\x` / `/x` (single leading separator, NOT a drive root, NOT UNC) are DROPPED;
      // only the drive-rooted absolute `C:\abs` survives. Pre-fix `\win` / `/x` were KEPT (rooted).
      const rootRelWin = sanitizePathValue("\\win;C:\\abs;/x", true);
      const rootRelSegs = rootRelWin.split(";");
      if (rootRelWin !== "C:\\abs" || rootRelSegs.includes("\\win") || rootRelSegs.includes("/x")) {
        fail(
          `(F5 f) Windows "\\win;C:\\abs;/x" sanitized => ${JSON.stringify(rootRelWin)}, expected only "C:\\abs" ` +
            `(root-relative "\\win" and "/x" dropped as current-drive dependent)`,
        );
        ok = false;
      }
      // Windows drive root survives alongside another dir; the implicit `.` between them is stripped.
      const rootWinDrive = sanitizePathValue("C:\\;.;D:\\bin", true);
      const winSegs = rootWinDrive.split(";");
      if (!winSegs.includes("C:\\") || !winSegs.includes("D:\\bin") || winSegs.includes(".")) {
        fail(
          `(F5 f) Windows "C:\\;.;D:\\bin" sanitized => ${JSON.stringify(rootWinDrive)}, expected "C:\\" AND ` +
            `"D:\\bin" kept with the "." stripped`,
        );
        ok = false;
      }
      // A UNC path `\\srv\share` is cwd-independent absolute ⇒ KEPT; a drive-RELATIVE `C:foo` (no separator
      // after the colon — current-directory-of-drive dependent) is DROPPED. Pre-fix `C:foo` was KEPT (the
      // predicate had no drive-relative check). Discriminates on both keep (UNC) and drop (drive-relative).
      const uncDriveRel = sanitizePathValue("\\\\srv\\share;C:foo;C:\\real", true);
      const udSegs = uncDriveRel.split(";");
      if (
        !udSegs.includes("\\\\srv\\share") ||
        !udSegs.includes("C:\\real") ||
        udSegs.includes("C:foo")
      ) {
        fail(
          `(F5 f) Windows "\\\\srv\\share;C:foo;C:\\real" sanitized => ${JSON.stringify(uncDriveRel)}, expected ` +
            `the UNC "\\\\srv\\share" AND "C:\\real" kept with the drive-relative "C:foo" DROPPED`,
        );
        ok = false;
      }
    }

    // (g) [POSIX root-dot keep vs Windows root-relative-dot drop] On POSIX a ROOTED component whose only
    //     segment is `.` (`/.`, `/./`) starts with `/` ⇒ it is a cwd-INDEPENDENT absolute path at the
    //     filesystem root and MUST survive — only an UNROOTED all-`.` component (`.`, `./`, `./.`) is the
    //     implicit-CWD form that is dropped. On Windows a backslash-rooted dot `\.` is ROOT-RELATIVE (single
    //     leading separator, current-drive dependent), so the decider DROPS it (it is NOT a drive-rooted
    //     `C:\.`). DISCRIMINATES: pre-fix the predicate dropped POSIX `/.` (`segs.every(s=>s===".")` was
    //     true) — so `=== "/."` FAILS pre-fix; and pre-fix the predicate KEPT Windows `\.` (its `isRooted`
    //     matched `startsWith("\")`) — so the `\. dropped` assertion FAILS pre-fix.
    {
      // POSIX rooted dot forms survive verbatim (absolute `/`-rooted => explicit root, not CWD).
      const rootDotPosix = sanitizePathValue("/.", false);
      if (rootDotPosix !== "/.") {
        fail(
          `(F5 g) POSIX rooted "/." sanitized => ${JSON.stringify(rootDotPosix)}, expected "/." (explicit root)`,
        );
        ok = false;
      }
      const rootDotSlashPosix = sanitizePathValue("/./", false);
      if (rootDotSlashPosix !== "/./") {
        fail(
          `(F5 g) POSIX rooted "/./" sanitized => ${JSON.stringify(rootDotSlashPosix)}, expected "/./" (explicit root)`,
        );
        ok = false;
      }
      // Windows backslash-rooted dot `\.` is ROOT-RELATIVE (current-drive dependent) ⇒ DROPPED. Pre-fix it
      // was KEPT; this `=== ""` assertion FAILS pre-fix.
      const rootDotWin = sanitizePathValue("\\.", true);
      if (rootDotWin !== "") {
        fail(
          `(F5 g) Windows rooted "\\." sanitized => ${JSON.stringify(rootDotWin)}, expected "" ` +
            `(root-relative \\. is current-drive dependent — DROPPED, not absolute)`,
        );
        ok = false;
      }
      // A drive-rooted dot `C:\.` IS cwd-independent absolute (drive + colon + separator) ⇒ KEPT.
      const driveRootDotWin = sanitizePathValue("C:\\.", true);
      if (driveRootDotWin !== "C:\\.") {
        fail(
          `(F5 g) Windows drive-rooted "C:\\." sanitized => ${JSON.stringify(driveRootDotWin)}, expected ` +
            `"C:\\." (drive-rooted absolute, kept)`,
        );
        ok = false;
      }
      // The UNROOTED implicit-CWD forms are STILL dropped.
      const cwdDot = sanitizePathValue(".", false);
      const cwdDotSlash = sanitizePathValue("./", false);
      if (cwdDot !== "" || cwdDotSlash !== "") {
        fail(
          `(F5 g) UNROOTED CWD forms must still drop: "." => ${JSON.stringify(cwdDot)} and "./" => ` +
            `${JSON.stringify(cwdDotSlash)}, both expected ""`,
        );
        ok = false;
      }
      // A rooted dot alongside an unrooted CWD `.`: the rooted "/." is kept, the bare "." is dropped.
      const mixRootDot = sanitizePathValue("/.:.:/usr/bin", false);
      const mixSegs = mixRootDot.split(":");
      if (!mixSegs.includes("/.") || !mixSegs.includes("/usr/bin") || mixSegs.includes(".")) {
        fail(
          `(F5 g) POSIX "/.:.:/usr/bin" sanitized => ${JSON.stringify(mixRootDot)}, expected "/." AND ` +
            `"/usr/bin" kept with the bare "." dropped`,
        );
        ok = false;
      }
    }

    // (j) [F-D ABSOLUTE-ONLY — decider Q3 mandated cases] The sanitizer keeps ONLY cwd-INDEPENDENT ABSOLUTE
    //     components and DROPS empty / dot-only / non-dot relative / `..`-relative / Windows drive-relative /
    //     Windows root-relative entries. These are the exact decider-named discriminating inputs. Tracked
    //     under its OWN `okJ` flag with its OWN pass — an independently visible F-D self-test.
    {
      let okJ = true;
      // POSIX: "bin:/abs/bin:../tools:." => "/abs/bin" — the non-dot relative "bin", the `..`-relative
      // "../tools", and the bare "." are dropped; only the absolute "/abs/bin" survives. Pre-fix KEPT "bin"
      // and "../tools" (it only dropped empty/dot-only), so the result was "bin:/abs/bin:../tools" — this
      // exact-equality FAILS pre-fix.
      const jPosix = sanitizePathValue("bin:/abs/bin:../tools:.", false);
      if (jPosix !== "/abs/bin") {
        fail(
          `(F5 j) POSIX sanitizePathValue("bin:/abs/bin:../tools:.", false) => ${JSON.stringify(jPosix)}, ` +
            `expected "/abs/bin" (relative "bin", "../tools", and "." dropped; only the absolute kept)`,
        );
        okJ = false;
      }
      // NEGATIVE: the dropped relative/`..`/dot entries must be ABSENT from the result.
      const jPosixSegs = jPosix.split(":");
      if (
        jPosixSegs.includes("bin") ||
        jPosixSegs.includes("../tools") ||
        jPosixSegs.includes(".")
      ) {
        fail(
          `(F5 j) POSIX result ${JSON.stringify(jPosix)} still carries a dropped relative/".."/"." entry`,
        );
        okJ = false;
      }
      // Windows-mode: a PATH with relative "tools\x", `..`-relative "..\x", drive-relative "C:foo", and
      // fully-qualified absolutes (a drive root "C:\Windows\System32" and a UNC "\\srv\share") => keeps ONLY
      // the two absolutes, drops the rest (and would drop root-relative `\x` / `/x`). Pre-fix KEPT
      // "tools\x"/"..\x"/"C:foo" (no relative/drive-relative checks), so this FAILS pre-fix.
      const jWin = sanitizePathValue(
        "tools\\x;..\\x;C:foo;C:\\Windows\\System32;\\\\srv\\share;\\x;/x",
        true,
      );
      const jWinSegs = jWin.split(";");
      const jWinKeeps =
        jWinSegs.length === 2 &&
        jWinSegs.includes("C:\\Windows\\System32") &&
        jWinSegs.includes("\\\\srv\\share");
      if (!jWinKeeps) {
        fail(
          `(F5 j) Windows-mode sanitize => ${JSON.stringify(jWin)}, expected exactly ` +
            `["C:\\Windows\\System32","\\\\srv\\share"] (the two fully-qualified absolutes), with ` +
            `"tools\\x", "..\\x", "C:foo", "\\x", and "/x" all DROPPED`,
        );
        okJ = false;
      }
      // NEGATIVE: every cwd-dependent Windows entry must be ABSENT.
      for (const dropped of ["tools\\x", "..\\x", "C:foo", "\\x", "/x"]) {
        if (jWinSegs.includes(dropped)) {
          fail(
            `(F5 j) Windows result ${JSON.stringify(jWin)} still carries the cwd-dependent entry ` +
              `${JSON.stringify(dropped)} — it must be dropped (absolute-only)`,
          );
          okJ = false;
        }
      }
      if (okJ) {
        pass(
          "(F5 j) [F-D] sanitizePathValue keeps ONLY cwd-independent absolute components: POSIX " +
            '"bin:/abs/bin:../tools:." => "/abs/bin" (relative/".."/dot dropped); Windows-mode keeps the ' +
            "drive-rooted absolute and the UNC while DROPPING relative tools\\x, ..\\x, drive-relative C:foo, " +
            "and root-relative \\x / /x (discriminating: pre-fix KEPT non-dot relative / `..` / drive-relative " +
            "entries)",
        );
      }
    }

    // (k) [F-D LOAD-BEARING AGREEMENT — preflight resolver vs child env] The whole point of the absolute-only
    //     sanitization: a relative `bin` PATH entry whose `bin/buf` WOULD resolve a tool pre-fix must, after
    //     `buildCargoEnv`, be invisible to BOTH (1) the preflight tool resolver (`resolvePathShim`, run on the
    //     SANITIZED env) AND (2) the executed-test child env (the PATH `buildCargoEnv` writes). If the two
    //     disagreed — preflight sees no `buf` ⇒ tolerance ON, while the Rust child resolves a relative `buf`
    //     and RUNS — a real stale-binding regression would be silently tolerated. We model POSIX-mode
    //     EXPLICITLY: both `resolvePathShim` and `buildCargoEnv` are passed `windows:false`, so the PATH
    //     splits on the LITERAL `:` from `pathDelimiterFor(false)`, host-INDEPENDENTLY — this case is correct
    //     on macOS, Windows, AND Linux, NOT reliant on the live host being POSIX. DISCRIMINATES: pre-fix
    //     `sanitizePathValue` KEPT the relative `bin`, so the sanitized PATH still contained `bin`, the
    //     preflight resolver resolved `bin/buf`, AND the child env carried `bin` — so BOTH the
    //     "resolver no longer sees buf" and "child PATH drops the relative entry" assertions FAIL pre-fix.
    //     Tracked under its OWN `okK` flag with its OWN pass — the independently visible load-bearing test.
    {
      let okK = true;
      // EXPLICIT POSIX-mode delimiter (host-independent): the same literal `:` source
      // `resolvePathShim(..., false)` and `buildCargoEnv(..., false)` split on below.
      const posixDelim = pathDelimiterFor(false);
      const relBinDir = "relbin"; // a NON-DOT RELATIVE PATH entry (cwd-dependent)
      const absDir = "/abs/tools"; // a legitimate absolute dir that must survive
      const relBuf = `${relBinDir}/buf`; // the relative shim `bin/buf` the Rust child's dir.join would honor
      const absBuf = `${absDir}/buf`;
      // Fake fs: a `buf` exists in BOTH the relative dir and the absolute dir. The relative one is the hazard.
      const fakeIsFile = (p) => p === relBuf || p === absBuf;

      // PRE-FIX MODEL (sanity): on the RAW unsanitized PATH the resolver WOULD resolve the relative buf. This
      // proves the relative shim is genuinely resolvable, so its post-sanitization disappearance is meaningful
      // (not a vacuous pass). We assert it resolves to SOMETHING (the relative buf is first in PATH order).
      const rawResolved = resolvePathShim(
        "buf",
        { PATH: `${relBinDir}${posixDelim}${absDir}` },
        fakeIsFile,
        false,
      );
      if (rawResolved !== relBuf) {
        fail(
          `(F5 k) pre-fix model: resolvePathShim on the RAW PATH "${relBinDir}:${absDir}" => ` +
            `${JSON.stringify(rawResolved)}, expected the relative ${JSON.stringify(relBuf)} (proving the ` +
            `relative shim is genuinely resolvable — the hazard the fix must close)`,
        );
        okK = false;
      }

      // POST-FIX: buildCargoEnv sanitizes that PATH to its ABSOLUTE-only components.
      const childEnv = buildCargoEnv({ PATH: `${relBinDir}${posixDelim}${absDir}` }, TGT, false);
      // (1) the child env PATH must NO LONGER contain the relative dir (only the absolute survives).
      const childSegs = (childEnv.PATH || "").split(posixDelim);
      if (childSegs.includes(relBinDir)) {
        fail(
          `(F5 k) child env PATH ${JSON.stringify(childEnv.PATH)} STILL carries the relative ${JSON.stringify(relBinDir)} ` +
            `— the Rust child's dir.join("${relBinDir}", "buf") would resolve a cwd-relative buf (the fail-open)`,
        );
        okK = false;
      }
      if (childEnv.PATH !== absDir) {
        fail(
          `(F5 k) child env PATH => ${JSON.stringify(childEnv.PATH)}, expected only the absolute ${JSON.stringify(absDir)}`,
        );
        okK = false;
      }
      // (2) the preflight resolver, run on the SAME sanitized env, must NOT resolve the relative buf — it can
      //     only find the absolute one (proving preflight and child AGREE: both see the absolute, neither the
      //     relative). This is the load-bearing agreement.
      const preflightResolved = resolvePathShim("buf", childEnv, fakeIsFile, false);
      if (preflightResolved === relBuf) {
        fail(
          `(F5 k) the preflight resolver STILL resolved the relative ${JSON.stringify(relBuf)} from the ` +
            `sanitized child env — preflight and child disagree (the silently-tolerated-regression fail-open)`,
        );
        okK = false;
      }
      if (preflightResolved !== absBuf) {
        fail(
          `(F5 k) preflight resolver on the sanitized env => ${JSON.stringify(preflightResolved)}, expected ` +
            `the ABSOLUTE ${JSON.stringify(absBuf)} (preflight and child agree on the absolute-only PATH)`,
        );
        okK = false;
      }
      if (okK) {
        pass(
          "(F5 k) [F-D LOAD-BEARING] a relative `bin` PATH entry whose `bin/buf` resolves PRE-FIX is, after " +
            "buildCargoEnv, invisible to BOTH the preflight resolver (resolvePathShim on the sanitized env " +
            "finds only the absolute buf, never the relative) AND the child env (PATH carries only the " +
            "absolute dir) — preflight and test AGREE on absolute-only tool resolution, closing the " +
            "silently-tolerated stale-binding fail-open (discriminating: pre-fix the relative bin survived " +
            "sanitization and both sides resolved the relative buf)",
        );
      }
    }

    // (l) [RAW NON-NORMALIZING APPEND — preflight candidate must match the Rust child's LEXICAL dir.join]
    //     The F-D sanitizer KEEPS an absolute PATH component VERBATIM, including an absolute that contains
    //     `..`/`.` or is a symlink (e.g. `/tmp/link/../bin`) — those ARE cwd-independent, so the absolute-only
    //     filter correctly keeps them. The Rust freshness test then resolves with `std::env::split_paths(PATH)
    //     + dir.join(tool)`, and Rust's `Path::join`/`PathBuf::push` are PURELY LEXICAL — they do NOT collapse
    //     `..`/`.` and do NOT resolve symlinks. So for `/tmp/link/../bin` the Rust child probes
    //     `/tmp/link/../bin/buf`. The JS preflight MUST probe the IDENTICAL byte path. Pre-fix `resolvePathShim`
    //     built the candidate with `node:path.join`, which NORMALIZES (`/tmp/link/../bin` => `/tmp/bin`,
    //     collapsing `..`), so the JS preflight probed `/tmp/bin/buf` while the Rust child probed
    //     `/tmp/link/../bin/buf` — a DIFFERENT file under a `/tmp/link` symlink, so the preflight could
    //     conclude "buf absent" (tolerance ON) while the Rust child resolves+runs `buf` (the exact
    //     silently-tolerated fail-open). DISCRIMINATES: with a fake fs that matches ONLY the RAW (un-normalized)
    //     candidate, post-fix `resolvePathShim` returns the raw path; pre-fix `join` normalized away the `..`
    //     and the fake rejected it ⇒ null. Tracked under its OWN `okL` flag with its OWN pass.
    {
      let okL = true;

      // POSIX: a kept absolute component carrying `..` (cwd-INDEPENDENT, so the F-D filter keeps it verbatim).
      const posixComponent = "/tmp/link/../bin";
      const rawPosixCandidate = `${posixComponent}/buf`; // what the Rust child's lexical dir.join probes
      const normalizedPosixCandidate = "/tmp/bin/buf"; // what node:path.join collapses it to (the pre-fix bug)
      // The fake fs matches ONLY the RAW candidate — never the normalized one. This is the load-bearing
      // discriminator: if the resolver normalizes (pre-fix), the candidate it builds is rejected ⇒ null.
      const fakeIsFilePosix = (p) => p === rawPosixCandidate;
      // CONTROL (proves the discrimination keys on raw-vs-normalized, not on the file merely existing): the
      // normalized candidate is NOT in the fake fs, so a normalizing resolver genuinely misses.
      if (fakeIsFilePosix(normalizedPosixCandidate)) {
        fail(
          `(F5 l) control invalid: the fake fs must NOT match the normalized candidate ` +
            `${JSON.stringify(normalizedPosixCandidate)} (else the test would pass even with normalization)`,
        );
        okL = false;
      }
      const resolvedPosix = resolvePathShim(
        "buf",
        { PATH: posixComponent },
        fakeIsFilePosix,
        false,
      );
      if (resolvedPosix !== rawPosixCandidate) {
        fail(
          `(F5 l) POSIX raw append: resolvePathShim("buf", {PATH:${JSON.stringify(posixComponent)}}, _, false) ` +
            `=> ${JSON.stringify(resolvedPosix)}, expected the RAW non-normalized ${JSON.stringify(rawPosixCandidate)} ` +
            `(mirroring Rust's lexical dir.join). Pre-fix node:path.join collapsed it to ` +
            `${JSON.stringify(normalizedPosixCandidate)} which the fake fs rejects ⇒ null (the fail-open).`,
        );
        okL = false;
      }
      // Direct unit assertion on the helper: the raw appender must NOT collapse `..` and must use the
      // platform separator selected by `windows` (here `/`). A normalizing implementation would FAIL this.
      const rawAppendPosix = appendPathComponentRaw(posixComponent, "buf", false);
      if (rawAppendPosix !== rawPosixCandidate) {
        fail(
          `(F5 l) appendPathComponentRaw(${JSON.stringify(posixComponent)}, "buf", false) => ` +
            `${JSON.stringify(rawAppendPosix)}, expected the RAW ${JSON.stringify(rawPosixCandidate)} (no ".." collapse)`,
        );
        okL = false;
      }

      // WINDOWS-MODE variant (exercised on a POSIX host via `windows=true`): a drive-rooted absolute carrying
      // `..` (`C:\link\..\bin`). The raw appender joins with `\`; `resolveExecutableShim` then appends `.CMD`.
      // The fake fs matches ONLY the raw `\`-joined `.CMD` candidate. Pre-fix on a POSIX host node:path.join is
      // posix.join — it neither interprets `\` nor collapses the `\`-separated `..`, and it joins with `/`, so
      // it produced `C:\link\..\bin/buf` (a DIFFERENT string), and resolveExecutableShim's `.CMD` form of THAT
      // is not in the fake fs ⇒ null. Either way the post-fix raw `\`-candidate is what discriminates.
      const winComponent = "C:\\link\\..\\bin";
      const rawWinCandidate = `${winComponent}\\buf.CMD`; // the raw `\`-joined candidate + the .CMD suffix
      const fakeIsFileWin = (p) => p === rawWinCandidate;
      const resolvedWin = resolvePathShim("buf", { PATH: winComponent }, fakeIsFileWin, true);
      if (resolvedWin !== rawWinCandidate) {
        fail(
          `(F5 l) Windows raw append: resolvePathShim("buf", {PATH:${JSON.stringify(winComponent)}}, _, true) ` +
            `=> ${JSON.stringify(resolvedWin)}, expected the RAW ${JSON.stringify(rawWinCandidate)} ` +
            `(\\-joined, no ".." collapse). Pre-fix join normalized/"/"-joined it ⇒ the fake rejects ⇒ null.`,
        );
        okL = false;
      }
      const rawAppendWin = appendPathComponentRaw(winComponent, "buf", true);
      if (rawAppendWin !== `${winComponent}\\buf`) {
        fail(
          `(F5 l) appendPathComponentRaw(${JSON.stringify(winComponent)}, "buf", true) => ` +
            `${JSON.stringify(rawAppendWin)}, expected ${JSON.stringify(`${winComponent}\\buf`)} ` +
            `(\\-separator, no ".." collapse)`,
        );
        okL = false;
      }

      // TRAILING-SEPARATOR case: a component already ending with `/` must NOT get a doubled separator.
      const trailComponent = "/tmp/abs/bin/";
      const trailCandidate = "/tmp/abs/bin/buf"; // exactly one `/`, not `/tmp/abs/bin//buf`
      const fakeIsFileTrail = (p) => p === trailCandidate;
      const resolvedTrail = resolvePathShim(
        "buf",
        { PATH: trailComponent },
        fakeIsFileTrail,
        false,
      );
      if (resolvedTrail !== trailCandidate) {
        fail(
          `(F5 l) trailing-separator: resolvePathShim("buf", {PATH:${JSON.stringify(trailComponent)}}, _, false) ` +
            `=> ${JSON.stringify(resolvedTrail)}, expected ${JSON.stringify(trailCandidate)} (no doubled separator)`,
        );
        okL = false;
      }
      const rawAppendTrail = appendPathComponentRaw(trailComponent, "buf", false);
      if (rawAppendTrail !== trailCandidate) {
        fail(
          `(F5 l) appendPathComponentRaw(${JSON.stringify(trailComponent)}, "buf", false) => ` +
            `${JSON.stringify(rawAppendTrail)}, expected ${JSON.stringify(trailCandidate)} (component already ` +
            `ends with "/" ⇒ append directly, no doubled "/")`,
        );
        okL = false;
      }
      // Windows trailing `\` variant: a component ending with `\` must not get a doubled `\`.
      const rawAppendTrailWin = appendPathComponentRaw("C:\\abs\\bin\\", "buf", true);
      if (rawAppendTrailWin !== "C:\\abs\\bin\\buf") {
        fail(
          `(F5 l) appendPathComponentRaw("C:\\abs\\bin\\", "buf", true) => ${JSON.stringify(rawAppendTrailWin)}, ` +
            `expected "C:\\abs\\bin\\buf" (trailing "\\" ⇒ no doubled separator)`,
        );
        okL = false;
      }
      // [FIX-B DISCRIMINATOR] POSIX trailing `\` variant: on POSIX a trailing backslash is an ORDINARY filename
      // byte (NOT a separator), so the `/` IS inserted — mirroring Rust's POSIX `Path::join` (`/tmp/abs\` +
      // `buf` => `/tmp/abs\/buf`). A sanitized POSIX PATH entry ending in `\` survives sanitization (the POSIX
      // absolute filter checks only `dir.startsWith("/")`), so the Rust child probes `/tmp/abs\/buf` and the
      // preflight MUST probe the same byte path or the fail-open reopens. DISCRIMINATES: the pre-fix
      // platform-blind `endsWithSep` saw the trailing `\` as a separator and returned "/tmp/abs\buf" (a
      // DIFFERENT file), so this assertion FAILS pre-fix and PASSES post-fix.
      const rawAppendTrailPosixBackslash = appendPathComponentRaw("/tmp/abs\\", "buf", false);
      if (rawAppendTrailPosixBackslash !== "/tmp/abs\\/buf") {
        fail(
          `(F5 l) appendPathComponentRaw("/tmp/abs\\", "buf", false) => ${JSON.stringify(rawAppendTrailPosixBackslash)}, ` +
            `expected "/tmp/abs\\/buf" (POSIX: a trailing "\\" is an ordinary filename byte, NOT a separator, ` +
            `so the "/" IS inserted — mirroring Rust POSIX Path::join; pre-fix returned "/tmp/abs\\buf")`,
        );
        okL = false;
      }
      // End-to-end through resolvePathShim: a POSIX PATH entry ending in `\` must resolve the `...\/buf`
      // candidate (the fake-fs matches ONLY that). Pre-fix resolvePathShim built `/tmp/abs\buf`, the fake-fs
      // rejects it ⇒ null ⇒ FAIL pre-fix; post-fix it builds `/tmp/abs\/buf` ⇒ resolves.
      const posixBackslashCandidate = "/tmp/abs\\/buf";
      const fakeIsFilePosixBackslash = (p) => p === posixBackslashCandidate;
      const resolvedPosixBackslash = resolvePathShim(
        "buf",
        { PATH: "/tmp/abs\\" },
        fakeIsFilePosixBackslash,
        false,
      );
      if (resolvedPosixBackslash !== posixBackslashCandidate) {
        fail(
          `(F5 l) resolvePathShim("buf", {PATH:"/tmp/abs\\"}, _, false) => ${JSON.stringify(resolvedPosixBackslash)}, ` +
            `expected ${JSON.stringify(posixBackslashCandidate)} (POSIX trailing "\\" is NOT a separator, so the ` +
            `candidate is "/tmp/abs\\/buf"; pre-fix probed "/tmp/abs\\buf" ⇒ the fake-fs rejects ⇒ null)`,
        );
        okL = false;
      }
      // EMPTY-`dir` case: an empty PATH component must yield the bare `toolName` (no leading separator),
      // mirroring Rust's `Path::new("").join(tool)` / `PathBuf::from("").push(tool)` which both yield exactly
      // `tool` (the relative file). These are DIRECT helper-contract assertions, not end-to-end: the
      // production resolver (`resolvePathShim`) drops empty PATH entries via `if (!dir) continue` BEFORE the
      // helper, so no `resolvePathShim` empty-dir case is meaningful — the helper assertion is the correct
      // discriminator. DISCRIMINATES: pre-fix returned "/buf" (POSIX) / "\\buf" (Windows) — a leading
      // separator Rust never produces — so both FAIL pre-fix and PASS post-fix.
      const rawAppendEmptyPosix = appendPathComponentRaw("", "buf", false);
      if (rawAppendEmptyPosix !== "buf") {
        fail(
          `(F5 l) appendPathComponentRaw("", "buf", false) => ${JSON.stringify(rawAppendEmptyPosix)}, ` +
            `expected "buf" (empty dir ⇒ bare toolName, no leading "/", mirroring Rust "".join(tool) == tool; ` +
            `pre-fix returned "/buf")`,
        );
        okL = false;
      }
      const rawAppendEmptyWin = appendPathComponentRaw("", "buf", true);
      if (rawAppendEmptyWin !== "buf") {
        fail(
          `(F5 l) appendPathComponentRaw("", "buf", true) => ${JSON.stringify(rawAppendEmptyWin)}, ` +
            `expected "buf" (empty dir ⇒ bare toolName, no leading "\\", mirroring Rust "".join(tool) == tool; ` +
            `pre-fix returned "\\buf")`,
        );
        okL = false;
      }

      if (okL) {
        pass(
          "(F5 l) [RAW APPEND] resolvePathShim builds the PATH candidate by NON-NORMALIZING concatenation " +
            "(appendPathComponentRaw), mirroring the Rust child's lexical dir.join: an absolute component " +
            'carrying ".." (`/tmp/link/../bin`, `C:\\link\\..\\bin`) resolves the RAW `../`-bearing candidate ' +
            "the Rust child probes (NOT the node:path.join-normalized form), the platform separator follows " +
            "the `windows` flag, and the trailing-separator suppression is platform-ACCURATE — a trailing `/` " +
            "suppresses the inserted separator on BOTH platforms, a trailing `\\` suppresses it on Windows ONLY, " +
            "while a POSIX trailing `\\` is an ordinary filename byte so the `/` IS inserted (mirroring Rust " +
            "POSIX `Path::join`) — so a symlinked/`..`-bearing OR trailing-`\\`-bearing absolute resolves " +
            "IDENTICALLY on the preflight and the Rust child (discriminating: pre-fix node:path.join collapsed " +
            "the `..` AND a platform-blind trailing-`\\` check probed a DIFFERENT file ⇒ null ⇒ a " +
            "silently-tolerated fail-open)",
        );
      }
    }

    // (m) [LITERAL PLATFORM DELIMITER — host-independent helper] Both PATH-splitting loci (`resolvePathShim`,
    //     `sanitizePathValue`) now select the delimiter via the single `pathDelimiterFor(windows)` helper —
    //     the LITERAL platform delimiter (`;`/`:`) chosen SOLELY by the `windows` flag, NOT the ambient
    //     `node:path.delimiter` (which is `;` on a Windows host). Pre-fix both loci read `windows ? ";" :
    //     delimiter` (the imported host delimiter), so a `windows:false` POSIX-mode call ON A WINDOWS HOST
    //     split a POSIX PATH on `;` instead of `:` — making the unconditional POSIX-mode self-tests (F5 j,
    //     F5 a', F5 k, …) spuriously FAIL on a Windows host (a false regression for a Windows contributor,
    //     violating the CRITICAL cross-platform rule that the gate self-test runs on macOS/Windows/Linux).
    //     finding #2 only MISBEHAVES on a Windows host, so a value assertion cannot make it RED on THIS POSIX
    //     host. The discriminating signal here is the HELPER itself: it is the change, so a direct unit
    //     assertion on `pathDelimiterFor` FAILS pre-fix (the export did not exist ⇒ `typeof !== "function"`),
    //     and proves the delimiter is the LITERAL value selected by `windows`, host-independent. Tracked under
    //     its OWN `okM` flag with its OWN pass.
    {
      let okM = true;
      // The helper must EXIST — the export IS the change. Pre-fix `pathDelimiterFor` was not exported by
      // gate-internals.mjs, and an ESM NAMED import of a non-existent export is a MODULE-LINKING error
      // (`SyntaxError: ... does not provide an export named 'pathDelimiterFor'`) that throws at module-link
      // time, BEFORE any code in this self-test module runs — so pre-fix the whole self-test module would
      // FAIL TO LOAD and none of these assertions would even execute. The export's existence is precisely
      // what lets the F5(m) assertions run at all; given it loads, the `typeof !== "function"` guard below is
      // a harmless residual defensive check, and the assertions characterize that the helper returns the
      // LITERAL platform delimiter selected by `windows`.
      if (typeof pathDelimiterFor !== "function") {
        fail(
          `(F5 m) pathDelimiterFor is ${typeof pathDelimiterFor}, expected a function — the single literal-` +
            `delimiter selector consumed by resolvePathShim AND sanitizePathValue (pre-fix it did not exist)`,
        );
        okM = false;
      } else {
        // POSIX mode selects the LITERAL ":" and Windows mode the LITERAL ";" — host-independent. Pre-fix the
        // POSIX arm read the imported host `delimiter`, which on a Windows host is ";" (the bug).
        if (pathDelimiterFor(false) !== ":") {
          fail(
            `(F5 m) pathDelimiterFor(false) => ${JSON.stringify(pathDelimiterFor(false))}, expected the LITERAL ` +
              `":" regardless of host (pre-fix the POSIX arm read node:path.delimiter, which is ";" on a Windows host)`,
          );
          okM = false;
        }
        if (pathDelimiterFor(true) !== ";") {
          fail(
            `(F5 m) pathDelimiterFor(true) => ${JSON.stringify(pathDelimiterFor(true))}, expected the LITERAL ";"`,
          );
          okM = false;
        }
      }
      // NO-REGRESSION (POSIX-mode value checks): the POSIX-mode splitters still split on ":" on this host. These
      // pass pre-fix on a POSIX host too (they only discriminate finding #2 on a Windows host), so they are the
      // NO-REGRESSION guard, NOT the discriminator — the helper unit assertions above are the discriminator.
      const sanPosix = sanitizePathValue("a:/abs:b", false);
      if (sanPosix !== "/abs") {
        fail(
          `(F5 m) no-regression: sanitizePathValue("a:/abs:b", false) => ${JSON.stringify(sanPosix)}, expected ` +
            `"/abs" (POSIX-mode splits on the literal ":", keeps only the absolute)`,
        );
        okM = false;
      }
      // resolvePathShim POSIX-mode also splits on ":" — a two-entry ":"-PATH with the shim only in the 2nd.
      const dA = "/abs/tools/pa";
      const dB = "/abs/tools/pb";
      const onB = `${dB}/buf`;
      const exPosix2 = (p) => p === onB;
      const rPosix2 = resolvePathShim("buf", { PATH: `${dA}:${dB}` }, exPosix2, false);
      if (rPosix2 !== onB) {
        fail(
          `(F5 m) no-regression: resolvePathShim on a ":"-separated two-entry POSIX PATH (shim only in the 2nd) ` +
            `=> ${JSON.stringify(rPosix2)}, expected ${JSON.stringify(onB)} (POSIX-mode splits on the literal ":")`,
        );
        okM = false;
      }
      if (okM) {
        pass(
          "(F5 m) [LITERAL DELIMITER] pathDelimiterFor(windows) returns the LITERAL platform delimiter " +
            '(":" for POSIX, ";" for Windows) selected SOLELY by the `windows` flag, host-INDEPENDENT — and ' +
            "is the single selector consumed by BOTH resolvePathShim and sanitizePathValue, so a windows:false " +
            "self-test splits on `:` even on a Windows host (discriminating: pre-fix the export did not exist " +
            "and the POSIX arm read node:path.delimiter, which is `;` on a Windows host ⇒ the unconditional " +
            "POSIX-mode self-tests spuriously failed for a Windows contributor)",
        );
      }
    }

    if (ok) {
      pass(
        "(F5) CARGO-ENV PATH SANITIZATION: buildCargoEnv sanitizes the PATH var to its CWD-INDEPENDENT " +
          "ABSOLUTE components ONLY — keeping a leading-`/` absolute (incl. bare root `/`, `/.`, `/./`) on " +
          "POSIX and a drive-rooted `C:\\x` / `C:\\.`, a UNC `\\\\srv\\share`, or a device path on Windows, " +
          "while DROPPING empty, dot-only (`.`/`./`/`.\\`), non-dot relative (`bin`/`tools\\x`), `..`-relative " +
          "(`../tools`), Windows drive-relative (`C:foo`), and Windows root-relative (`\\x`/`/x`/`\\`/`\\.`) " +
          "entries — and never " +
          "creating a missing PATH; a PATH with NO absolute component (`:`/`:.`/`.`/all-relative) DELETES the " +
          'key (not assigns "") so Rust\'s var_os("PATH")? early-returns None; on Windows the PATH var is ' +
          "identified by CASE-INSENSITIVE key so a `PaTh`-cased env is sanitized/deleted (matching Rust " +
          "var_os) while POSIX leaves a `PaTh` var untouched (case-exact) — the verdict preflight resolver " +
          "AND the executed cargo/nextest/libtest tests resolve every tool from the SAME absolute-only PATH " +
          "(the CLOSED cwd-independent invariant, no preflight-vs-test disagreement) — discriminating: " +
          'pre-fix buildCargoEnv left PATH unchanged, assigned "" on all-implicit, KEPT non-dot relative / ' +
          "`..` / drive-relative / Windows root-relative entries, left a `PaTh` key unsanitized, and " +
          "sanitizePathValue did not exist",
      );
    }
  }

  // --------------------------------------------------------------------------------------------------
  // (F3) VERDICT GATING — the durable invariant at the verdict boundary. The exact freshness-pair name is
  //      tolerated ONLY when `freshnessToleranceAllowed === true`. We drive the REAL classifiers IN-PROCESS
  //      with the flag set both ways. Each case DISCRIMINATES against the pre-change signatures: pre-change
  //      `analyzeLibtestSurface(text, code, binaryId)` had NO tolerance gate and ALWAYS returned `tolerated`
  //      for the pair — so "pair + tolerance-disabled => FAIL" cannot pass against today's code.
  //        (1) nextest classifier: pair-only + tolerance=false => FAIL; + tolerance=true => PASS-WITH-TOLERATED.
  //        (2) nextest live-agg (exit 100, summary failed=1): pair + false => FAIL; + true => PASS-WITH-TOLERATED.
  //        (3) libtest analyzer (exit 101 + matching summary): pair + false => FAIL; + true => PASS-WITH-TOLERATED.
  //        (4) a CRASH whose ONLY name is the pair (libtest signal exit 134) => FAIL regardless of the flag
  //            (the crash hard-fail path is UNAFFECTED by tolerance).
  //        (5) a non-allowlisted FAIL => FAIL regardless of the flag.
  // --------------------------------------------------------------------------------------------------
  process.stderr.write("\n(F3) verdict gating on freshnessToleranceAllowed\n");
  {
    let ok = true;
    const NX_NAME =
      "cases::typeinfo_proto_ts_freshness::typeinfo_ts_bindings_are_byte_equal_to_regenerated_buf_output";
    const BIN = "verter_protocol::main";
    const LT_NAME =
      "cases::typeinfo_proto_ts_freshness::typeinfo_ts_bindings_are_byte_equal_to_regenerated_buf_output";

    // (1) nextest content classifier — pair-only FAIL line.
    const nxPairLog = `    FAIL [   0.012s] ${BIN} ${NX_NAME}\n`;
    const c1Off = verdictClassifyNextest(nxPairLog, false);
    const c1On = verdictClassifyNextest(nxPairLog, true);
    if (c1Off !== "FAIL") {
      fail(
        `(F3.1) nextest classifier: pair + tolerance=false => '${c1Off}', expected FAIL (tools present ⇒ a freshness FAIL is a hard regression)`,
      );
      ok = false;
    }
    if (c1On !== "PASS-WITH-TOLERATED") {
      fail(
        `(F3.1) nextest classifier: pair + tolerance=true => '${c1On}', expected PASS-WITH-TOLERATED`,
      );
      ok = false;
    }

    // (2) nextest LIVE aggregation — pair FAIL + exit 100 + matching Summary (failed=1).
    const nxPairRun =
      `    FAIL [   0.012s] ${BIN} ${NX_NAME}\n` +
      "     Summary [  62.968s] 15543 tests run: 15542 passed, 1 failed, 547 skipped\n";
    const r2Off = verdictNextestRun(100, nxPairRun, false);
    const r2On = verdictNextestRun(100, nxPairRun, true);
    if (r2Off !== "FAIL") {
      fail(`(F3.2) nextest live-agg: pair + tolerance=false => '${r2Off}', expected FAIL`);
      ok = false;
    }
    if (r2On !== "PASS-WITH-TOLERATED") {
      fail(
        `(F3.2) nextest live-agg: pair + tolerance=true => '${r2On}', expected PASS-WITH-TOLERATED`,
      );
      ok = false;
    }

    // (3) libtest analyzer — pair FAILED + exit 101 + matching summary (failed=1). The headline
    //     discriminator: pre-change there was NO tolerance gate, so this ALWAYS tolerated.
    const ltPair = `running 5 tests\ntest ${LT_NAME} ... FAILED\n\ntest result: FAILED. 4 passed; 1 failed; 0 ignored\n`;
    const l3Off = verdictLibtest(101, BIN, ltPair, false);
    const l3On = verdictLibtest(101, BIN, ltPair, true);
    if (l3Off !== "FAIL") {
      fail(
        `(F3.3) libtest analyzer: pair + tolerance=false => '${l3Off}', expected FAIL (pre-change had no gate and always tolerated)`,
      );
      ok = false;
    }
    if (l3On !== "PASS-WITH-TOLERATED") {
      fail(
        `(F3.3) libtest analyzer: pair + tolerance=true => '${l3On}', expected PASS-WITH-TOLERATED`,
      );
      ok = false;
    }

    // (4) a CRASH whose ONLY name is the pair — libtest signal exit (134), no summary => FAIL regardless.
    const ltCrash = `running 3 tests\ntest ${LT_NAME} ... FAILED\nthread 'main' panicked / SIGABRT: process abort\n`;
    const cr4Off = verdictLibtest(134, BIN, ltCrash, false);
    const cr4On = verdictLibtest(134, BIN, ltCrash, true);
    if (cr4Off !== "FAIL" || cr4On !== "FAIL") {
      fail(
        `(F3.4) libtest crash (exit 134) whose only name is the pair => off='${cr4Off}' on='${cr4On}', expected FAIL on BOTH (crash hard-fail is unaffected by tolerance)`,
      );
      ok = false;
    }
    // Also a nextest crash (SIGABRT status) whose only failure is the pair name => FAIL regardless.
    const nxCrash =
      `    SIGABRT [   0.204s] ${BIN} ${NX_NAME}\n` +
      "     Summary [   1.230s] 1 tests run: 0 passed, 1 failed, 0 skipped\n";
    const ncOff = verdictNextestRun(101, nxCrash, false);
    const ncOn = verdictNextestRun(101, nxCrash, true);
    if (ncOff !== "FAIL" || ncOn !== "FAIL") {
      fail(
        `(F3.4) nextest crash (SIGABRT) on the pair name => off='${ncOff}' on='${ncOn}', expected FAIL on BOTH`,
      );
      ok = false;
    }

    // (5) a non-allowlisted FAIL => FAIL regardless of the flag.
    const nxReal = `    FAIL [   0.030s] verter_compiler::main template::vmemo::renders_cached\n`;
    const re5Off = verdictClassifyNextest(nxReal, false);
    const re5On = verdictClassifyNextest(nxReal, true);
    if (re5Off !== "FAIL" || re5On !== "FAIL") {
      fail(
        `(F3.5) non-allowlisted FAIL => off='${re5Off}' on='${re5On}', expected FAIL on BOTH (tolerance never applies to a non-allowlisted name)`,
      );
      ok = false;
    }

    if (ok) {
      pass(
        "(F3) VERDICT GATING: the freshness pair is FAIL with tolerance=false and PASS-WITH-TOLERATED with " +
          "tolerance=true on BOTH the nextest classifier/live-agg AND the libtest analyzer; a crash on the pair " +
          "name and a non-allowlisted FAIL are FAIL regardless of the flag (discriminating — pre-change the " +
          "libtest analyzer had no gate and always tolerated the pair)",
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
