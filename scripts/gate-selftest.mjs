#!/usr/bin/env node
// gate-selftest.mjs — proves the safety properties of the gate using stand-ins except for GB15's real
// production CLI and Vapor/TypeScript harness paths, which run with Cargo/tool stand-ins.
//
// HOW IT DRIVES THE GATE PRIMITIVES (no magic flag on the production gate).
//   The classifier / verdict / sweep-matcher / suite-selection scenarios call the REAL gate functions
//   DIRECTLY, imported in-process from `gate-internals.mjs` (the same code `gate.mjs` composes) — NOT by
//   invoking `gate.mjs` with a test-seam flag (the production gate has none). The mutex / process-
//   containment / timeout / stall / teardown / seam scenarios — which genuinely need a real subprocess and
//   a real process group — spawn the SELF-TEST-ONLY runner `gate-selftest-runner.mjs`, which imports the
//   same gate primitives and runs them against `sleep`/`echo` stand-ins. The production gate (`gate.mjs`)
//   is exercised by the no-bypass assertions (scenario U-P0) and by GB15, which invokes the real
//   Vapor/TypeScript harness paths with Cargo/tool stand-ins.
//
// NO workspace Cargo builds/tests run here. GB15 replaces Cargo and auxiliary tools with stand-ins; other
// contained commands use `sleep`/`echo` stand-ins as applicable, so the build lock is never touched.
// Each test uses a UNIQUE lock dir (an os.tmpdir() mkdtemp) so a developer's real lock is
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
// A RED RUN HERE MAY NOT BE A REGRESSION — check which scenario failed first. Two distinct environment
// sensitivities are known and tracked as GI-18 in docs/contributing/gate-integrity-ledger.md:
//   (a) `(viii)` asserts a whole-gate budget against an ABSOLUTE ~6s window and IS load-sensitive.
//       Measured on this 8-core host: green at load 2-27, fails at load 67-102 (12x oversubscription)
//       with `took 11s ... did NOT bound the sequence near 6s`.
//   (b) `i-survival`, `vi` and `xvii` spawn processes and observe them in the process table. These are
//       NOT load-sensitive — they PASSED at that same load 67-102 — but four independent reviewer runs
//       saw them fail at load 1.6-7.6 inside restricted sandboxes. The mechanism is ESTABLISHED, not
//       inferred: one reported `mkdtemp` denial, and a later run ABORTED at `(i) MUTEX` with a direct
//       `EPERM` from `mkdtemp` at load 2.72, never reaching those three at all. If you are in a
//       container or a sandboxed shell, expect this and do not read it as a gate regression.
// The two sets are DISJOINT and inversely correlated with load, which is how load was ruled OUT for (b).
// Neither is a correctness signal about the gate itself: the classifier / verdict / parser scenarios are
// pure in-process computation with no clock, no spawn and no filesystem, and they are unaffected by both.
// This note is an INTERIM, and an inadequate one by design — a self-test whose job is detecting exactly
// this class cannot discharge it by asking the reader to notice. GI-18 owes the real fix: skip under a
// measured precondition, counted in SKIP and never in PASS.
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
//   (ix)   SURFACE-1 NON-FAIL — a crash (SIGABRT/SIGSEGV/…) or a setup/harness error (non-zero exit, no
//                           `FAIL [` line) classifies FAIL on both the classifier and the live-aggregation
//                           hook; the tolerated baseline stays PASS-WITH-TOLERATED.
//   (x)    FAIL-CLOSED MUTEX — an alive holder with an EMPTY/uncheckable start-identity REFUSES (126); a
//                           dead holder with empty identity still reclaims + PASSes (discriminating).
//   (xi)   LEGACY SURFACE-2 CLASSIFIER — zero / partial verter_session suite selection returns setup failure
//          (127); a proper
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
//   (GB9)  BUILD-PREREQUISITE PREFLIGHT — the gate distinguishes "the code is broken" from "an artifact was
//                           never built". Parts of the Rust suite load artifacts cargo does not build (the
//                           real-provider suites spawn tsserver with `--globalPlugins
//                           @verter/typescript-plugin`, whose entry is a `tsc -b` output `pnpm install` does
//                           NOT produce); without them ~64 `*_tsserver` tests failed with `TS2307: Cannot
//                           find module './Comp.vue'` and the gate reported them as ordinary regressions.
//                           The oracle is a REAL LOAD of that entry in a child process, so the case a
//                           stat-based check ACCEPTS is covered: both `index.js` present with one EMITTED
//                           HELPER missing still throws inside tsserver and must still refuse. Leg 1 drives
//                           the real `checkBuildPrerequisites` / `runBuildPrerequisiteLoadProbe` in-process
//                           over injected probe outcomes (incl. every fail-closed shape: spawn error,
//                           signal, timeout, unparseable output); legs 2-6 drive the REAL PRODUCTION CLI (a
//                           byte-copy rooted in a SYNTHETIC git repo holding a miniature of the package
//                           graph, so nothing in the developer's tree is touched and the production gate
//                           keeps its zero test seams). Discriminates in six directions: nothing built /
//                           plugin entry missing / language-shared missing / helper missing => exit 127
//                           carrying the marker, the probe target and the producer command, with NEITHER
//                           the freshness preflight NOR the archive build reached (the ordering half — the
//                           freshness preflight's `pnpm install` is what turns the silent-SKIP state into
//                           the 64-failure state); everything built => no refusal, SATISFIED, and the run
//                           PROCEEDS. Every plant is stat-PROVEN applied before the run and re-stated
//                           after it.
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
  statSync,
  copyFileSync,
  chmodSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
// The REAL gate primitives — imported in-process so the classifier/verdict/sweep-matcher/suite-selection
// scenarios drive the ACTUAL gate code, not a re-implementation and not a magic flag on the production CLI.
import {
  classifyNextestFailures,
  analyzeNextestSurface,
  parseNextestSummary,
  analyzeLibtestSurface,
  selectSessionSuites,
  ensureRequiredWindowsDebugSidecars,
  isBuildTool,
  targetDirMatches,
  classifyProcessIdentityComparison,
  normalizeWindowsWmicCreationDate,
  preparedSuccessLines,
  PREPARE_SUCCESS_MARKER,
  buildPrepareWarmSpawnEnv,
  classifyPrepareWarmResult,
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
  // build-prerequisite preflight — the non-cargo artifacts the Rust suite loads from disk (GB9).
  checkBuildPrerequisites,
  runBuildPrerequisiteLoadProbe,
  parseTsserverEnvDenylist,
  probeBudgetMs,
  BUILD_PREREQUISITE_PACKAGES,
  BUILD_PREREQUISITE_PROBE_SEGMENTS,
  BUILD_PREREQUISITE_PROBE_MAX_MS,
  BUILD_PREREQUISITE_COMMAND,
  BUILD_PREREQUISITE_MARKER,
  TSSERVER_ENV_DENYLIST_SOURCE_SEGMENTS,
  // real conformance-harness gate smokes (GB15/GB16).
  HARNESS_SMOKE_MARKER,
  HARNESS_SMOKE_MODES,
  CORE_HARNESS_SMOKE_MODES,
  BF2_HARNESS_SMOKE_MODES,
  harnessSmokeCommand,
  decideHarnessSmokeResult,
  formatHarnessSmokeFailure,
  // oracle-cache prerequisite preflight — the offline Svelte/Vue oracle npm cache the bf2-authoritative
  // conformance suites realize from (GB11).
  checkOracleCachePrerequisite,
  runOracleCacheLoadProbe,
  oracleCacheProbeBudgetMs,
  ORACLE_CACHE_PREREQUISITE_MARKER,
  ORACLE_CACHE_PROVISION_COMMAND,
  ORACLE_CACHE_PROBE_MAX_MS,
  ORACLE_CACHE_PROBE_MODULE_SEGMENTS,
  // core archive feature isolation + dedicated BF2 exact inventory + shipped-cfg scan (GB12).
  ARCHIVE_FEATURES,
  BF2_AUTHORITATIVE_FEATURE,
  BF2_AUTHORITATIVE_MODULES,
  buildBf2NextestArgs,
  countBf2AuthoritativeListTests,
  decideBf2AuthoritativeInventoryMatch,
  scanBf2AuthoritativeSourceInventory,
  buildNextestArchiveArgs,
  buildSurface1RunArgs,
  buildShippedCfgContractArgs,
  countTestAttributesInDir,
  decideShippedCfgGuardExpectedCountMatch,
  // local fail-fast / explicit exhaustive policy and coverage-complete receipt verdict (GB17).
  reduceGateLaneReceipts,
  SHIPPED_CFG_LANE_ENABLED,
  SHIPPED_CFG_SKIP_SUMMARY,
  SHIPPED_CFG_SKIP_VERDICT_NOTE,
  // trybuild exclusion (interim, pending maintainer disposition) — GB13.
  TRYBUILD_EXCLUDED_SUITES,
  buildTrybuildExclusionFilterExpr,
  trybuildSkipArgsForPackage,
  countTrybuildExclusionMatches,
  discoverCompilerTrybuildSourceModulePrefixes,
  compilerTrybuildDriverUsesCanonicalConstructor,
  // reused by the gate-failure-triage parsing scenarios (GB14) so a nextest recap fixture is read through
  // the SAME extractor the live gate/triage share — no second nextest-output parser.
  extractNextestTerminalFailures,
} from "./gate-internals.mjs";
// gate-failure-triage's pure parsing/classification helpers (no CLI, no cargo — see its own header). The
// REAL end-to-end proof (planted tests, real cargo nextest, REAL/FLAKY/INTERACTION classification) lives in
// the dedicated `triage-gate-failure-selftest.mjs`, which DOES run cargo and is NOT part of this cargo-free
// suite; here we exercise only the in-process log-parsing/classification contract.
import {
  parseGateVerdict,
  splitGateLogSurfaces,
  isSyntheticFailureName,
  buildIsolationFilter,
  quoteNextestFilterValue,
  resolveIsolationTargets,
  classifyAttempts,
} from "./triage-gate-internals.mjs";

const SELFTEST_DIR = dirname(fileURLToPath(import.meta.url));
// The PRODUCTION gate CLI — exercised by the U-P0 "no bypass mode" scenario and by GB15, which invokes
// the real Vapor/TypeScript harness paths with Cargo/tool stand-ins. Other scenarios use stand-ins as applicable.
const GATE = join(SELFTEST_DIR, "gate.mjs");
// The SELF-TEST-ONLY subprocess runner (mutex + containment + timeout/stall + teardown + seam, against
// sleep/echo stand-ins). It imports the same gate primitives; production never runs it.
const RUNNER = join(SELFTEST_DIR, "gate-selftest-runner.mjs");
// The memory-ceiling self-test (parseMemorySize / deriveGateResourceLimits / buildCargoEnv job-cap clamp /
// process-table RSS parsers / real MEMORY-MONITOR reap behavior). Run as a subprocess below so it is part
// of this same canonical self-test entrypoint instead of sitting unreferenced.
const MEMORY_SELFTEST = join(SELFTEST_DIR, "gate-memory-selftest.mjs");
// The multi-registration containment supervisor self-test. Its scripted cases prove aggregate authority
// and race semantics; its native cases prove exact cleanup of every registered process forest.
const LANE_SELFTEST = join(SELFTEST_DIR, "gate-lane-selftest.mjs");

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
const EXIT_USAGE = 127;

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

// Run a PRODUCTION gate.mjs CLI (the real file, or a byte-copy of it rooted elsewhere) and CAPTURE its
// output. Used by (GB9), which drives the real CLI against a SYNTHETIC repo root so it can observe the
// build-prerequisite refusal in both directions without mutating the developer's tree. Returns
// { code, out } where `out` is stdout+stderr concatenated.
function runGateCapture(gatePath, args, env, { cwd } = {}) {
  const child = { ...process.env, ...env };
  // A caller-provided PATH is an exact security boundary. On Windows, inherited case variants such as
  // `Path` and an override named `PATH` denote the same OS variable but can otherwise survive as two JS
  // object keys and make the spawned child's effective lookup path ambiguous. Collapse them before spawn.
  if (Object.hasOwn(env, "PATH")) {
    if (IS_WINDOWS) {
      for (const key of Object.keys(child)) {
        if (key.toUpperCase() === "PATH") delete child[key];
      }
    } else {
      delete child.PATH;
    }
    child.PATH = env.PATH;
  }
  // The gate honors VERTER_GATE_TARGET_DIR; every (GB9) leg passes --target-dir explicitly, so drop the
  // ambient value rather than let a developer's export decide where the synthetic run writes.
  delete child.VERTER_GATE_TARGET_DIR;
  const r = spawnSync(process.execPath, [gatePath, ...args], {
    env: child,
    cwd,
    encoding: "utf8",
    timeout: 300_000,
  });
  const out = `${r.stdout || ""}${r.stderr || ""}`;
  if (r.status === null && r.signal) return { code: 128, signal: r.signal, out };
  return { code: r.status === null ? 1 : r.status, out };
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

// Legacy Surface-2 libtest verdict fixture — mirrors the old `--selftest-libtest`.
function verdictLibtest(code, binaryId, text, freshnessToleranceAllowed = true) {
  const r = analyzeLibtestSurface(text, code, binaryId, freshnessToleranceAllowed);
  if (r.verdict === "fail") return "FAIL";
  if (r.verdict === "tolerated") return "PASS-WITH-TOLERATED";
  return "PASS";
}
function verdictLibtestFile(code, binaryId, file, freshnessToleranceAllowed = true) {
  return verdictLibtest(code, binaryId, readFileSync(file, "utf8"), freshnessToleranceAllowed);
}

// Legacy Surface-2 suite-selection fixture — mirrors the old `--selftest-surface2`. Returns the same { code, out }
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

// Return PATH with every component capable of resolving `tool` removed. GB15 uses this to make pnpm
// unresolvable while retaining the developer's git/process-inspection tools. The tested gate receives
// temporary buf/oxfmt shims before this remainder, so freshness preparation cannot install or lock the
// developer checkout even when its local node_modules shims are absent.
function pathWithoutTool(pathValue, tool) {
  const delimiter = pathDelimiterFor(IS_WINDOWS);
  return String(pathValue || "")
    .split(delimiter)
    .filter(
      (dir) =>
        dir !== "" && resolveExecutableShim(appendPathComponentRaw(dir, tool, IS_WINDOWS)) === null,
    )
    .join(delimiter);
}

function writeSuccessfulToolShim(dir, tool) {
  if (IS_WINDOWS) {
    const shim = join(dir, `${tool}.CMD`);
    writeFileSync(shim, "@ECHO OFF\r\n@EXIT /B 0\r\n");
    return shim;
  }
  const shim = join(dir, tool);
  writeFileSync(shim, "#!/bin/sh\nexit 0\n", { mode: 0o755 });
  return shim;
}

// ====================================================================================================
async function main() {
  process.stderr.write(
    "=== gate.mjs self-test (GB15: real production CLI + Vapor/TypeScript harness paths with Cargo/tool stand-ins; other scenarios use stand-ins as applicable) ===\n",
  );
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

  // (i) MUTEX — a second concurrent run must REFUSE with LOCK-REFUSED (126).
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
      "        FAIL [   0.012s] verter_protocol::main cases::typeinfo_proto_ts_freshness::typeinfo_ts_bindings_are_byte_equal_to_regenerated_buf_output\n",
    );
    // (b) an allowlisted test PLUS a non-allowlisted test failed => FAIL.
    writeFileSync(
      B,
      "        FAIL [   0.012s] verter_protocol::main cases::typeinfo_proto_ts_freshness::typeinfo_ts_bindings_are_byte_equal_to_regenerated_buf_output\n" +
        "        FAIL [   0.030s] verter_compiler::main template::vmemo::renders_cached\n",
    );
    // (c) a NON-allowlisted test whose name merely CONTAINS an allowlisted substring failed => FAIL.
    writeFileSync(
      C,
      "        FAIL [   0.041s] verter_session::main cases::typeinfo_proto_ts_freshness_lookalike::regresses\n",
    );
    // (d) a NON-allowlisted test whose exact final token is an ENTIRE allowlisted name PLUS a suffix => FAIL.
    writeFileSync(
      D,
      "        FAIL [   0.044s] verter_protocol::main cases::typeinfo_proto_ts_freshness::typeinfo_ts_bindings_are_byte_equal_to_regenerated_buf_output_extra\n",
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
  // (GB15) REAL CONFORMANCE-HARNESS COMMANDS + PRODUCTION ORDERING. Command construction is pure and
  //        launches the harness-owned executable through the current Node binary, once for each exact
  //        mode. Core must run only TypeScript before freshness/archive work; the dedicated BF2 command
  //        must run oracle realization then Vapor before its exact nextest list.
  // --------------------------------------------------------------------------------------------------
  process.stderr.write("\n(GB15) REAL CONFORMANCE-HARNESS COMMANDS + PRODUCTION ORDERING\n");
  {
    let ok = true;
    if (JSON.stringify(HARNESS_SMOKE_MODES) !== JSON.stringify(["vapor", "typescript"])) {
      fail(
        `(GB15.1) smoke modes must be exactly vapor/typescript; got ${JSON.stringify(HARNESS_SMOKE_MODES)}`,
      );
      ok = false;
    }
    if (
      JSON.stringify(CORE_HARNESS_SMOKE_MODES) !== JSON.stringify(["typescript"]) ||
      JSON.stringify(BF2_HARNESS_SMOKE_MODES) !== JSON.stringify(["vapor"])
    ) {
      fail(
        `(GB15.1) core must own only typescript and BF2 only vapor; got core=` +
          `${JSON.stringify(CORE_HARNESS_SMOKE_MODES)} bf2=${JSON.stringify(BF2_HARNESS_SMOKE_MODES)}`,
      );
      ok = false;
    }
    for (const mode of ["vapor", "typescript"]) {
      let command;
      try {
        command = harnessSmokeCommand(REPO_REALPATH, mode);
      } catch (error) {
        // Keep constructor validation strict, but never let it abort GB15 before the production-CLI
        // negative control below. An omitted production mode must be proven through the mutated real CLI.
        fail(`(GB15.1) ${mode} command construction failed: ${error.message}`);
        ok = false;
        continue;
      }
      const expectedScript = join(
        REPO_REALPATH,
        "packages",
        "framework-conformance-harness",
        "bin",
        "gate-smoke.mjs",
      );
      if (
        command.cmd !== process.execPath ||
        JSON.stringify(command.args) !== JSON.stringify([expectedScript, mode]) ||
        command.cwd !== REPO_REALPATH
      ) {
        fail(
          `(GB15.1) ${mode} command did not target the harness-owned executable: ${JSON.stringify(command)}`,
        );
        ok = false;
      }
    }

    const gateSource = readFileSync(GATE, "utf8");
    const oracleAt = gateSource.indexOf("const oraclePrereq = checkOracleCachePrerequisite(");
    const smokeAt = gateSource.indexOf("const harnessSmokesOk = await runHarnessSmokeChecks(ctx);");
    const freshnessAt = gateSource.indexOf("const preflight = await preflightFreshnessTooling(");
    const archiveAt = gateSource.indexOf("const out = await archiveAndList(ctx);", smokeAt);
    const bf2Source = readFileSync(join(SELFTEST_DIR, "bf2-authoritative.mjs"), "utf8");
    const bf2OracleAt = bf2Source.indexOf("const oracle = checkOracleCachePrerequisite(");
    const bf2VaporAt = bf2Source.indexOf("for (const mode of BF2_HARNESS_SMOKE_MODES)");
    const bf2ListAt = bf2Source.indexOf('buildBf2NextestArgs("list")');
    if (
      !(
        oracleAt === -1 &&
        smokeAt >= 0 &&
        smokeAt < freshnessAt &&
        smokeAt < archiveAt &&
        bf2OracleAt >= 0 &&
        bf2OracleAt < bf2VaporAt &&
        bf2VaporAt < bf2ListAt
      )
    ) {
      fail(
        `(GB15.2) core order must be TypeScript smoke -> freshness -> cargo archive with no oracle ` +
          `preflight, while dedicated BF2 must order oracle -> vapor smoke -> nextest list; got ` +
          `coreOracle=${oracleAt} smoke=${smokeAt} freshness=${freshnessAt} archive=${archiveAt} ` +
          `bf2Oracle=${bf2OracleAt} bf2Vapor=${bf2VaporAt} bf2List=${bf2ListAt}`,
      );
      ok = false;
    }
    const stubDir = freshTmpDir("gatetest-harness-smoke-cargostub-");
    const stubName = IS_WINDOWS ? "cargo.exe" : "cargo";
    const stubPath = join(stubDir, stubName);
    if (IS_WINDOWS) {
      // A copied Node launcher is a spawnable .exe stand-in. Cargo's first arg (`nextest`) is not a JS
      // file, so it fails immediately without reaching the real Cargo binary later on PATH.
      copyFileSync(process.execPath, stubPath);
    } else {
      writeFileSync(stubPath, "#!/bin/sh\nexit 9\n", { mode: 0o755 });
    }
    const bufShim = writeSuccessfulToolShim(stubDir, "buf");
    const oxfmtShim = writeSuccessfulToolShim(stubDir, "oxfmt");
    const delimiter = pathDelimiterFor(IS_WINDOWS);
    const hermeticPath = `${stubDir}${delimiter}${pathWithoutTool(process.env.PATH, "pnpm")}`;
    const smokeEnv = { PATH: hermeticPath };
    const pnpmPath = resolvePnpm(smokeEnv);
    const resolvedBuf = resolvePathShim("buf", smokeEnv);
    const resolvedOxfmt = resolvePathShim("oxfmt", smokeEnv);
    if (pnpmPath !== null || resolvedBuf !== bufShim || resolvedOxfmt !== oxfmtShim) {
      fail(
        `(GB15.3) hermetic smoke environment invalid: pnpm=${JSON.stringify(pnpmPath)} ` +
          `buf=${JSON.stringify(resolvedBuf)} oxfmt=${JSON.stringify(resolvedOxfmt)}`,
      );
      ok = false;
    }
    const smokeTarget = freshTmpDir("gatetest-harness-smoke-target-");
    const smokeLock = freshLock();
    // Fail closed without launching the real CLI if the no-installer proof above ever regresses.
    const live =
      pnpmPath === null && resolvedBuf === bufShim && resolvedOxfmt === oxfmtShim
        ? runGateCapture(
            GATE,
            ["--timeout", "120s", "--stall", "60s", "--target-dir", smokeTarget],
            { ...smokeEnv, VERTER_GATE_LOCK: smokeLock },
          )
        : { code: 1, out: "GB15 hermetic environment refused before production launch" };
    const vaporDone = live.out.indexOf("HARNESS-SMOKE [vapor]: SATISFIED");
    const typescriptDone = live.out.indexOf("HARNESS-SMOKE [typescript]: SATISFIED");
    const cargoStarted = live.out.indexOf(
      "archiving workspace test universe (dev profile) (cargo nextest archive --workspace)",
    );
    const freshnessWasNonInstalling =
      live.out.includes("freshness-tooling preflight: already-present") ||
      live.out.includes("freshness-tooling preflight: path-fallback");
    if (!freshnessWasNonInstalling || live.out.includes("freshness-tooling preflight: installed")) {
      fail(
        `(GB15.3) production smoke leg must prove a non-installing freshness path; output:\n${live.out}`,
      );
      ok = false;
    }
    if (!(vaporDone < 0 && typescriptDone >= 0 && typescriptDone < cargoStarted)) {
      fail(
        `(GB15.3) core must omit vapor and complete the real TypeScript smoke before Cargo; ` +
          `got vapor=${vaporDone} typescript=${typescriptDone} cargo=${cargoStarted}:\n${live.out}`,
      );
      ok = false;
    }
    if (ok) {
      pass(
        "(GB15) commands target the harness-owned executable in exact vapor/typescript modes; core runs " +
          "only TypeScript before freshness/Cargo, while BF2 owns oracle -> vapor -> exact list ordering",
      );
    }
  }

  // --------------------------------------------------------------------------------------------------
  // (GB16) FAIL-CLOSED SMOKE RECEIPTS. Drive the exact result decider used by runHarnessSmokeChecks over
  //        every incomplete shape: real non-zero smoke, timeout, signal, spawn failure, missing/invalid/
  //        mismatched receipt. Only an exact mode-bound receipt is accepted.
  // --------------------------------------------------------------------------------------------------
  process.stderr.write("\n(GB16) FAIL-CLOSED CONFORMANCE-HARNESS SMOKE RECEIPTS\n");
  {
    let ok = true;
    const receipt = (mode) => JSON.stringify({ schema: "verter-harness-smoke/v1", mode, ok: true });
    for (const mode of ["vapor", "typescript"]) {
      const accepted = decideHarnessSmokeResult(mode, {
        code: 0,
        reason: "",
        signalName: "",
        spawnError: false,
        stdout: receipt(mode),
      });
      if (!accepted.ok) {
        fail(`(GB16) exact ${mode} receipt must pass: ${JSON.stringify(accepted)}`);
        ok = false;
      }
    }
    const rejected = [
      ["vapor", { code: 3, reason: "", signalName: "", spawnError: false, stdout: "" }, "exit"],
      [
        "typescript",
        { code: 3, reason: "", signalName: "", spawnError: false, stdout: "" },
        "exit",
      ],
      [
        "vapor",
        { code: 0, reason: "TIMEOUT", signalName: "", spawnError: false, stdout: receipt("vapor") },
        "TIMEOUT",
      ],
      [
        "typescript",
        {
          code: 0,
          reason: "STALL",
          signalName: "",
          spawnError: false,
          stdout: receipt("typescript"),
        },
        "STALL",
      ],
      [
        "vapor",
        { code: 0, reason: "MEMORY", signalName: "", spawnError: false, stdout: receipt("vapor") },
        "MEMORY",
      ],
      [
        "typescript",
        {
          code: 0,
          reason: "MEMORY_MONITOR",
          signalName: "",
          spawnError: false,
          stdout: receipt("typescript"),
        },
        "MEMORY_MONITOR",
      ],
      [
        "typescript",
        { code: 128, reason: "", signalName: "SIGABRT", spawnError: false, stdout: "" },
        "SIGABRT",
      ],
      [
        "typescript",
        { code: 1, reason: "", signalName: "", spawnError: true, stdout: "" },
        "spawn",
      ],
      ["vapor", { code: 0, reason: "", signalName: "", spawnError: false, stdout: "" }, "missing"],
      [
        "vapor",
        { code: 0, reason: "", signalName: "", spawnError: false, stdout: "not-json" },
        "invalid",
      ],
      [
        "vapor",
        { code: 0, reason: "", signalName: "", spawnError: false, stdout: receipt("typescript") },
        "mode",
      ],
      [
        "typescript",
        {
          code: 0,
          reason: "",
          signalName: "",
          spawnError: false,
          stdout: JSON.stringify({ schema: "wrong", mode: "typescript", ok: true }),
        },
        "schema",
      ],
      [
        "typescript",
        {
          code: 0,
          reason: "",
          signalName: "",
          spawnError: false,
          stdout: JSON.stringify({
            schema: "verter-harness-smoke/v1",
            mode: "typescript",
            ok: false,
          }),
        },
        "receipt",
      ],
      [
        "vapor",
        {
          code: 0,
          reason: "",
          signalName: "",
          spawnError: false,
          stdout: JSON.stringify({
            schema: "verter-harness-smoke/v1",
            mode: "vapor",
            ok: true,
            extra: "must be rejected",
          }),
        },
        "keys",
      ],
      ["vapor", { code: 0, reason: "", signalName: "", spawnError: false, stdout: "[]" }, "object"],
      [
        "vapor",
        { code: 0, reason: "", signalName: "", spawnError: false, stdout: "null" },
        "object",
      ],
      [
        "typescript",
        { code: 0, reason: "", signalName: "", spawnError: false, stdout: '"receipt"' },
        "object",
      ],
      [
        "typescript",
        { code: 0, reason: "", signalName: "", spawnError: false, stdout: "42" },
        "object",
      ],
      [
        "typescript",
        { code: 0, reason: "", signalName: "", spawnError: false, stdout: "true" },
        "object",
      ],
    ];
    for (const [mode, result, marker] of rejected) {
      const decision = decideHarnessSmokeResult(mode, result);
      if (decision.ok || !decision.detail.includes(marker)) {
        fail(
          `(GB16) ${mode}/${marker} must fail closed with an actionable detail: ${JSON.stringify(decision)}`,
        );
        ok = false;
      }
      const attributed = formatHarnessSmokeFailure(mode, decision);
      const otherMode = mode === "vapor" ? "typescript" : "vapor";
      if (
        !attributed.startsWith(`${HARNESS_SMOKE_MARKER} [${mode}]:`) ||
        attributed.includes(`${HARNESS_SMOKE_MARKER} [${otherMode}]:`)
      ) {
        fail(`(GB16) failure marker must attribute exact mode ${mode}: ${attributed}`);
        ok = false;
      }
    }
    const gateSource = readFileSync(GATE, "utf8");
    const fnStart = gateSource.indexOf("async function runHarnessSmokeChecks(ctx)");
    const fnEnd = gateSource.indexOf("\n}\n", fnStart);
    const body = fnStart < 0 || fnEnd < 0 ? "" : gateSource.slice(fnStart, fnEnd);
    if (
      !body.includes("decideHarnessSmokeResult(mode, result)") ||
      !body.includes("formatHarnessSmokeFailure(mode, decision)")
    ) {
      fail(
        "(GB16) production runHarnessSmokeChecks must use the verified decider and exact mode formatter",
      );
      ok = false;
    }
    if (ok) {
      pass(
        "(GB16) only exact-key, mode-bound object receipts pass; every watchdog outcome, non-zero real " +
          "smoke, signal, spawn failure, and missing/invalid/mismatched receipt fails with exact mode attribution",
      );
    }
  }

  // --------------------------------------------------------------------------------------------------
  // (GB17) LOCAL FAIL-FAST / EXPLICIT EXHAUSTIVE POLICY. Pure argv, transition and verdict helpers are the
  //        primary behavior contract; the final bounded source check proves the production CLI wires those
  //        tested helpers rather than keeping an inline second authority. No Cargo command runs here.
  // --------------------------------------------------------------------------------------------------
  process.stderr.write("\n(GB17) LOCAL FAIL-FAST / EXPLICIT EXHAUSTIVE GATE POLICY\n");
  {
    let ok = true;
    const surfaceBase = {
      archiveFile: "C:/synthetic/dev.tar.zst",
      extractDir: "C:/synthetic/extract",
      repoRealpath: "C:/synthetic/repo",
      filterExpr: buildTrybuildExclusionFilterExpr(),
      testThreads: 7,
    };
    const localSurface = buildSurface1RunArgs(surfaceBase);
    const exhaustiveSurface = buildSurface1RunArgs({ ...surfaceBase, exhaustive: true });
    const expectedLocalSurface = [
      "nextest",
      "run",
      "--archive-file",
      surfaceBase.archiveFile,
      "--extract-to",
      surfaceBase.extractDir,
      "--extract-overwrite",
      "--workspace-remap",
      surfaceBase.repoRealpath,
      "-E",
      buildTrybuildExclusionFilterExpr(),
      "--test-threads",
      "7",
    ];
    const expectedExhaustiveSurface = expectedLocalSurface.slice();
    expectedExhaustiveSurface.splice(expectedExhaustiveSurface.length - 2, 0, "--no-fail-fast");
    if (JSON.stringify(localSurface) !== JSON.stringify(expectedLocalSurface)) {
      fail(
        `(GB17.1) bare/local Surface-1 argv must preserve selection and omit --no-fail-fast; got ` +
          JSON.stringify(localSurface),
      );
      ok = false;
    }
    if (JSON.stringify(exhaustiveSurface) !== JSON.stringify(expectedExhaustiveSurface)) {
      fail(
        `(GB17.2) exhaustive Surface-1 argv must differ only by one --no-fail-fast; got ` +
          JSON.stringify(exhaustiveSurface),
      );
      ok = false;
    }

    const contractBase = { timingsEnabled: true, testThreads: 7 };
    const localContract = buildShippedCfgContractArgs(contractBase);
    const exhaustiveContract = buildShippedCfgContractArgs({ ...contractBase, exhaustive: true });
    if (localContract.includes("--no-fail-fast")) {
      fail(`(GB17.3) bare/local shipped contract must omit --no-fail-fast: ${localContract}`);
      ok = false;
    }
    if ((exhaustiveContract.filter((arg) => arg === "--no-fail-fast").length ?? 0) !== 1) {
      fail(
        `(GB17.3) exhaustive shipped contract must carry exactly one --no-fail-fast: ` +
          exhaustiveContract,
      );
      ok = false;
    }
    if (
      JSON.stringify(localContract) !==
      JSON.stringify(exhaustiveContract.filter((arg) => arg !== "--no-fail-fast"))
    ) {
      fail(
        "(GB17.3) execution policy must not change shipped package/profile/thread/timing selection",
      );
      ok = false;
    }

    const completeSurface = {
      hardFailure: false,
      failures: [],
      toleratedOccurred: false,
      coverage: { parseable: true, complete: true },
    };
    const completeShipped = {
      hardFailure: false,
      failures: [],
      check: { status: "ok" },
      contract: { status: "ok", parseable: true, complete: true },
      parity: { complete: true, matches: true },
    };
    const receiptRows = [
      [{ surface: completeSurface, shipped: completeShipped }, "PASS", true],
      [
        { surface: { ...completeSurface, toleratedOccurred: true }, shipped: completeShipped },
        "PASS-WITH-TOLERATED",
        true,
      ],
      [
        {
          surface: {
            ...completeSurface,
            hardFailure: true,
            failures: [{ surface: "nextest", name: "cases::real_failure" }],
          },
          shipped: completeShipped,
        },
        "FAIL",
        true,
      ],
      [{ surface: completeSurface, shipped: null }, "FAIL", false],
      [
        {
          surface: completeSurface,
          shipped: {
            ...completeShipped,
            contract: { status: "not-run", parseable: false, complete: false },
          },
        },
        "FAIL",
        false,
      ],
    ];
    for (const [receipts, expectedVerdict, expectedCoverage] of receiptRows) {
      const decision = reduceGateLaneReceipts(receipts);
      if (
        decision.verdict !== expectedVerdict ||
        decision.coverageComplete !== expectedCoverage ||
        (!expectedCoverage && !decision.failures.some((row) => row.surface === "gate/incomplete"))
      ) {
        fail(
          `(GB17.5) receipt coverage row expected ${expectedVerdict}/${expectedCoverage}, got ` +
            JSON.stringify(decision),
        );
        ok = false;
      }
    }
    const skippedPass = reduceGateLaneReceipts({
      surface: completeSurface,
      shipped: null,
      shippedCfgLaneEnabled: false,
    });
    if (skippedPass.verdict !== "PASS" || skippedPass.coverageComplete !== true) {
      fail(
        `(GB17.5) Surface-1-only skip must PASS from a complete Surface receipt without a shipped lane, ` +
          `got ${JSON.stringify(skippedPass)}`,
      );
      ok = false;
    }
    const skippedIncomplete = reduceGateLaneReceipts({
      surface: { ...completeSurface, coverage: { parseable: false, complete: false } },
      shipped: null,
      shippedCfgLaneEnabled: false,
    });
    if (
      skippedIncomplete.verdict !== "FAIL" ||
      skippedIncomplete.coverageComplete !== false ||
      !skippedIncomplete.failures.some((row) => row.surface === "gate/incomplete")
    ) {
      fail(
        `(GB17.5) Surface-1-only skip must still FAIL on an incomplete Surface receipt, ` +
          `got ${JSON.stringify(skippedIncomplete)}`,
      );
      ok = false;
    }

    const strictCases = [
      ["retired gate flag", ["--no-fail-fast"]],
      ["prepare + exhaustive", ["--prepare", "--exhaustive"]],
      ["help + exhaustive", ["--help", "--exhaustive"]],
      ["exhaustive + unknown", ["--exhaustive", "--bad"]],
    ];
    for (const [label, argv] of strictCases) {
      const result = runGate(argv, {});
      if (result.code !== EXIT_USAGE) {
        fail(`(GB17.6) ${label} must exit 127; got ${result.code}`);
        ok = false;
      }
    }
    const parseRoot = freshTmpDir("gatetest-exhaustive-argv-");
    let parseGit = true;
    try {
      execFileSync("git", ["init", "-q", parseRoot], { stdio: "ignore" });
    } catch {
      parseGit = false;
    }
    if (!parseGit) {
      skip("(GB17.6) positive --exhaustive production parse skipped because git is unavailable");
    } else {
      const parseScripts = join(parseRoot, "scripts");
      mkdirSync(parseScripts, { recursive: true });
      for (const name of ["gate.mjs", "gate-internals.mjs"]) {
        writeFileSync(join(parseScripts, name), readFileSync(join(SELFTEST_DIR, name)));
      }
      const accepted = runGateCapture(
        join(parseScripts, "gate.mjs"),
        ["--exhaustive", "--target-dir", join(parseRoot, "target", "gate-runner")],
        { VERTER_GATE_LOCK: join(parseRoot, "gate.lock.d") },
      );
      if (
        accepted.code !== EXIT_USAGE ||
        !accepted.out.includes(BUILD_PREREQUISITE_MARKER) ||
        accepted.out.includes("unknown argument")
      ) {
        fail(
          `(GB17.6) positive --exhaustive must parse and reach the synthetic prerequisite refusal: ` +
            `rc=${accepted.code}\n${accepted.out}`,
        );
        ok = false;
      }

      const noToolsPath = join(parseRoot, "no-tools-on-path");
      mkdirSync(noToolsPath, { recursive: true });
      const gateValueFlags = [
        "--timeout",
        "--stall",
        "--target-dir",
        "--build-jobs",
        "--test-threads",
        "--memory-limit",
      ];
      const prepareValueFlags = gateValueFlags.filter((flag) => flag !== "--test-threads");
      const malformedValueCases = [
        ...gateValueFlags.flatMap((flag) => [
          [`gate ${flag} missing value`, [flag]],
          [`gate ${flag} before policy flag`, [flag, "--exhaustive"]],
        ]),
        ...prepareValueFlags.flatMap((flag) => [
          [`prepare ${flag} missing value`, ["--prepare", flag]],
          [`prepare ${flag} before policy flag`, ["--prepare", flag, "--exhaustive"]],
        ]),
        ["gate target-dir before retired flag", ["--target-dir", "--no-fail-fast"]],
        ["prepare target-dir before retired flag", ["--prepare", "--target-dir", "--no-fail-fast"]],
        ["target-dir before prepare mode", ["--target-dir", "--prepare"]],
        ["gate target-dir before value flag", ["--target-dir", "--test-threads", "4"]],
      ];
      for (const [label, argv] of malformedValueCases) {
        const malformed = runGateCapture(join(parseScripts, "gate.mjs"), argv, {
          PATH: noToolsPath,
          VERTER_GATE_LOCK: join(parseRoot, "malformed.lock.d"),
        });
        if (
          malformed.code !== EXIT_USAGE ||
          !malformed.out.includes("ARGUMENT VALUE ERROR") ||
          malformed.out.includes(BUILD_PREREQUISITE_MARKER)
        ) {
          fail(
            `(GB17.6) ${label} must fail at strict value parsing before setup/Cargo: ` +
              `rc=${malformed.code}\n${malformed.out}`,
          );
          ok = false;
        }
      }
    }

    const gateSource = readFileSync(GATE, "utf8");
    const runGateStart = gateSource.indexOf("async function runGate(opts, ctx)");
    const runGateEnd = gateSource.indexOf("\n}\n", runGateStart);
    const runGateBody =
      runGateStart < 0 || runGateEnd < 0 ? "" : gateSource.slice(runGateStart, runGateEnd);
    const commandPlanAt = runGateBody.indexOf("buildGateLaneCommandPlan({");
    const commandPlanEnd = runGateBody.indexOf("\n  });", commandPlanAt);
    const commandPlanCall =
      commandPlanAt < 0 || commandPlanEnd < 0
        ? ""
        : runGateBody.slice(commandPlanAt, commandPlanEnd);
    const continuationAt = runGateBody.indexOf("await orchestrateGateLanes({");
    const shippedAt = runGateBody.indexOf(
      "runShippedCfgLane(opts, ctx, { allSuites, commandPlan })",
    );
    const finalizerAt = runGateBody.indexOf("reduceGateLaneReceipts(receipts)");
    const passVerdictCount = (runGateBody.match(/"VERDICT: PASS/g) || []).length;
    const firstPassVerdictAt = runGateBody.indexOf('"VERDICT: PASS');
    const shippedFnStart = gateSource.indexOf("async function runShippedCfgLane(");
    const shippedFnEnd = gateSource.indexOf("\n}\n", shippedFnStart);
    const shippedFnBody =
      shippedFnStart < 0 || shippedFnEnd < 0 ? "" : gateSource.slice(shippedFnStart, shippedFnEnd);
    if (
      commandPlanAt < 0 ||
      !commandPlanCall.includes("exhaustive: opts.exhaustive") ||
      !commandPlanCall.includes("filterExpr: SURFACE_1_FILTER") ||
      !(continuationAt >= 0 && continuationAt < shippedAt) ||
      finalizerAt < shippedAt ||
      passVerdictCount !== 2 ||
      firstPassVerdictAt <= finalizerAt ||
      !shippedFnBody.includes("guard.summary.initialCount") ||
      !shippedFnBody.includes("guard.summary.unrun === 0") ||
      !shippedFnBody.includes('runStep("shipped-cfg"') ||
      !runGateBody.includes("SHIPPED_CFG_LANE_ENABLED") ||
      !runGateBody.includes("SHIPPED_CFG_SKIP_SUMMARY") ||
      !runGateBody.includes("SHIPPED_CFG_SKIP_VERDICT_NOTE")
    ) {
      fail(
        `(GB17.7) production runGate must wire the tested argv/transition/completion/finalizer helpers: ` +
          `plan=${commandPlanAt} continuation=${continuationAt} shipped=${shippedAt} ` +
          `finalizer=${finalizerAt} pass-sites=${passVerdictCount}/${firstPassVerdictAt} ` +
          `shipped-body=${shippedFnBody.length}`,
      );
      ok = false;
    }
    if (ok) {
      pass(
        "(GB17) bare/local argv omits --no-fail-fast, exhaustive argv adds it to both nextest runs " +
          "without changing selection; local stops the shipped guard after hard failures; missing required " +
          "coverage defeats empty and tolerated-only PASS paths; argv is strict; production wires the tested helpers",
      );
    }
  }

  // --------------------------------------------------------------------------------------------------
  // (GB19) PARALLEL-LANE PRODUCTION WIRING. This is deliberately a bounded call-site check, not a broad
  // source mirror: the pure lane self-test owns behavior, while this proves the production gate invokes
  // that tested orchestration once after the one unchanged archive/list front half.
  // --------------------------------------------------------------------------------------------------
  process.stderr.write("\n(GB19) PARALLEL-LANE PRODUCTION WIRING\n");
  {
    const gateSource = readFileSync(GATE, "utf8");
    const runGateStart = gateSource.indexOf("async function runGate(opts, ctx)");
    const runGateEnd = gateSource.indexOf("\n}\n\nmain().catch", runGateStart);
    const runGateBody =
      runGateStart < 0 || runGateEnd < 0 ? "" : gateSource.slice(runGateStart, runGateEnd);
    const archiveCalls = (runGateBody.match(/await archiveAndList\(ctx\)/g) || []).length;
    const orchestrationCalls = (runGateBody.match(/await orchestrateGateLanes\(\{/g) || []).length;
    const surfaceLaneCalls = (runGateBody.match(/runSurface1Lane\(/g) || []).length;
    const shippedLaneCalls = (runGateBody.match(/runShippedCfgLane\(/g) || []).length;
    const reductionCalls = (runGateBody.match(/reduceGateLaneReceipts\(/g) || []).length;
    const replayCalls = (runGateBody.match(/replayGateLaneTranscript\(/g) || []).length;
    const layoutAt = gateSource.indexOf("deriveGateLaneLayout(runnerTarget, gateDir)");
    const supervisorCount = (gateSource.match(/createGateRunSupervisor\(\{/g) || []).length;
    const supervisorAt = gateSource.indexOf("createGateRunSupervisor({");
    const supervisorEnd = gateSource.indexOf("\n  });", supervisorAt);
    const supervisorCall =
      supervisorAt < 0 || supervisorEnd < 0 ? "" : gateSource.slice(supervisorAt, supervisorEnd);
    const mutexOwnedProvenanceUmbrella = /ownershipRoots:\s*\[runnerTarget\]/.test(supervisorCall);
    const separateSurfaceEnv =
      gateSource.includes("const surfaceCargoEnv =") &&
      gateSource.includes("laneLayout.surface1.targetDir");
    const separateShippedEnv =
      gateSource.includes("const shippedCargoEnv =") &&
      gateSource.includes("laneLayout.shippedCfg.targetDir");
    const localCancellation = runGateBody.includes("ctx.supervisor.cancelLane(laneId, reason)");
    const closeAt = gateSource.indexOf('supervisor.closeAndReapAll("GATE_TEARDOWN")');
    const mutexReleaseAt = gateSource.indexOf("mutex.release();", closeAt);
    if (
      archiveCalls !== 1 ||
      orchestrationCalls !== 1 ||
      surfaceLaneCalls !== 1 ||
      shippedLaneCalls !== 1 ||
      reductionCalls !== 1 ||
      replayCalls !== 1 ||
      layoutAt < 0 ||
      supervisorCount !== 1 ||
      !mutexOwnedProvenanceUmbrella ||
      !separateSurfaceEnv ||
      !separateShippedEnv ||
      !localCancellation ||
      !(closeAt >= 0 && closeAt < mutexReleaseAt)
    ) {
      fail(
        `(GB19) production must derive one disjoint layout, keep one archive/list and one supervisor, ` +
          `then invoke one Surface lane, one shipped lane, one concurrent orchestrator, one fixed-order ` +
          `reducer and one canonical replay: archive=${archiveCalls} orchestrate=${orchestrationCalls} ` +
          `surface=${surfaceLaneCalls} shipped=${shippedLaneCalls} reduce=${reductionCalls} ` +
          `replay=${replayCalls} layout=${layoutAt} supervisors=${supervisorCount} ` +
          `provenance-umbrella=${mutexOwnedProvenanceUmbrella} ` +
          `envs=${separateSurfaceEnv}/${separateShippedEnv} cancel=${localCancellation} ` +
          `teardown=${closeAt}/${mutexReleaseAt}`,
      );
    } else {
      pass(
        "(GB19) production wires the tested disjoint layout and one-supervisor concurrent lane boundary " +
          "exactly once after the unchanged single archive/list front half, with the mutex-owned runner " +
          "target as its sole configured provenance umbrella",
      );
    }
  }

  // --------------------------------------------------------------------------------------------------
  // (GB20) LANE RESOURCE SPLIT VALUE WIRING — BEHAVIORAL. GB19 proves the STRUCTURE (one layout, one
  // supervisor, two envs) is wired; this proves the actual per-lane cargo VALUES reach the real
  // production CLI rather than, say, a comment or a dead occurrence. It drives the REAL production CLI
  // end-to-end against a controlled `cargo` stand-in on PATH. Production currently skips the shipped-cfg
  // lane (`SHIPPED_CFG_LANE_ENABLED=false`): Surface 1 must receive the full `--build-jobs 8` /
  // `--test-threads 8` ceiling, shipped-cfg cargo must never be invoked, and the skip must be disclosed
  // in the captured output. Flipping the constant back to true MUST restore the previous 6/6 vs 2/2
  // split assertions (surface=6, shipped=2 via `SHIPPED_CFG_LANE_SHARE=0.25`) and require all three
  // cargo invocations.
  // --------------------------------------------------------------------------------------------------
  process.stderr.write("\n(GB20) LANE RESOURCE SPLIT VALUE WIRING\n");
  posix_gb20: {
    if (IS_WINDOWS) {
      skip(
        "(GB20) LANE RESOURCE SPLIT VALUE WIRING — POSIX bash cargo-stub + PATH override (no portable " +
          "Windows cargo-stub stand-in here)",
      );
      break posix_gb20;
    }
    // Same precondition discipline as (xix): this drives the REAL gate against the REAL repo root, so it
    // must reach past the build-prerequisite preflight before it can reach cargo at all. See (xix) above
    // for the full rationale on the module-not-found-only skip vs. any-other-failure fail.
    const gb20Prereq = checkBuildPrerequisites({ repoRoot: REPO_REALPATH });
    if (!gb20Prereq.ok && gb20Prereq.reason === "module-not-found") {
      skip(
        "(GB20) LANE RESOURCE SPLIT VALUE WIRING — SKIPPED: this tree's build prerequisites are absent, " +
          `so the real gate exits 127 before reaching cargo (${gb20Prereq.detail.split("\n")[0]}). Build ` +
          `them with \`${BUILD_PREREQUISITE_COMMAND}\` and re-run to exercise this scenario.`,
      );
      break posix_gb20;
    }
    if (!gb20Prereq.ok) {
      fail(
        `(GB20) the build-prerequisite probe could not ANSWER (reason=${gb20Prereq.reason}): ` +
          `${gb20Prereq.detail.split("\n")[0]}. That is an infrastructure failure, not a missing build, ` +
          "so this scenario must FAIL rather than skip.",
      );
      break posix_gb20;
    }

    const stubDir = freshTmpDir("gatetest-gb20-cargostub-");
    const stubPath = join(stubDir, "cargo");
    const listJsonPath = join(stubDir, "list.json");
    const surfaceMarker = join(stubDir, "surface.marker");
    const shippedCheckMarker = join(stubDir, "shipped-check.marker");
    const shippedContractMarker = join(stubDir, "shipped-contract.marker");

    // A minimal but VALID `cargo nextest list --message-format json` fixture: one testcase per
    // TRYBUILD_EXCLUDED_SUITES row, so archiveAndList's own trybuild-coverage guard
    // (verifyTrybuildExclusionCoverage) is satisfied for real — this exercises the gate's REAL post-list
    // wiring rather than a shortcut around it.
    const suitesJson = {};
    TRYBUILD_EXCLUDED_SUITES.forEach((row, i) => {
      suitesJson[`${row.package}::bin${i}`] = {
        "package-name": row.package,
        "binary-id": `${row.package}::bin${i}`,
        "binary-path": join(stubDir, `bin${i}`),
        testcases: { [`${row.modulePrefix}dummy`]: {} },
      };
    });
    writeFileSync(
      listJsonPath,
      JSON.stringify({
        "rust-build-meta": { "target-directory": stubDir },
        "rust-suites": suitesJson,
      }),
      "utf8",
    );

    // The stub dispatches on the REAL argv shape each production call site builds (buildNextestArchiveArgs
    // / the list step / buildSurface1RunArgs / buildShippedCfgCheckArgs / buildShippedCfgContractArgs) and
    // records the ACTUAL env + argv each per-lane invocation receives, then fails fast (archive/list
    // succeed so the gate reaches the lanes; the lane invocations themselves exit non-zero once recorded,
    // since only their observed inputs — never the overall verdict — are under test here).
    writeFileSync(
      stubPath,
      `#!/usr/bin/env bash
if [ "$1" = "nextest" ] && [ "$2" = "archive" ]; then
  prev=""
  archfile=""
  for a in "$@"; do
    if [ "$prev" = "--archive-file" ]; then archfile="$a"; fi
    prev="$a"
  done
  : > "$archfile"
  exit 0
elif [ "$1" = "nextest" ] && [ "$2" = "list" ]; then
  cat "${listJsonPath}"
  exit 0
elif [ "$1" = "nextest" ] && [ "$2" = "run" ] && [ "$3" = "--archive-file" ]; then
  { printf 'CARGO_BUILD_JOBS=%s\\n' "$CARGO_BUILD_JOBS"; printf 'ARGS %s\\n' "$*"; } >> "${surfaceMarker}"
  exit 1
elif [ "$1" = "check" ]; then
  { printf 'CARGO_BUILD_JOBS=%s\\n' "$CARGO_BUILD_JOBS"; printf 'ARGS %s\\n' "$*"; } >> "${shippedCheckMarker}"
  exit 0
elif [ "$1" = "nextest" ] && [ "$2" = "run" ] && [ "$3" = "-p" ]; then
  { printf 'CARGO_BUILD_JOBS=%s\\n' "$CARGO_BUILD_JOBS"; printf 'ARGS %s\\n' "$*"; } >> "${shippedContractMarker}"
  exit 1
else
  exit 0
fi
`,
      { mode: 0o755 },
    );
    try {
      chmodSync(stubPath, 0o755);
    } catch {
      /* ignore */
    }

    const stubPATH = `${stubDir}:${process.env.PATH || ""}`;
    const lk = freshLock();
    const tgt = freshTmpDir("gatetest-gb20-target-");
    const r = runGateCapture(
      GATE,
      [
        "--build-jobs",
        "8",
        "--test-threads",
        "8",
        "--memory-limit",
        "4GiB",
        "--target-dir",
        tgt,
        "--timeout",
        "180s",
        "--stall",
        "90s",
      ],
      { PATH: stubPATH, VERTER_GATE_LOCK: lk },
    );

    const surfaceRaw = existsSync(surfaceMarker) ? readFileSync(surfaceMarker, "utf8") : "";
    const shippedCheckRaw = existsSync(shippedCheckMarker)
      ? readFileSync(shippedCheckMarker, "utf8")
      : "";
    const shippedContractRaw = existsSync(shippedContractMarker)
      ? readFileSync(shippedContractMarker, "utf8")
      : "";
    const surfaceBuildJobs = (surfaceRaw.match(/CARGO_BUILD_JOBS=(\d+)/) || [])[1];
    const surfaceTestThreads = (surfaceRaw.match(/--test-threads (\d+)/) || [])[1];
    const shippedCheckBuildJobs = (shippedCheckRaw.match(/CARGO_BUILD_JOBS=(\d+)/) || [])[1];
    const shippedContractBuildJobs = (shippedContractRaw.match(/CARGO_BUILD_JOBS=(\d+)/) || [])[1];
    const shippedContractTestThreads = (shippedContractRaw.match(/--test-threads (\d+)/) || [])[1];

    note(
      `real gate + cargo-stub dispatcher => rc=${r.code}; surface build-jobs=${surfaceBuildJobs} ` +
        `test-threads=${surfaceTestThreads}; shipped-check build-jobs=${shippedCheckBuildJobs}; ` +
        `shipped-contract build-jobs=${shippedContractBuildJobs} test-threads=${shippedContractTestThreads}`,
    );

    if (SHIPPED_CFG_LANE_ENABLED) {
      const allInvoked = surfaceRaw !== "" && shippedCheckRaw !== "" && shippedContractRaw !== "";
      const EXPECT_SURFACE = "6";
      const EXPECT_SHIPPED = "2";
      if (!allInvoked) {
        fail(
          "(GB20) LANE RESOURCE SPLIT VALUE WIRING: the real per-lane cargo invocation(s) were never " +
            `observed (surface-invoked=${surfaceRaw !== ""} shipped-check-invoked=${shippedCheckRaw !== ""} ` +
            `shipped-contract-invoked=${shippedContractRaw !== ""}) — the gate did not reach the lanes under ` +
            `test (rc=${r.code}). Tail of captured output:\n${r.out.slice(-4000)}`,
        );
      } else if (
        surfaceBuildJobs !== EXPECT_SURFACE ||
        surfaceTestThreads !== EXPECT_SURFACE ||
        shippedCheckBuildJobs !== EXPECT_SHIPPED ||
        shippedContractBuildJobs !== EXPECT_SHIPPED ||
        shippedContractTestThreads !== EXPECT_SHIPPED
      ) {
        fail(
          "(GB20) production must thread deriveGateLaneResourceSplit's actual per-lane VALUES into the " +
            "REAL cargo env (CARGO_BUILD_JOBS) and command line (--test-threads) each lane's own cargo " +
            "process actually receives — not opts.buildJobs/opts.testThreads (the pre-fix un-split ceiling, " +
            `applied twice): expected surface=${EXPECT_SURFACE}/${EXPECT_SURFACE}, ` +
            `shipped=${EXPECT_SHIPPED}/${EXPECT_SHIPPED}; observed surface build-jobs=${surfaceBuildJobs} ` +
            `test-threads=${surfaceTestThreads}, shipped-check build-jobs=${shippedCheckBuildJobs}, ` +
            `shipped-contract build-jobs=${shippedContractBuildJobs} test-threads=${shippedContractTestThreads}`,
        );
      } else {
        pass(
          "(GB20) LANE RESOURCE SPLIT VALUE WIRING: driving the REAL gate.mjs CLI against a controlled " +
            "cargo stand-in (--build-jobs 8 --test-threads 8, a concurrent, non-degenerate split) proves the " +
            "ACTUAL per-lane cargo invocations receive deriveGateLaneResourceSplit's split values — surface " +
            "CARGO_BUILD_JOBS=6/--test-threads 6, shipped-cfg CARGO_BUILD_JOBS=2/--test-threads 2 — never the " +
            "un-split 8/8 ceiling applied twice; a behavioral proof over the real command/env each lane's " +
            "cargo process is actually handed, discriminating regardless of gate.mjs's source text",
        );
      }
    } else {
      const EXPECT_SURFACE = "8";
      const skipDisclosed =
        r.out.includes(SHIPPED_CFG_SKIP_SUMMARY) && r.out.includes(SHIPPED_CFG_SKIP_VERDICT_NOTE);
      if (surfaceRaw === "") {
        fail(
          "(GB20) Surface 1 cargo was never invoked while the shipped-cfg lane is skipped " +
            `(rc=${r.code}). Tail of captured output:\n${r.out.slice(-4000)}`,
        );
      } else if (shippedCheckRaw !== "" || shippedContractRaw !== "") {
        fail(
          "(GB20) shipped-cfg cargo must not run while SHIPPED_CFG_LANE_ENABLED is false: " +
            `shipped-check-invoked=${shippedCheckRaw !== ""} shipped-contract-invoked=${shippedContractRaw !== ""}`,
        );
      } else if (surfaceBuildJobs !== EXPECT_SURFACE || surfaceTestThreads !== EXPECT_SURFACE) {
        fail(
          "(GB20) while the shipped-cfg lane is skipped, Surface 1 must receive the full " +
            `--build-jobs/--test-threads ceiling (expected ${EXPECT_SURFACE}/${EXPECT_SURFACE}), not a ` +
            `split leftover: observed build-jobs=${surfaceBuildJobs} test-threads=${surfaceTestThreads}`,
        );
      } else if (!skipDisclosed) {
        fail(
          "(GB20) a skipped shipped-cfg lane must disclose the skip in the captured output " +
            `(missing ${JSON.stringify(SHIPPED_CFG_SKIP_SUMMARY)} and/or ` +
            `${JSON.stringify(SHIPPED_CFG_SKIP_VERDICT_NOTE)})\n${r.out.slice(-4000)}`,
        );
      } else {
        pass(
          "(GB20) shipped-cfg lane skip is behavioral: driving the REAL gate.mjs CLI against a " +
            "controlled cargo stand-in (--build-jobs 8 --test-threads 8) invokes Surface 1 at the full " +
            "8/8 ceiling, never launches shipped-cfg cargo, and discloses the skip in the verdict/summary",
        );
      }
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
    const unaccountedPlusTolerated = join(fixDir, "unaccounted_plus_tolerated.log");
    const setupError = join(fixDir, "setup_error.log");
    const tolerated = join(fixDir, "tolerated.log");
    // A SIGABRT crash with NO `FAIL [` line; the summary still counts it as failed and the run exits
    // non-zero. Pre-fix: classified PASS (no FAIL line). Post-fix: FAIL.
    writeFileSync(
      sigabrt,
      "        PASS [   0.010s] verter_compiler template::renders\n" +
        "     SIGABRT [   0.204s] verter_other crash::aborts_in_drop\n" +
        "     Summary [   1.230s] 2 tests run: 1 passed, 1 failed, 0 skipped\n",
    );
    // A tolerated `FAIL` PLUS a failure the log does NOT name: the summary counts 2 non-passing tests
    // (3 run, 1 passed) but only 1 status line is present, so the accounting is short by one and the
    // shortfall trips. This is the realistic shape of a lost / interleaved / truncated status line under
    // a parallel run — NOT a `LEAK` line, which nextest emits for a test it counts as PASSED (see GB6.6);
    // pinning the tripwire on a leak claimed the opposite of what nextest does.
    // Pre-fix: classified PASS-WITH-TOLERATED (only the tolerated FAIL name was checked). Post-fix: FAIL.
    writeFileSync(
      unaccountedPlusTolerated,
      "        FAIL [   0.204s] verter_protocol::main cases::typeinfo_proto_ts_freshness::typeinfo_ts_bindings_are_byte_equal_to_regenerated_buf_output\n" +
        "     Summary [   1.500s] 3 tests run: 1 passed, 2 failed, 0 skipped\n",
    );
    // A nextest harness/setup error: non-zero exit, NO `FAIL [` line, NO Summary line. Pre-fix: PASS.
    // Post-fix: FAIL (the `code !== 0 && no FAIL name` arm).
    writeFileSync(setupError, "error: creating test list failed\nCaused by: harness error\n");
    // The real tolerated baseline shape (the 2 env FAILs, summary failed=2): still PASS-WITH-TOLERATED.
    writeFileSync(
      tolerated,
      "        FAIL [   0.204s] verter_protocol::main cases::typeinfo_proto_ts_freshness::typeinfo_ts_bindings_are_byte_equal_to_regenerated_buf_output\n" +
        "        FAIL [   0.207s] verter_protocol::main cases::typeinfo_proto_ts_freshness::proto_ts_bindings_byte_pinned_repo_wide\n" +
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
    const cUnacc = classify(unaccountedPlusTolerated);
    const cTol = classify(tolerated);
    note(`classify: sigabrt=${cSig} unaccounted+tol=${cUnacc} tolerated=${cTol}`);
    if (cSig !== "FAIL") {
      fail(
        `(ix) classifier: SIGABRT crash => '${cSig}', expected FAIL (a non-FAIL status must not pass)`,
      );
      ok = false;
    }
    if (cUnacc !== "FAIL") {
      fail(`(ix) classifier: tolerated-FAIL + an unnamed unaccounted failure => '', expected FAIL`);
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
    const rUnacc = classifyRun(100, unaccountedPlusTolerated);
    const rSetup = classifyRun(1, setupError);
    const rTol = classifyRun(100, tolerated);
    note(
      `live-agg: sigabrt=${rSig} unaccounted+tol=${rUnacc} setup-error=${rSetup} tolerated=${rTol}`,
    );
    if (rSig !== "FAIL") {
      fail(`(ix) live-agg: SIGABRT (exit 101) => '${rSig}', expected FAIL`);
      ok = false;
    }
    if (rUnacc !== "FAIL") {
      fail(
        `(ix) live-agg: tolerated-FAIL + an unnamed unaccounted failure (exit 100) => '', expected FAIL`,
      );
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
        "(ix) SURFACE-1: SIGABRT crash, an unnamed unaccounted failure, and a setup/harness error ALL => FAIL on both the " +
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
  {
    const rawWmic = "CreationDate=20260822160000.000000+060";
    const normalizedWmic = normalizeWindowsWmicCreationDate(rawWmic);
    const migration = classifyProcessIdentityComparison("08/22/2026 16:00:00", normalizedWmic);
    const reused = classifyProcessIdentityComparison("win-start-ms:63923007600001", normalizedWmic);
    const same = classifyProcessIdentityComparison("win-start-ms:63923007600000", normalizedWmic);
    const rawWmicSameProcess = classifyProcessIdentityComparison("08/22/2026 16:00:00", rawWmic);
    if (
      normalizedWmic !== "win-start-ms:63923007600000" ||
      migration.comparable ||
      migration.provesReuse ||
      rawWmicSameProcess.comparable ||
      rawWmicSameProcess.provesReuse ||
      !reused.comparable ||
      !reused.provesReuse ||
      !same.matches ||
      same.provesReuse
    ) {
      fail(
        "(x-idfmt) mutex identity migration: a legacy/current-format pair must refuse as " +
          "incomparable, raw WMIC must never prove reuse, and compatible different identities prove " +
          "reuse while equal identities match",
      );
    } else {
      pass(
        "(x-idfmt) MUTEX IDENTITY MIGRATION: a legacy live owner and current-format identity are " +
          "incomparable (never stolen as reused), raw WMIC cannot prove reuse; compatible-format " +
          "difference proves reuse and equality proves the same holder",
      );
    }
  }
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
  // (xi) LEGACY SURFACE-2 ZERO-SUITES / PARTIAL-FILTER CLASSIFIER. This is a retained selftest regression
  //      fixture, not a production gate stage: gate.mjs no longer calls selectSessionSuites(). We drive the
  //      frozen classifier IN-PROCESS. Zero session
  //      suites => 127; a lib-only filter (missing the integration `test` kind) => 127; a proper 1-lib +
  //      N-test listing => OK/0 (discrimination). Pre-fix: runGate had NO zero-suite guard — an empty
  //      filter produced an empty loop and reached the green aggregate verdict.
  // --------------------------------------------------------------------------------------------------
  process.stderr.write("\n(xi) LEGACY SURFACE-2 zero-suites / partial-filter classifier\n");
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
        "(xi) LEGACY SURFACE-2 CLASSIFIER: zero session suites => 127, lib-only (missing test kind) => 127, " +
          "1-lib+2-test => 0 (discriminating retained selftest fixture; not a production gate stage)",
      );
    }
  }

  // --------------------------------------------------------------------------------------------------
  // (xi-b) WINDOWS ARCHIVE DEBUG-SIDECAR COMPLETENESS. cargo-nextest archives the executable test
  //      artifact but currently omits its hashed PDB. The allocation-site audit deliberately verifies
  //      named caller attribution, so the canonical archived surface must restore that sidecar from the
  //      runner-owned build tree before nextest launches the test. This drives the real helper with an
  //      injected filesystem: the required verter_napi PDB replaces an already-present stale extracted
  //      sidecar; a missing source PDB is a loud setup error; non-Windows runs perform no copy.
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
    const isolatedCopied = [];
    const isolatedMkdir = [];
    const isolatedPresent = new Set([source]);
    // `--extract-overwrite` does not remove files omitted by the new archive. Model a destination PDB left
    // behind by an earlier gate run: it must never suppress copying the current matching build sidecar.
    const present = new Set([source, destination]);
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
      // A stale extracted destination must not mask a missing current source.
      existsFn: (path) => path === destination,
      copyFileFn: () => {
        throw new Error("copy must not run without the source PDB");
      },
    });
    const isolated = ensureRequiredWindowsDebugSidecars({
      allSuites: [suite],
      runnerTarget: "C:\\gate\\runner",
      extractDir: "C:\\gate\\extract",
      destinationExtractDir: "C:\\gate\\surface-extract",
      windows: true,
      existsFn: (path) => isolatedPresent.has(path),
      mkdirFn: (path) => isolatedMkdir.push(path),
      copyFileFn: (from, to) => {
        isolatedCopied.push([from, to]);
        isolatedPresent.add(to);
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
      isolated.error ||
      isolated.copied !== 1 ||
      isolatedMkdir[0] !== "C:\\gate\\surface-extract\\target\\debug\\deps" ||
      isolatedCopied[0]?.[0] !== source ||
      isolatedCopied[0]?.[1] !==
        "C:\\gate\\surface-extract\\target\\debug\\deps\\verter_napi-deadbeef.pdb" ||
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
        "(xi-b) WINDOWS archive debug sidecar: required verter_napi PDB replaces a stale extracted " +
          "sidecar, missing source fails setup, and non-Windows execution is a no-op",
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
      "        FAIL [   0.012s] verter_protocol::main cases::typeinfo_proto_ts_freshness::typeinfo_ts_bindings_are_byte_equal_to_regenerated_buf_output\n",
    );
    // The same tolerated FAIL line WITH a matching Summary (failed=1 == 1 parsed FAIL name) => accounted =>
    // PASS-WITH-TOLERATED. Proves the requirement is summary-PRESENCE + exact-count, not a blanket fail.
    writeFileSync(
      tolWithSummary,
      "        FAIL [   0.012s] verter_protocol::main cases::typeinfo_proto_ts_freshness::typeinfo_ts_bindings_are_byte_equal_to_regenerated_buf_output\n" +
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
      // PRECONDITION, not an assertion. This scenario runs the REAL gate against the REAL repo root and
      // expects it to reach cargo. The gate's FIRST step is the build-prerequisite preflight, which exits
      // 127 on a tree whose tsserver plugin cannot be loaded — long before the archive step. On such a
      // tree this scenario would report "the stub was NOT invoked" and "expected 1, got 127", which says
      // nothing about the property under test. Worse, it makes the self-test's own verdict depend on the
      // very tree state the gate is checking for: a green run would only ever be reachable when the
      // artifacts happen to exist. So the state is measured and declared as a TRUE skip (counted in SKIP,
      // never in PASS) rather than silently mis-measured.
      const stubPrereq = checkBuildPrerequisites({ repoRoot: REPO_REALPATH });
      // The SKIP is allowed for exactly ONE cause: the artifacts are demonstrably absent
      // (`module-not-found`). Any OTHER failure class — an EPERM/spawn failure, a probe timeout, the
      // plugin throwing for its own reasons, an unreadable tsserver launcher — means the prerequisite
      // could not be ANSWERED, not that it is missing, and skipping on those would green-skip a scenario
      // whose artifacts are present: a narrower version of the very silent pass this precondition was
      // added to remove. `finish()` exits 0 while FAIL is zero, so a wrong SKIP here is invisible.
      if (!stubPrereq.ok && stubPrereq.reason === "module-not-found") {
        skip(
          "(xix) STUB-INVOKED — SKIPPED: this tree's build prerequisites are absent, so the real gate " +
            `exits 127 at its build-prerequisite preflight before reaching cargo (${stubPrereq.detail.split("\n")[0]}). ` +
            `Build them with \`${BUILD_PREREQUISITE_COMMAND}\` and re-run to exercise this scenario.`,
        );
        break posix_xix;
      }
      if (!stubPrereq.ok) {
        fail(
          `(xix) the build-prerequisite probe could not ANSWER (reason=${stubPrereq.reason}): ` +
            `${stubPrereq.detail.split("\n")[0]}. That is an infrastructure failure, not a missing build, ` +
            "so this scenario must FAIL rather than skip — a skip here would hide a scenario whose " +
            "artifacts are present.",
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
  //          * `--prepare --exhaustive` => 127 (a GATE-ONLY flag after --prepare is rejected — pre-fix it
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
        argv: ["--prepare", "--exhaustive"],
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
          "--prepare --selftest-x / --prepare --exhaustive each => 127 — the exit-0 non-gate modes are " +
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
  // (xviii) LEGACY SURFACE-2 CRASHED/ABNORMAL LIBTEST CLASSIFIER. This retained selftest-only fixture proves
  //         the retired direct-libtest classifier remains fail-closed; production gate.mjs does not call it.
  //         A direct-libtest failure is tolerated ONLY
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
  process.stderr.write("\n(xviii) LEGACY SURFACE-2 crashed/abnormal libtest classifier\n");
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
        "(xviii) LEGACY SURFACE-2 CRASH CLASSIFIER: SIGABRT(134), exit-101-without-summary, and summary-count-mismatch ALL " +
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
  //      the legacy selftest-only `analyzeLibtestSurface(text, code, binaryId)` had NO tolerance gate and
  //      ALWAYS returned `tolerated` for the pair — so "pair + tolerance-disabled => FAIL" cannot pass
  //      against today's retained classifier.
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
    const nxPairLog = `        FAIL [   0.012s] ${BIN} ${NX_NAME}\n`;
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
      `        FAIL [   0.012s] ${BIN} ${NX_NAME}\n` +
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
      `     SIGABRT [   0.204s] ${BIN} ${NX_NAME}\n` +
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
    const nxReal = `        FAIL [   0.030s] verter_compiler::main template::vmemo::renders_cached\n`;
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
  // (GB6) TERMINAL-OUTCOME ACCOUNTING — a nextest run is accounted for by its SUMMARY, not by its `FAIL [`
  //       lines. nextest reports several terminal outcomes that are NOT `FAIL`: `N timed out`, `N exec
  //       failed`, and an interrupted/cancelled run's `A/B tests run` (B-A tests that never ran at all). A
  //       timed-out test has not passed — it has not even finished — so it MUST count toward the verdict and
  //       MUST be NAMED with the same visibility as an ordinary failure. Pre-fix the analyzer keyed on
  //       `summary.failed === parsedFailNames.length`, and nextest's `failed` count EXCLUDES `timed out` /
  //       `exec failed`; so a run whose ONLY problem was a timeout (or whose plain failures were all
  //       allowlisted) reported PASS / PASS-WITH-TOLERATED with ZERO named failures, and a run with real
  //       failures PLUS timeouts named only the failures. The accounting below derives the failure total
  //       from `runCount - passed`, which is label-INDEPENDENT (a future nextest outcome nextest counts as
  //       run-but-not-passed is caught without this parser knowing its name).
  //       DISCRIMINATION: GB6.6 is the inverse control — a `LEAK` line marks a test nextest counts as
  //       PASSED (leaky, not fatal, outside leak-fail-mode), so a green run with a leak must stay PASS. A
  //       change that simply failed on every non-`FAIL` status line would fail GB6.6.
  // --------------------------------------------------------------------------------------------------
  process.stderr.write(
    "\n(GB6) TERMINAL-OUTCOME ACCOUNTING (timed out / exec failed / never ran)\n",
  );
  {
    // The EXACT allowlisted freshness-pair name (the only tolerated name) — reused so the tolerance path is
    // genuinely reachable in the scenarios that must NOT be tolerated for an unrelated reason.
    const TOL =
      "cases::typeinfo_proto_ts_freshness::typeinfo_ts_bindings_are_byte_equal_to_regenerated_buf_output";
    const T1 = "cases::g_compile::compile_fail::hot_materialize_structural_rails_smoke";
    const T2 =
      "cases::tracked_paths_no_machine_roots::tracked_files_contain_no_machine_specific_path_markers";
    const names = (r) => r.failures.map((f) => `${f.surface}|${f.name}`).join("\n");
    let ok = true;

    // GB6.1 — REAL-RUN SHAPE: plain failures AND timeouts. Every terminal failure must be NAMED.
    //   Pre-fix: failures.length === 2 (the two `FAIL` names only) — the timeouts were invisible.
    const mixed =
      "        FAIL [   0.204s] ( 1/12) verter_session::main cases::a::alpha\n" +
      "        FAIL [   0.207s] ( 2/12) verter_session::main cases::b::beta\n" +
      `     TIMEOUT [ 180.008s] ( 3/12) verter_session::main ${T1}\n` +
      `     TIMEOUT [ 180.005s] ( 4/12) verter_session::main ${T2}\n` +
      "     Summary [ 900.014s] 12 tests run: 8 passed, 2 failed, 2 timed out, 5 skipped\n";
    const rMixed = analyzeNextestSurface(mixed, 100, true);
    note(`GB6.1 mixed failures=${rMixed.failures.length}\n${names(rMixed)}`);
    if (rMixed.failures.length !== 4) {
      fail(
        `(GB6.1) 2 FAIL + 2 TIMEOUT => ${rMixed.failures.length} named failure(s), expected 4 (a timed-out ` +
          `test has not passed; it must be counted AND named)`,
      );
      ok = false;
    }
    for (const t of [T1, T2]) {
      if (!rMixed.failures.some((f) => f.name === t)) {
        fail(`(GB6.1) timed-out test '${t}' is NOT in the named failure list: ${names(rMixed)}`);
        ok = false;
      }
    }
    if (!rMixed.failures.some((f) => /TIMEOUT/.test(f.surface) && f.name === T1)) {
      fail(
        `(GB6.1) the timed-out entry does not carry a TIMEOUT-tagged surface (the verdict line must say ` +
          `WHY it failed): ${names(rMixed)}`,
      );
      ok = false;
    }

    // GB6.2 — THE SILENT PASS (headline). Every plain `FAIL` is allowlisted AND a test timed out. Pre-fix
    //   the summary `failed` count (1) matched the one parsed `FAIL` name, so the run was "accounted for"
    //   and the timeout never entered the verdict => PASS-WITH-TOLERATED with a test that never finished.
    const tolPlusTimeout =
      `        FAIL [   0.204s] ( 1/12) verter_protocol::main ${TOL}\n` +
      `     TIMEOUT [ 180.008s] ( 2/12) verter_session::main ${T1}\n` +
      "     Summary [ 900.014s] 12 tests run: 10 passed, 1 failed, 1 timed out, 5 skipped\n";
    const vTolTimeout = verdictNextestRun(100, tolPlusTimeout, true);
    const rTolTimeout = analyzeNextestSurface(tolPlusTimeout, 100, true);
    note(`GB6.2 tolerated-FAIL + TIMEOUT => ${vTolTimeout}`);
    if (vTolTimeout !== "FAIL") {
      fail(
        `(GB6.2) an allowlisted FAIL plus a TIMEOUT => '${vTolTimeout}', expected FAIL — a timeout is NEVER ` +
          `tolerated, and a run whose only problem is a timeout must not certify the tree`,
      );
      ok = false;
    }
    if (!rTolTimeout.failures.some((f) => f.name === T1)) {
      fail(`(GB6.2) the timed-out test is not named in the verdict: ${names(rTolTimeout)}`);
      ok = false;
    }

    // GB6.3 — TIMEOUT ONLY, no `FAIL [` line at all. Pre-fix this did fail, but ONLY through the opaque
    //   `<run exit …; unaccounted failure(s)>` catch-all — the operator was never told WHICH test hung.
    const timeoutOnly =
      `     TIMEOUT [ 180.002s] ( 4/4) verter_session::main ${T2}\n` +
      "     Summary [ 900.003s] 4 tests run: 3 passed, 1 timed out, 2 skipped\n";
    const rTimeoutOnly = analyzeNextestSurface(timeoutOnly, 100, true);
    const vTimeoutOnly = verdictNextestRun(100, timeoutOnly, true);
    note(`GB6.3 timeout-only => ${vTimeoutOnly}\n${names(rTimeoutOnly)}`);
    if (vTimeoutOnly !== "FAIL") {
      fail(`(GB6.3) a timeout-only run => '${vTimeoutOnly}', expected FAIL`);
      ok = false;
    }
    if (!rTimeoutOnly.failures.some((f) => f.name === T2)) {
      fail(
        `(GB6.3) a timeout-only run must NAME the timed-out test, not only report an opaque unaccounted ` +
          `catch-all: ${names(rTimeoutOnly)}`,
      );
      ok = false;
    }

    // GB6.4 — `N exec failed` is a terminal outcome nextest reports SEPARATELY from `N failed`; pre-fix the
    //   `(\d+)\s+failed` scan never saw it, so a tolerated FAIL alongside an exec-failed test passed.
    const execFailed =
      `        FAIL [   0.204s] ( 1/6) verter_protocol::main ${TOL}\n` +
      "     Summary [  12.000s] 6 tests run: 4 passed, 1 failed, 1 exec failed, 0 skipped\n";
    const vExec = verdictNextestRun(100, execFailed, true);
    note(`GB6.4 tolerated-FAIL + exec-failed => ${vExec}`);
    if (vExec !== "FAIL") {
      fail(
        `(GB6.4) an allowlisted FAIL plus an 'exec failed' test => '${vExec}', expected FAIL (the exec-failed ` +
          `test is unaccounted for by the parsed FAIL names)`,
      );
      ok = false;
    }

    // GB6.5 — INTERRUPTED/CANCELLED run: nextest's `A/B tests run` form means B-A tests NEVER RAN. Pre-fix
    //   the parser ignored the `A/B` form entirely, so a cancelled run with one tolerated failure certified
    //   a tree where 39 of 41 tests never executed.
    const cancelled =
      `        FAIL [   0.009s] ( 1/41) verter_protocol::main ${TOL}\n` +
      "  Cancelling due to test failure: 1 test still running\n" +
      "     Summary [   1.516s] 2/41 tests run: 1 passed, 1 failed, 0 skipped\n";
    const vCancelled = verdictNextestRun(100, cancelled, true);
    const rCancelled = analyzeNextestSurface(cancelled, 100, true);
    note(`GB6.5 cancelled 2/41 => ${vCancelled}\n${names(rCancelled)}`);
    if (vCancelled !== "FAIL") {
      fail(
        `(GB6.5) a cancelled run (2 of 41 tests run) with only an allowlisted failure => '${vCancelled}', ` +
          `expected FAIL — 39 tests never ran, so the run cannot certify the tree`,
      );
      ok = false;
    }
    if (!rCancelled.failures.some((f) => /never ran|39/.test(f.name))) {
      fail(`(GB6.5) the unrun-test count is not surfaced in the verdict: ${names(rCancelled)}`);
      ok = false;
    }

    // GB6.6 — INVERSE CONTROL (false-positive guard). `LEAK` is the terminal status of a test nextest counts
    //   as PASSED (it leaked a handle/subprocess; fatal only under leak-fail-mode, which renders `LEAK-FAIL`).
    //   A green run containing a LEAK line must stay PASS on BOTH the classifier and the live analyzer.
    //   This is what stops the fix from degenerating into "any non-FAIL status line fails the gate".
    const leakyGreen =
      "        PASS [   0.013s] ( 1/2) verter_session::main cases::ok::fine\n" +
      "        LEAK [   0.215s] ( 2/2) verter_session::main cases::ok::leaks_a_child\n" +
      "     Summary [   1.003s] 2 tests run: 2 passed (1 leaky), 0 skipped\n";
    const cLeaky = verdictClassifyNextest(leakyGreen, true);
    const rLeaky = verdictNextestRun(0, leakyGreen, true);
    note(`GB6.6 leaky-but-green: classifier=${cLeaky} live-agg=${rLeaky}`);
    if (cLeaky !== "PASS" || rLeaky !== "PASS") {
      fail(
        `(GB6.6) a GREEN run with a leaky (passed) test => classifier='${cLeaky}' live-agg='${rLeaky}', ` +
          `expected PASS on both — LEAK marks a test nextest counted as PASSED; only LEAK-FAIL is a failure`,
      );
      ok = false;
    }
    // …and LEAK-FAIL (leak-fail-mode) IS a failure, named.
    const leakFail =
      "   LEAK-FAIL [   0.215s] ( 2/2) verter_session::main cases::ok::leaks_a_child\n" +
      "     Summary [   1.003s] 2 tests run: 1 passed, 1 failed, 0 skipped\n";
    const rLeakFail = analyzeNextestSurface(leakFail, 100, true);
    if (!rLeakFail.failures.some((f) => f.name === "cases::ok::leaks_a_child")) {
      fail(`(GB6.6) a LEAK-FAIL test must be named as a failure: ${names(rLeakFail)}`);
      ok = false;
    }

    // GB6.7 — CLEAN CONTROL: a fully green run stays PASS (the accounting must not blanket-fail).
    const green =
      "        PASS [   0.013s] ( 1/2) verter_session::main cases::ok::fine\n" +
      "     Summary [  63.890s] 15543 tests run: 15543 passed, 547 skipped\n";
    const vGreen = verdictNextestRun(0, green, true);
    if (vGreen !== "PASS") {
      fail(`(GB6.7) a fully green run => '${vGreen}', expected PASS`);
      ok = false;
    }

    if (ok) {
      pass(
        "(GB6) TERMINAL-OUTCOME ACCOUNTING: timed-out and exec-failed tests are COUNTED and NAMED (a " +
          "timeout is never tolerated, even alongside an allowlisted FAIL), an interrupted run's never-ran " +
          "tests fail the verdict, and the inverse controls hold — a leaky-but-PASSED test and a fully " +
          "green run both stay PASS (discriminating: pre-fix the analyzer keyed on nextest's `failed` " +
          "count, which excludes `timed out`/`exec failed`, so a timeout-only problem reported PASS)",
      );
    }
  }

  // --------------------------------------------------------------------------------------------------
  // (GB6b) COMPOUND AND RETRY STATUS FIELDS. The (GB6) accounting above is only as good as its NAMING: a
  //        terminal failure the status parser cannot read still fails the gate (the derived count catches
  //        it) but surfaces as an opaque `<unaccounted>` line instead of the test's name, which is exactly
  //        what (GB6) set out to end. nextest's status field is NOT a single uppercase token. Every fixture
  //        below is a VERBATIM line captured from a real `cargo-nextest 0.9.130` run, not a guess:
  //
  //          " FAIL + LEAK [   1.019s] (4/5) gb6status::t leak_and_fail"   — a test that failed AND leaked
  //          "  TRY 3 FAIL [   0.008s] (2/3) gb6status::t always_fails"    — final attempt under `retries`
  //          " TRY 3 FL+LK [   1.028s] (3/3) gb6status::t leak_and_fail"   — abbreviated compound + retry
  //
  //        A `^([A-Z][A-Z-]*) \[` scan reads NONE of them: `FAIL` is followed by " + LEAK [", and a `TRY N`
  //        prefix pushes the real status off the line start.
  //
  //        DISCRIMINATION — and the reason this cannot be fixed by broad substring matching. A FLAKY test
  //        renders `TRY 1 FAIL` and then `TRY 2 PASS`, is counted `1 passed (1 flaky)`, and the run EXITS
  //        0. Any parser that names every line containing "FAIL" reddens that GREEN run. GB6b.4 is that
  //        control, and GB6b.5 pins the count: intermediate attempts must not each become a failure. The
  //        terminal status per test is what counts, so the parser keeps the LAST status line per test.
  // --------------------------------------------------------------------------------------------------
  process.stderr.write("\n(GB6b) COMPOUND + RETRY STATUS FIELDS (FAIL + LEAK, TRY N …)\n");
  {
    const names = (r) => r.failures.map((f) => `${f.surface}|${f.name}`).join("\n");
    let ok = true;

    // GB6b.1 — compound `FAIL + LEAK`: the failing test must be NAMED, not folded into `<unaccounted>`.
    const compound =
      "        PASS [   0.015s] (1/5) gb6status::t plain_pass\n" +
      "        FAIL [   0.016s] (2/5) gb6status::t always_fails_for_retry\n" +
      "        LEAK [   1.019s] (3/5) gb6status::t leak_and_pass\n" +
      " FAIL + LEAK [   1.019s] (4/5) gb6status::t leak_and_fail\n" +
      "     TIMEOUT [   2.004s] (5/5) gb6status::t hangs\n" +
      "     Summary [   2.004s] 5 tests run: 2 passed (1 leaky), 2 failed, 1 timed out, 0 skipped\n" +
      "        FAIL [   0.016s] (2/5) gb6status::t always_fails_for_retry\n" +
      " FAIL + LEAK [   1.019s] (4/5) gb6status::t leak_and_fail\n" +
      "     TIMEOUT [   2.004s] (5/5) gb6status::t hangs\n";
    const rCompound = analyzeNextestSurface(compound, 100, true);
    note(`GB6b.1 compound failures=${rCompound.failures.length}\n${names(rCompound)}`);
    // 5 run - 2 passed = 3 non-passing: the plain FAIL, the FAIL+LEAK, and the TIMEOUT. All three named,
    // and NOTHING else (the leaky-but-PASSED test is not a failure, and no `<unaccounted>` filler).
    if (rCompound.failures.length !== 3) {
      fail(
        `(GB6b.1) FAIL + (FAIL + LEAK) + TIMEOUT => ${rCompound.failures.length} named, expected exactly 3`,
      );
      ok = false;
    }
    if (!rCompound.failures.some((f) => f.name === "leak_and_fail")) {
      fail(`(GB6b.1) the compound 'FAIL + LEAK' test is not named: ${names(rCompound)}`);
      ok = false;
    }
    if (rCompound.failures.some((f) => f.name === "leak_and_pass")) {
      fail(`(GB6b.1) a leaky-but-PASSED test was named as a failure: ${names(rCompound)}`);
      ok = false;
    }
    if (rCompound.failures.some((f) => /unaccounted/.test(f.name))) {
      fail(
        `(GB6b.1) every failure was nameable, so no opaque <unaccounted> entry may appear: ${names(rCompound)}`,
      );
      ok = false;
    }

    // GB6b.2/3 — `TRY N FAIL` and `TRY N FL+LK` (the `retries` profile). Only the TERMINAL attempt counts.
    const retried =
      "        PASS [   0.011s] (1/3) gb6status::t plain_pass\n" +
      "  TRY 1 FAIL [   0.014s] (───) gb6status::t always_fails_for_retry\n" +
      "  TRY 2 FAIL [   0.011s] (───) gb6status::t always_fails_for_retry\n" +
      "  TRY 3 FAIL [   0.008s] (2/3) gb6status::t always_fails_for_retry\n" +
      " TRY 1 FL+LK [   1.032s] (───) gb6status::t leak_and_fail\n" +
      " TRY 2 FL+LK [   1.024s] (───) gb6status::t leak_and_fail\n" +
      " TRY 3 FL+LK [   1.028s] (3/3) gb6status::t leak_and_fail\n" +
      "     Summary [   3.090s] 3 tests run: 1 passed, 2 failed, 2 skipped\n" +
      "  TRY 3 FAIL [   0.008s] (2/3) gb6status::t always_fails_for_retry\n" +
      " TRY 3 FL+LK [   1.028s] (3/3) gb6status::t leak_and_fail\n";
    const rRetried = analyzeNextestSurface(retried, 100, true);
    note(`GB6b.2/3 retried failures=${rRetried.failures.length}\n${names(rRetried)}`);
    for (const t of ["always_fails_for_retry", "leak_and_fail"]) {
      if (!rRetried.failures.some((f) => f.name === t)) {
        fail(`(GB6b.2/3) retried failure '${t}' is not named: ${names(rRetried)}`);
        ok = false;
      }
    }
    // GB6b.5 — COUNT PIN: 3 run - 1 passed = 2 non-passing, so EXACTLY 2 named. Six `TRY` lines plus two
    // recap lines must not inflate this — a per-line counter would report 8.
    if (rRetried.failures.length !== 2) {
      fail(
        `(GB6b.5) six TRY attempts over two tests => ${rRetried.failures.length} named, expected exactly 2 ` +
          `(the terminal attempt per test, not one failure per attempt line)`,
      );
      ok = false;
    }

    // GB6b.4 — THE CONTROL. A flaky test fails its first attempt, PASSES its second, is counted
    // `1 passed (1 flaky)`, and the run EXITS 0. It must stay PASS: naming any line containing "FAIL"
    // would redden this green run.
    const flakyGreen =
      "  TRY 1 FAIL [   0.022s] (───) gb6status::flaky flaky_passes_on_retry\n" +
      "  TRY 2 PASS [   0.019s] (1/1) gb6status::flaky flaky_passes_on_retry\n" +
      "     Summary [   0.049s] 1 test run: 1 passed (1 flaky), 5 skipped\n";
    const vFlaky = verdictNextestRun(0, flakyGreen, true);
    const cFlaky = verdictClassifyNextest(flakyGreen, true);
    note(`GB6b.4 flaky-pass: live-agg=${vFlaky} classifier=${cFlaky}`);
    if (vFlaky !== "PASS" || cFlaky !== "PASS") {
      fail(
        `(GB6b.4) a FLAKY test that failed attempt 1 and PASSED attempt 2 (exit 0, '1 passed (1 flaky)') ` +
          `=> live-agg='${vFlaky}' classifier='${cFlaky}', expected PASS on both — the terminal status is ` +
          `PASS, and a parser that matches any line containing "FAIL" reddens this green run`,
      );
      ok = false;
    }

    if (ok) {
      pass(
        "(GB6b) COMPOUND + RETRY STATUS FIELDS: a compound `FAIL + LEAK` and a retried `TRY N FAIL` / " +
          "`TRY N FL+LK` are each NAMED (not folded into an opaque <unaccounted> entry), the named count " +
          "equals the summary's non-passing count so retry attempts do not inflate it, and the inverse " +
          "controls hold — a leaky-but-PASSED test and a FLAKY test that passed on retry both stay PASS " +
          "(discriminating: a `^([A-Z][A-Z-]*) \\[` scan reads none of these status fields, and broad " +
          "substring matching reddens the green flaky run)",
      );
    }
  }

  // --------------------------------------------------------------------------------------------------
  // (GB6c) STATUS-LINE IMPERSONATION. The parser reads a channel that is NOT exclusively the runner's:
  //        nextest relays a test's CAPTURED OUTPUT on the same stream as its own status lines. So any
  //        parse that trusts line SHAPE alone is parsing data a test can influence — and the "attacker"
  //        need only be a test that legitimately prints nextest-shaped text, or a fixture containing one.
  //
  //        THE ATTACK, reproduced against the pre-fix parser: because naming resolved last-status-wins
  //        UNCONDITIONALLY, a captured `PASS` line naming a test that genuinely FAILED overwrote the real
  //        FAIL. A second captured line naming the ALLOWLISTED test then rebalanced the arithmetic
  //        (namedCount=1, nonPassed=1, no shortfall), so the run reported PASS-WITH-TOLERATED while a real
  //        failure sat in the log. Clearing the named set alone does not do this — the count still trips;
  //        it is BALANCING the count that produces the green verdict.
  //
  //        THE LOAD-BEARING LAYER IS THE COUNT, not the transition rule. `nonPassed` comes from nextest's
  //        own accounting and cannot be lowered by anything printed into the stream, so clearing a failure
  //        leaves a shortfall that fails the run. The transition rule below only raises the cost of the
  //        BARE-`PASS` forgery; a forged `TRY n` pair defeats it outright (see GB6d), because captured
  //        output can supply both sides of the one transition it permits. Two further layers: a named
  //        count EXCEEDING nextest's non-passing count is surfaced as impersonation; and a status line
  //        must occupy nextest's exact 12-column status field (verified 44/44 against real runs), which
  //        the 4-space capture indent breaks for echoed output. The tolerance path - the only route from
  //        `failures exist` to green - is closed separately in GB6d.
  // --------------------------------------------------------------------------------------------------
  process.stderr.write(
    "\n(GB6c) STATUS-LINE IMPERSONATION (captured output cannot clear a failure)\n",
  );
  {
    const TOL =
      "cases::typeinfo_proto_ts_freshness::typeinfo_ts_bindings_are_byte_equal_to_regenerated_buf_output";
    const REAL = "cases::real::genuine_failure";
    const names = (r) => r.failures.map((f) => `${f.surface}|${f.name}`).join("  ");
    let ok = true;

    // GB6c.1 — THE S0. The captured `PASS` is COLUMN-EXACT (8-space pad, the genuine 12-column field), so
    // the layout check cannot save us here; only the transition rule can. Pre-fix: PASS-WITH-TOLERATED.
    const impersonated =
      `        FAIL [   0.204s] ( 1/12) verter_session::main ${REAL}\n` +
      "  stdout ───\n" +
      `        PASS [   0.010s] ( 1/12) verter_session::main ${REAL}\n` +
      `        FAIL [   0.011s] ( 2/12) verter_protocol::main ${TOL}\n` +
      "     Summary [ 900.014s] 12 tests run: 11 passed, 1 failed, 0 skipped\n";
    const vImp = verdictNextestRun(100, impersonated, true);
    const rImp = analyzeNextestSurface(impersonated, 100, true);
    note(`GB6c.1 impersonated-PASS => ${vImp} | ${names(rImp)}`);
    if (vImp !== "FAIL") {
      fail(
        `(GB6c.1) a CAPTURED 'PASS' line naming a genuinely FAILED test, plus a captured line naming the ` +
          `allowlisted test to balance the count => '${vImp}', expected FAIL — captured test output must ` +
          `never be able to clear a real failure`,
      );
      ok = false;
    }
    if (!rImp.failures.some((f) => f.name === REAL)) {
      fail(`(GB6c.1) the genuinely failed test is not named in the verdict: ${names(rImp)}`);
      ok = false;
    }

    // GB6c.2 — the other half: a captured line ADDING a failure nextest never counted. The named count
    // exceeds the summary's non-passing count, which is only possible if something impersonated a status
    // line, so it must be surfaced rather than silently ignored.
    const overCount =
      "        FAIL [   0.204s] ( 1/12) verter_session::main cases::real::one_true_failure\n" +
      "  stdout ───\n" +
      "        FAIL [   0.010s] ( 2/12) verter_session::main cases::fake::invented_by_stdout\n" +
      "     Summary [ 900.014s] 12 tests run: 11 passed, 1 failed, 0 skipped\n";
    const rOver = analyzeNextestSurface(overCount, 100, true);
    note(`GB6c.2 over-count named=${rOver.namedCount} nonPassed=${rOver.summary.nonPassed}`);
    if (
      !rOver.failures.some((f) => /nextest counted \d+ non-passing/.test(f.name)) ||
      !rOver.failures.some((f) => f.name === "cases::real::one_true_failure")
    ) {
      fail(
        `(GB6c.2) the log named ${rOver.namedCount} failures but nextest counted ` +
          `${rOver.summary.nonPassed} non-passing — that mismatch must be surfaced: ${names(rOver)}`,
      );
      ok = false;
    }

    // GB6c.3 — CONTROL: the genuine retry sequence must survive the transition rule. A blanket
    // "a PASS may never clear a FAIL" rule would break exactly this, which is why the rule is scoped to
    // TRY-tagged, strictly-increasing attempts.
    const genuineRetry =
      "  TRY 1 FAIL [   0.022s] (───) gb6status::flaky flaky_passes_on_retry\n" +
      "  TRY 2 PASS [   0.019s] (1/1) gb6status::flaky flaky_passes_on_retry\n" +
      "     Summary [   0.049s] 1 test run: 1 passed (1 flaky), 5 skipped\n";
    const vRetry = verdictNextestRun(0, genuineRetry, true);
    if (vRetry !== "PASS") {
      fail(
        `(GB6c.3) a GENUINE retry (TRY 1 FAIL -> TRY 2 PASS, exit 0, '1 passed (1 flaky)') => '${vRetry}', ` +
          `expected PASS — the transition rule must permit the one legitimate fail-to-pass transition`,
      );
      ok = false;
    }

    // GB6c.4 — CONTROL: a SLOW progress line followed by a real PASS is an ordinary green test.
    const slowThenPass =
      "        SLOW [> 60.000s] (───) verter_session::main cases::slow::big_scan\n" +
      "        PASS [  64.584s] (2/2) verter_session::main cases::slow::big_scan\n" +
      "     Summary [  64.587s] 2 tests run: 2 passed, 0 skipped\n";
    const vSlow = verdictNextestRun(0, slowThenPass, true);
    if (vSlow !== "PASS") {
      fail(`(GB6c.4) SLOW followed by PASS => '${vSlow}', expected PASS`);
      ok = false;
    }

    // GB6c.5 — LAYOUT LAYER. The realistic accidental case: a test echoes a genuine-looking status line,
    // and nextest indents captured output by 4 spaces, pushing the status field off its 12-column slot.
    // Such a line must not parse as a status line AT ALL, so it neither clears nor invents a failure.
    const indentedEcho =
      `        FAIL [   0.204s] ( 1/12) verter_session::main ${REAL}\n` +
      "  stdout ───\n" +
      `            PASS [   0.010s] ( 1/12) verter_session::main ${REAL}\n` +
      "     Summary [ 900.014s] 12 tests run: 11 passed, 1 failed, 0 skipped\n";
    const rEcho = analyzeNextestSurface(indentedEcho, 100, true);
    note(`GB6c.5 indented-echo named=${rEcho.namedCount} | ${names(rEcho)}`);
    if (rEcho.failures.length !== 1 || rEcho.failures[0].name !== REAL) {
      fail(
        `(GB6c.5) a 4-space-indented captured echo must not parse as a status line; expected exactly the ` +
          `one real failure, got: ${names(rEcho)}`,
      );
      ok = false;
    }

    // GB6c.6 — [MINOR] identical test NAMES in two different binaries are two distinct tests. Collapsing
    // them by bare name loses one and leaves an opaque unaccounted entry in its place.
    const twoBinaries =
      "        FAIL [   0.204s] ( 1/12) verter_session::main cases::shared::same_name\n" +
      "        FAIL [   0.207s] ( 2/12) verter_protocol::main cases::shared::same_name\n" +
      "     Summary [ 900.014s] 12 tests run: 10 passed, 2 failed, 0 skipped\n";
    const rTwo = analyzeNextestSurface(twoBinaries, 100, true);
    note(`GB6c.6 two-binaries named=${rTwo.namedCount} failures=${rTwo.failures.length}`);
    if (rTwo.failures.length !== 2 || rTwo.failures.some((f) => /unaccounted/.test(f.name))) {
      fail(
        `(GB6c.6) the same test name in TWO binaries is two failures; expected 2 named and no opaque ` +
          `unaccounted entry, got: ${names(rTwo)}`,
      );
      ok = false;
    }

    // GB6c.7 — [ENV] the gate must not let an inherited env var disable the signal its parser depends on.
    // `NEXTEST_FINAL_STATUS_LEVEL=none` suppresses the failure recap; a gate whose correctness silently
    // depends on that being unset is one `export` away from certifying a broken tree. buildCargoEnv must
    // OVERRIDE it to a known value, exactly as it already does for CARGO_TARGET_DIR.
    const hostileEnv = buildCargoEnv(
      { PATH: "/usr/bin", NEXTEST_FINAL_STATUS_LEVEL: "none", NEXTEST_STATUS_LEVEL: "none" },
      "/tmp/runner-target",
      false,
    );
    note(
      `GB6c.7 env: final=${hostileEnv.NEXTEST_FINAL_STATUS_LEVEL} status=${hostileEnv.NEXTEST_STATUS_LEVEL}`,
    );
    if (hostileEnv.NEXTEST_FINAL_STATUS_LEVEL === "none") {
      fail(
        `(GB6c.7) an inherited NEXTEST_FINAL_STATUS_LEVEL=none survived buildCargoEnv — it suppresses the ` +
          `failure recap the parser reads, so the gate's correctness would depend on an unset env var`,
      );
      ok = false;
    }
    if (hostileEnv.NEXTEST_STATUS_LEVEL === "none") {
      fail(`(GB6c.7) an inherited NEXTEST_STATUS_LEVEL=none survived buildCargoEnv`);
      ok = false;
    }

    if (ok) {
      pass(
        "(GB6c) STATUS-LINE IMPERSONATION: a captured `PASS` cannot clear a genuine FAIL (a fail-to-pass " +
          "transition requires a TRY-tagged, strictly-increasing retry), a named count exceeding nextest's " +
          "own non-passing count is surfaced, an indented captured echo does not parse as a status line, " +
          "one test name in two binaries stays two failures, and buildCargoEnv overrides a hostile " +
          "NEXTEST_*_STATUS_LEVEL — while the genuine retry and SLOW-then-PASS controls stay PASS",
      );
    }
  }

  // --------------------------------------------------------------------------------------------------
  // (GB6d) FORGED RETRY PAIRS, AND WHICH LAYER ACTUALLY BEARS THE LOAD.
  //
  //        The (GB6c) transition rule permits ONE fail-to-pass transition: a `TRY <n>`-tagged pair with a
  //        strictly increasing attempt number. Captured output can supply BOTH SIDES of that pair, so the
  //        rule's defining case is trivially forgeable and the rule is NOT the load-bearing layer. Two
  //        reviewer-executed payloads, both of which cleared a genuine failure and reported
  //        PASS-WITH-TOLERATED:
  //
  //          FAIL … genuine_failure / TRY 1 FAIL … genuine_failure / TRY 2 PASS … genuine_failure
  //          FAIL … genuine_failure / TRY 1 FL   … genuine_failure / TRY 2 LK   … genuine_failure
  //
  //        WHAT ACTUALLY BEARS THE LOAD is the COUNT reconciliation: `nonPassed` comes from nextest's own
  //        accounting and cannot be lowered by anything printed into the stream. Clearing a failure creates
  //        a shortfall, which fails the run — UNLESS the forger also supplies a replacement name to balance
  //        the count. And a replacement only produces a GREEN verdict if it is ALLOWLISTED, because any
  //        other name is itself a named failure. So the entire residual attack surface is the tolerance
  //        path, and that is where the fail-closed check belongs: tolerance is refused whenever any test's
  //        failure was superseded by a pass, because the gate cannot prove whether that supersession was a
  //        genuine retry or a forgery. Refusing costs nothing real (this repo runs `retries = 0`, so no
  //        genuine supersession occurs) and it closes both payloads.
  //
  //        GB6d.3 pins the LAYER ATTRIBUTION rather than asserting it in prose: the same forged pair is
  //        stopped by tolerance-refusal when column-exact, and by the layout rule when carrying nextest's
  //        4-space capture indent. GB6d.4/5 are the controls that keep the fix from being a blanket denial.
  // --------------------------------------------------------------------------------------------------
  process.stderr.write("\n(GB6d) FORGED RETRY PAIRS (layer attribution)\n");
  {
    const TOL =
      "cases::typeinfo_proto_ts_freshness::typeinfo_ts_bindings_are_byte_equal_to_regenerated_buf_output";
    const REAL = "cases::real::genuine_failure";
    const SUM = "     Summary [ 900.014s] 12 tests run: 11 passed, 1 failed, 0 skipped\n";
    const realFail = `        FAIL [   0.204s] ( 1/12) verter_session::main ${REAL}\n`;
    const tolFail = `        FAIL [   0.011s] ( 2/12) verter_protocol::main ${TOL}\n`;
    const names = (r) => r.failures.map((f) => `${f.surface}|${f.name}`).join("  ");
    let ok = true;

    // GB6d.1 / GB6d.2 — both reviewer payloads. Column-exact, so layout cannot stop them.
    const forgedPass =
      realFail +
      `  TRY 1 FAIL [   0.010s] (───) verter_session::main ${REAL}\n` +
      `  TRY 2 PASS [   0.011s] (1/1) verter_session::main ${REAL}\n` +
      tolFail +
      SUM;
    const forgedAbbrev =
      realFail +
      `    TRY 1 FL [   0.010s] (───) verter_session::main ${REAL}\n` +
      `    TRY 2 LK [   0.011s] (1/1) verter_session::main ${REAL}\n` +
      tolFail +
      SUM;
    for (const [label, log] of [
      ["GB6d.1 forged TRY n PASS", forgedPass],
      ["GB6d.2 forged TRY n FL/LK", forgedAbbrev],
    ]) {
      const v = verdictNextestRun(100, log, true);
      const r = analyzeNextestSurface(log, 100, true);
      note(`${label} => ${v} | ${names(r)}`);
      if (v !== "FAIL") {
        fail(
          `(${label}) a forged retry pair supplying BOTH sides cleared a genuine failure => '${v}', ` +
            `expected FAIL — captured output can fabricate the one fail-to-pass transition the ` +
            `transition rule permits, so tolerance must refuse when any failure was superseded by a pass`,
        );
        ok = false;
      }
    }

    // GB6d.3 — LAYER ATTRIBUTION. Same forged pair, but carrying nextest's real 4-space capture indent:
    // the layout rule rejects the lines outright, so the genuine FAIL is never cleared and the run fails
    // by NAME rather than by tolerance-refusal. This is what makes the two layers separately observable.
    const forgedIndented =
      realFail +
      `      TRY 1 FAIL [   0.010s] (───) verter_session::main ${REAL}\n` +
      `      TRY 2 PASS [   0.011s] (1/1) verter_session::main ${REAL}\n` +
      SUM;
    const rIndent = analyzeNextestSurface(forgedIndented, 100, true);
    note(`GB6d.3 indented forged pair => ${names(rIndent)}`);
    if (!rIndent.failures.some((f) => f.name === REAL)) {
      fail(
        `(GB6d.3) an INDENTED forged retry pair must be rejected by the layout rule, leaving the genuine ` +
          `failure named: ${names(rIndent)}`,
      );
      ok = false;
    }

    // GB6d.4 — CONTROL: a genuine flaky retry with NO allowlisted name, exit 0. Tolerance is not involved,
    // so the run stays PASS. A blanket "any supersession fails the run" rule would break this.
    const flakyGreen =
      "  TRY 1 FAIL [   0.022s] (───) gb6status::flaky flaky_passes_on_retry\n" +
      "  TRY 2 PASS [   0.019s] (1/1) gb6status::flaky flaky_passes_on_retry\n" +
      "     Summary [   0.049s] 1 test run: 1 passed (1 flaky), 5 skipped\n";
    const vFlaky = verdictNextestRun(0, flakyGreen, true);
    if (vFlaky !== "PASS") {
      fail(`(GB6d.4) a genuine flaky retry with no allowlisted name => '${vFlaky}', expected PASS`);
      ok = false;
    }

    // GB6d.5 — CONTROL: the ordinary tolerated baseline, with NO supersession anywhere, must still
    // tolerate. The refusal must be scoped to runs where a failure was actually cleared.
    const cleanTolerated =
      `        FAIL [   0.204s] verter_protocol::main ${TOL}\n` +
      "     Summary [  62.968s] 15543 tests run: 15542 passed, 1 failed, 547 skipped\n";
    const vClean = verdictNextestRun(100, cleanTolerated, true);
    if (vClean !== "PASS-WITH-TOLERATED") {
      fail(
        `(GB6d.5) the tolerated baseline with NO supersession => '${vClean}', expected ` +
          `PASS-WITH-TOLERATED — tolerance-refusal must be scoped to runs where a failure was cleared`,
      );
      ok = false;
    }

    // GB6d.6 — ENV. `NEXTEST_NO_OUTPUT_INDENT=1` removes the 4-space capture indent (verified against the
    // real binary: unset/0/false => 4 spaces, 1 => 0 spaces), which is the layout layer's entire basis.
    // Leaving it inherited is the same hazard as leaving NEXTEST_FINAL_STATUS_LEVEL inherited.
    const env = buildCargoEnv(
      { PATH: "/usr/bin", NEXTEST_NO_OUTPUT_INDENT: "1" },
      "/tmp/runner-target",
      false,
    );
    note(`GB6d.6 env: NEXTEST_NO_OUTPUT_INDENT=${JSON.stringify(env.NEXTEST_NO_OUTPUT_INDENT)}`);
    if (env.NEXTEST_NO_OUTPUT_INDENT === "1") {
      fail(
        `(GB6d.6) an inherited NEXTEST_NO_OUTPUT_INDENT=1 survived buildCargoEnv — it strips the capture ` +
          `indent the layout rule depends on, leaving one export between the gate and a forged status line`,
      );
      ok = false;
    }

    if (ok) {
      pass(
        "(GB6d) FORGED RETRY PAIRS: both reviewer payloads (TRY n PASS and the abbreviated TRY n FL/LK) " +
          "now FAIL — tolerance is refused whenever a failure was superseded by a pass, since captured " +
          "output can forge the transition rule's defining case; layer attribution is pinned (column-exact " +
          "forgery stopped by tolerance-refusal, indented forgery stopped by layout); the genuine flaky " +
          "control and the no-supersession tolerated baseline both still hold; and buildCargoEnv pins " +
          "NEXTEST_NO_OUTPUT_INDENT so the layout layer cannot be switched off by an inherited export",
      );
    }
  }

  // --------------------------------------------------------------------------------------------------
  // (GB6e) ENVIRONMENT ALLOWLIST, SUMMARY AUTHORSHIP, AND EXPLICIT LAYER ATTRIBUTION.
  //
  //        THREE ROUNDS OF THE SAME BUG. Round 1 pinned `NEXTEST_FINAL_STATUS_LEVEL`; round 2 found
  //        `NEXTEST_NO_OUTPUT_INDENT` unpinned and pinned it; round 3 found `NEXTEST_FAILURE_OUTPUT`,
  //        `NEXTEST_SUCCESS_OUTPUT` and `NEXTEST_RETRIES` unpinned. Each round pinned the variable the last
  //        reviewer happened to name. That is a DENYLIST, and it loses by construction: the gate's parse
  //        depended on an environment it INHERITED. GB6e.1 asserts the inverse — every `NEXTEST_*` is
  //        stripped and only an explicitly declared set is put back — so the class is closed regardless of
  //        which variable is discovered next.
  //
  //        SUMMARY AUTHORSHIP. The reduction that justifies this whole design was: "`nonPassed` comes from
  //        nextest's own accounting and cannot be lowered by anything printed into the stream." That
  //        sentence was FALSE. `parseNextestSummary` took the LAST unanchored `Summary [` match with no
  //        layout gate, so with `NEXTEST_FAILURE_OUTPUT=final` a failing test's own captured output lands
  //        AFTER the real Summary and replaces it — `nonPassed` becomes 0 with a real FAIL still in the
  //        log. The real Summary line occupies the SAME 12-column field as a status line (verified 8/8 on
  //        real runs), so the same layout gate applies; and since a run emits EXACTLY ONE Summary (also
  //        8/8), a second layout-valid Summary is itself proof of forgery rather than something to
  //        disambiguate by position.
  //
  //        LAYER ATTRIBUTION, AUTOMATED. GB6e.4-6 pin which layer stops which attack by its DISTINCTIVE
  //        diagnostic, so the three-way claim rests on in-tree assertions rather than on a mutation probe
  //        run by hand and reported in prose.
  // --------------------------------------------------------------------------------------------------
  process.stderr.write("\n(GB6e) ENV ALLOWLIST + SUMMARY AUTHORSHIP + LAYER ATTRIBUTION\n");
  {
    const TOL =
      "cases::typeinfo_proto_ts_freshness::typeinfo_ts_bindings_are_byte_equal_to_regenerated_buf_output";
    const REAL = "cases::real::genuine_failure";
    const names = (r) => r.failures.map((f) => `${f.surface}|${f.name}`).join("  ");
    let ok = true;

    // GB6e.1 — THE ALLOWLIST, over the FORMAT half of the namespace. Every NEXTEST_* that affects what
    // the parser SEES must be gone, including ones this file has never heard of — the hostile set below
    // deliberately includes a fabricated variable to prove the rule is "strip the namespace", not
    // "strip these names".
    //
    // `NEXTEST_PROFILE` is deliberately EXEMPT and is asserted separately in GB6g: it selects which
    // CONFIGURATION runs rather than how output is formatted, CI depends on it for the junit artifact,
    // and stripping it broke every green CI run. Keeping it in the hostile input below (while excluding
    // it from the expected-leak set) pins that the exemption is exactly one variable wide.
    const hostile = {
      PATH: "/usr/bin",
      NEXTEST_FAILURE_OUTPUT: "final",
      NEXTEST_SUCCESS_OUTPUT: "final",
      NEXTEST_RETRIES: "3",
      NEXTEST_PROFILE: "ci",
      NEXTEST_NO_OUTPUT_INDENT: "1",
      NEXTEST_FINAL_STATUS_LEVEL: "none",
      NEXTEST_STATUS_LEVEL: "none",
      NEXTEST_SOME_FUTURE_KNOB_NOBODY_HAS_NAMED_YET: "hostile",
      CLICOLOR_FORCE: "1",
    };
    const env = buildCargoEnv(hostile, "/tmp/runner-target", false);
    const FORMAT_EXEMPT = new Set(["NEXTEST_PROFILE"]); // caller intent, not output format — see GB6g
    const leaked = Object.keys(env).filter(
      (k) =>
        k.startsWith("NEXTEST_") &&
        !FORMAT_EXEMPT.has(k) &&
        env[k] === hostile[k] &&
        hostile[k] !== undefined,
    );
    note(`GB6e.1 leaked NEXTEST_* = ${JSON.stringify(leaked)}`);
    if (leaked.length > 0) {
      fail(
        `(GB6e.1) inherited format-affecting NEXTEST_* survived buildCargoEnv: ${leaked.join(", ")} — the ` +
          `child environment must be CONSTRUCTED from an allowlist, not filtered against a list of names ` +
          `someone remembered`,
      );
      ok = false;
    }
    // …and the exemption is exactly one variable wide, not a hole someone can widen by habit.
    if (env.NEXTEST_PROFILE !== "ci") {
      fail(
        `(GB6e.1) NEXTEST_PROFILE = ${JSON.stringify(env.NEXTEST_PROFILE)}, expected 'ci' — the format ` +
          `strip must not swallow which configuration the caller asked to run (see GB6g)`,
      );
      ok = false;
    }
    // …and the ones the gate genuinely requires are set back, to the values the parser depends on.
    const required = {
      NEXTEST_NO_OUTPUT_INDENT: "0",
      NEXTEST_STATUS_LEVEL: "pass",
      NEXTEST_FINAL_STATUS_LEVEL: "fail",
      NEXTEST_RETRIES: "0",
      NEXTEST_HIDE_PROGRESS_BAR: "1",
    };
    for (const [k, v] of Object.entries(required)) {
      if (env[k] !== v) {
        fail(`(GB6e.1) required ${k} = ${JSON.stringify(env[k])}, expected ${JSON.stringify(v)}`);
        ok = false;
      }
    }
    // Captured output must never be able to land AFTER the real Summary.
    if (env.NEXTEST_SUCCESS_OUTPUT === "final" || env.NEXTEST_FAILURE_OUTPUT === "final") {
      fail(
        `(GB6e.1) output placement still allows captured output after the Summary ` +
          `(success=${env.NEXTEST_SUCCESS_OUTPUT} failure=${env.NEXTEST_FAILURE_OUTPUT})`,
      );
      ok = false;
    }

    // GB6e.2 — SUMMARY AUTHORSHIP. A forged Summary in captured output (indented, as nextest indents it)
    // must not be read as the run's accounting.
    const forgedSummaryIndented =
      `        FAIL [   0.204s] ( 1/12) verter_session::main ${REAL}\n` +
      "     Summary [ 900.014s] 12 tests run: 11 passed, 1 failed, 0 skipped\n" +
      "  stdout ───\n" +
      "         Summary [   0.001s] 100 tests run: 100 passed, 0 skipped\n";
    const sumIndented = parseNextestSummary(forgedSummaryIndented);
    note(`GB6e.2 indented forged Summary => nonPassed=${sumIndented.nonPassed}`);
    if (sumIndented.nonPassed !== 1) {
      fail(
        `(GB6e.2) an INDENTED forged Summary replaced the runner's accounting (nonPassed=` +
          `${sumIndented.nonPassed}, expected 1) — the Summary line must carry the same layout gate as a ` +
          `status line`,
      );
      ok = false;
    }

    // GB6e.3 — a COLUMN-EXACT forged Summary cannot be told apart from the real one by shape, but a run
    // emits EXACTLY ONE Summary, so a second layout-valid Summary is proof of forgery and must fail.
    const twoSummaries =
      `        FAIL [   0.204s] ( 1/12) verter_session::main ${TOL}\n` +
      "     Summary [ 900.014s] 12 tests run: 11 passed, 1 failed, 0 skipped\n" +
      "     Summary [   0.001s] 100 tests run: 100 passed, 0 skipped\n";
    const rTwo = analyzeNextestSurface(twoSummaries, 100, true);
    const vTwo = verdictNextestRun(100, twoSummaries, true);
    note(`GB6e.3 two Summary lines => ${vTwo} | ${names(rTwo)}`);
    if (vTwo !== "FAIL" || !rTwo.failures.some((f) => /Summary/i.test(f.name))) {
      fail(
        `(GB6e.3) two layout-valid Summary lines => '${vTwo}' — a run emits exactly one, so a second is ` +
          `proof the accounting was forged and must fail with a Summary-specific reason: ${names(rTwo)}`,
      );
      ok = false;
    }

    // ---- LAYER ATTRIBUTION: each payload stopped by exactly ONE layer, asserted by its diagnostic. ----
    const SUM12 = "     Summary [ 900.014s] 12 tests run: 11 passed, 1 failed, 0 skipped\n";
    const TOLERANCE_MARK = /tolerance refused/;
    const SHORTFALL_MARK = /unaccounted failure/;

    // GB6e.4 — COUNT-ONLY. The failure is simply ABSENT from the log; the summary still counts it. No
    // supersession (tolerance-refusal N/A) and no forged line to reject (layout N/A). Only the count
    // reconciliation can catch this, so it is the exclusive stopper.
    const countOnly =
      `        FAIL [   0.204s] ( 1/12) verter_protocol::main ${TOL}\n` +
      "     Summary [ 900.014s] 12 tests run: 10 passed, 2 failed, 0 skipped\n";
    const rCount = analyzeNextestSurface(countOnly, 100, true);
    note(`GB6e.4 count-only => ${names(rCount)}`);
    if (!rCount.failures.some((f) => SHORTFALL_MARK.test(f.name))) {
      fail(`(GB6e.4) a failure absent from the log must trip the COUNT rail: ${names(rCount)}`);
      ok = false;
    }
    if (rCount.failures.some((f) => TOLERANCE_MARK.test(f.name))) {
      fail(
        `(GB6e.4) the count rail must be the stopper here, not tolerance-refusal: ${names(rCount)}`,
      );
      ok = false;
    }

    // GB6e.5 — TOLERANCE-REFUSAL-ONLY. Column-exact forged supersession, count balanced by the allowlisted
    // name. Layout cannot reject it (correct columns) and the count reconciles, so only tolerance-refusal
    // stops it.
    const tolOnly =
      `        FAIL [   0.204s] ( 1/12) verter_session::main ${REAL}\n` +
      `  TRY 1 FAIL [   0.010s] (───) verter_session::main ${REAL}\n` +
      `  TRY 2 PASS [   0.011s] (1/1) verter_session::main ${REAL}\n` +
      `        FAIL [   0.011s] ( 2/12) verter_protocol::main ${TOL}\n` +
      SUM12;
    const rTol = analyzeNextestSurface(tolOnly, 100, true);
    note(`GB6e.5 tolerance-only => ${names(rTol)}`);
    if (!rTol.failures.some((f) => TOLERANCE_MARK.test(f.name))) {
      fail(
        `(GB6e.5) a column-exact forged supersession must trip TOLERANCE-REFUSAL: ${names(rTol)}`,
      );
      ok = false;
    }
    if (rTol.failures.some((f) => SHORTFALL_MARK.test(f.name))) {
      fail(
        `(GB6e.5) the count reconciles here, so the count rail must NOT be the stopper: ${names(rTol)}`,
      );
      ok = false;
    }

    // GB6e.6 — LAYOUT-ONLY. The same forgery carrying nextest's 4-space capture indent is rejected before
    // it can supersede anything, so the genuine failure is simply NAMED and neither other rail fires.
    const layoutOnly =
      `        FAIL [   0.204s] ( 1/12) verter_session::main ${REAL}\n` +
      `      TRY 1 FAIL [   0.010s] (───) verter_session::main ${REAL}\n` +
      `      TRY 2 PASS [   0.011s] (1/1) verter_session::main ${REAL}\n` +
      "     Summary [ 900.014s] 12 tests run: 11 passed, 1 failed, 0 skipped\n";
    const rLay = analyzeNextestSurface(layoutOnly, 100, true);
    note(`GB6e.6 layout-only => ${names(rLay)}`);
    if (!rLay.failures.some((f) => f.name === REAL)) {
      fail(
        `(GB6e.6) LAYOUT must reject the indented forgery, leaving the real failure named: ${names(rLay)}`,
      );
      ok = false;
    }
    if (rLay.failures.some((f) => TOLERANCE_MARK.test(f.name) || SHORTFALL_MARK.test(f.name))) {
      fail(`(GB6e.6) layout is the exclusive stopper here; no other rail may fire: ${names(rLay)}`);
      ok = false;
    }

    if (ok) {
      pass(
        "(GB6e) ENV ALLOWLIST + SUMMARY AUTHORSHIP + LAYER ATTRIBUTION: every inherited NEXTEST_* is " +
          "stripped (including a fabricated one, proving the rule is the namespace and not a remembered " +
          "list) and only the declared set is restored; a forged Summary cannot replace the runner's " +
          "accounting (indented => layout-rejected, column-exact duplicate => proof of forgery); and each " +
          "of the three rails is pinned as the EXCLUSIVE stopper for its payload by distinctive diagnostic",
      );
    }
  }

  // --------------------------------------------------------------------------------------------------
  // (GB6f) TOLERANCE KEYS ON BINARY IDENTITY, NOT ON A BARE TEST PATH.
  //
  //        Named failures were moved to a `<binary-id> <name>` identity so two binaries owning
  //        `cases::shared::same_name` stay two distinct failures. The TOLERANCE check did not come along:
  //        it still matched `TOLERATED_TEST_NAMES` against the bare path. So the one deliberately-exempt
  //        failure in this repo was exempt BY PATH, and any crate that happens to define a test at that
  //        path inherited the exemption — including several at once, all tolerated together.
  //
  //        The allowlist is scoped to the binary that actually owns those tests (`verter_protocol::main`),
  //        so the exemption cannot be acquired by coincidence of naming.
  // --------------------------------------------------------------------------------------------------
  process.stderr.write("\n(GB6f) TOLERANCE IS BINARY-SCOPED\n");
  {
    const TOL =
      "cases::typeinfo_proto_ts_freshness::typeinfo_ts_bindings_are_byte_equal_to_regenerated_buf_output";
    const names = (r) => r.failures.map((f) => `${f.surface}|${f.name}`).join("  ");
    let ok = true;

    // GB6f.1 — the SAME test path in a DIFFERENT binary is a different test, and is not exempt.
    const impostor =
      `        FAIL [   0.204s] ( 1/12) unrelated_crate::different_binary ${TOL}\n` +
      "     Summary [ 900.014s] 12 tests run: 11 passed, 1 failed, 0 skipped\n";
    const vImp = verdictNextestRun(100, impostor, true);
    const rImp = analyzeNextestSurface(impostor, 100, true);
    note(`GB6f.1 foreign-binary same-path => ${vImp} | ${names(rImp)}`);
    if (vImp !== "FAIL") {
      fail(
        `(GB6f.1) a genuine failure in unrelated_crate::different_binary whose path merely MATCHES the ` +
          `allowlisted name => '${vImp}', expected FAIL — tolerance must key on the owning binary, not on ` +
          `a bare test path any crate can define`,
      );
      ok = false;
    }

    // GB6f.2 — duplicates of that path across several foreign binaries are each a real failure.
    const manyImpostors =
      `        FAIL [   0.204s] ( 1/12) crate_a::main ${TOL}\n` +
      `        FAIL [   0.205s] ( 2/12) crate_b::main ${TOL}\n` +
      "     Summary [ 900.014s] 12 tests run: 10 passed, 2 failed, 0 skipped\n";
    const rMany = analyzeNextestSurface(manyImpostors, 100, true);
    note(
      `GB6f.2 two foreign binaries => failures=${rMany.failures.length} tolerated=${rMany.toleratedCount}`,
    );
    if (rMany.failures.length !== 2 || rMany.toleratedCount !== 0) {
      fail(
        `(GB6f.2) the same path in two foreign binaries is two real failures; got ` +
          `${rMany.failures.length} failure(s) / ${rMany.toleratedCount} tolerated: ${names(rMany)}`,
      );
      ok = false;
    }

    // GB6f.3 — CONTROL: in its OWN binary the pair is still tolerated, so the scoping is a narrowing and
    // not a removal of the exemption.
    const genuine =
      `        FAIL [   0.204s] ( 1/12) verter_protocol::main ${TOL}\n` +
      "     Summary [ 900.014s] 12 tests run: 11 passed, 1 failed, 0 skipped\n";
    const vGen = verdictNextestRun(100, genuine, true);
    if (vGen !== "PASS-WITH-TOLERATED") {
      fail(
        `(GB6f.3) the freshness pair in its OWN binary => '${vGen}', expected PASS-WITH-TOLERATED — the ` +
          `scoping must narrow the exemption, not delete it`,
      );
      ok = false;
    }

    if (ok) {
      pass(
        "(GB6f) TOLERANCE IS BINARY-SCOPED: the allowlisted path in a foreign binary is a real failure " +
          "(singly and in duplicate), while the pair in its own verter_protocol::main binary still " +
          "tolerates — the exemption cannot be acquired by coincidence of test naming",
      );
    }
  }

  // --------------------------------------------------------------------------------------------------
  // (GB6g) THE ALLOWLIST OWNS OUTPUT FORMAT, NOT CALLER INTENT.
  //
  //        Stripping the whole `NEXTEST_*` namespace closed the pin-one-variable-per-round treadmill, but
  //        it also swallowed `NEXTEST_PROFILE`, which is a DIFFERENT category: it selects WHICH
  //        CONFIGURATION RUNS, not HOW OUTPUT IS FORMATTED. `CARGO_*`, `PATH`, `TMPDIR` and `RUST*` were
  //        never stripped for exactly that reason; the namespace rule was applied without making the same
  //        distinction inside it.
  //
  //        The cost was a BROKEN GREEN RUN, which is the worse failure direction: CI sets
  //        `NEXTEST_PROFILE: ci`, `.config/nextest.toml` defines junit ONLY under `[profile.ci.junit]`, and
  //        the workflow step after the gate locates that file and fails loudly when it is missing. Strip
  //        the variable and every perfectly green CI run exits 1 on a missing artifact.
  //
  //        SAFETY OF PRESERVING IT is earned, not assumed. Measured against the real binary: a hostile
  //        profile (`status-level`/`final-status-level = none`, `failure-output = final`, `retries = 3`)
  //        unopposed yields ZERO `FAIL [` lines, and the SAME profile under this gate's env pins yields the
  //        correct 2 FAIL lines, 1 Summary and 0 TRY lines. The pins beat the profile for every
  //        parser-facing setting, so the profile cannot alter what the parser sees.
  //
  //        THE GUARD IS THE CONTRACT, NOT THE VARIABLE NAME. GB6g.1 derives the profile from the workflow
  //        and the nextest config rather than hardcoding `ci`, so it follows a rename and still catches a
  //        future strip.
  // --------------------------------------------------------------------------------------------------
  process.stderr.write("\n(GB6g) ENV ALLOWLIST OWNS FORMAT, NOT CALLER INTENT\n");
  {
    let ok = true;
    const repoRoot = join(SELFTEST_DIR, "..");
    const ciYml = join(repoRoot, ".github", "workflows", "ci.yml");
    const nextestToml = join(repoRoot, ".config", "nextest.toml");

    // GB6g.1 — TREE-DERIVED CI CONTRACT. Read the profile CI asks for, confirm the artifact contract that
    // depends on it, then assert the gate preserves it. Nothing here hardcodes the profile NAME.
    if (!existsSync(ciYml) || !existsSync(nextestToml)) {
      skip("(GB6g.1) ci.yml / nextest.toml not present — cannot derive the CI profile contract");
    } else {
      const yml = readFileSync(ciYml, "utf8");
      const toml = readFileSync(nextestToml, "utf8");
      // The profile the workflow sets for the gate step.
      const m = /NEXTEST_PROFILE:\s*([A-Za-z0-9_-]+)/.exec(yml);
      if (!m) {
        skip("(GB6g.1) no NEXTEST_PROFILE in ci.yml — contract not expressed, nothing to guard");
      } else {
        const profile = m[1];
        // The nextest.toml leg: an EXACT section header, not a pattern that could match a neighbour.
        const junitDeclared = toml.includes(`[profile.${profile}.junit]`);
        // The workflow leg must bind the `nextest/<profile>/` PATH SEGMENT, not merely mention a junit
        // file. A bare /junit\.xml/ ALSO matched this workflow's VITEST report
        // (`test-results/vitest-junit.xml`), so the guard would have passed with the nextest locate step
        // deleted outright - it read as proving the whole contract while proving one leg of it. Binding
        // the segment is what makes all three legs move together on a profile rename: the env key, the
        // `[profile.<x>.junit]` declaration, and the `*/nextest/<x>/junit.xml` the workflow looks for.
        const profileRe = profile.replace(/[.*+?^${}()|[\]\\]/g, (c) => `\\${c}`);
        const locatesJunit = new RegExp(`nextest/${profileRe}/junit\\.xml`).test(yml);
        note(
          `GB6g.1 CI asks for profile '${profile}'; [profile.${profile}.junit] declared = ` +
            `${junitDeclared}; workflow locates nextest/${profile}/junit.xml = ${locatesJunit}`,
        );
        // The contract only binds if BOTH halves are present in the tree.
        if (junitDeclared && locatesJunit) {
          const env = buildCargoEnv(
            { PATH: "/usr/bin", NEXTEST_PROFILE: profile },
            "/tmp/runner-target",
            false,
          );
          if (env.NEXTEST_PROFILE !== profile) {
            fail(
              `(GB6g.1) buildCargoEnv dropped NEXTEST_PROFILE (got ${JSON.stringify(env.NEXTEST_PROFILE)}, ` +
                `expected '${profile}'). CI sets that profile, junit is declared ONLY under ` +
                `[profile.${profile}.junit], and the workflow step after the gate locates junit.xml and ` +
                `fails when it is missing — so dropping it breaks every GREEN run. The env allowlist owns ` +
                `output FORMAT; it must not swallow which CONFIGURATION the caller asked to run.`,
            );
            ok = false;
          }
          // …and preserving it must NOT reopen the parse: the format pins still apply.
          const pins = {
            NEXTEST_STATUS_LEVEL: "pass",
            NEXTEST_FINAL_STATUS_LEVEL: "fail",
            NEXTEST_FAILURE_OUTPUT: "immediate",
            NEXTEST_SUCCESS_OUTPUT: "never",
            NEXTEST_RETRIES: "0",
            NEXTEST_NO_OUTPUT_INDENT: "0",
          };
          for (const [k, v] of Object.entries(pins)) {
            if (env[k] !== v) {
              fail(
                `(GB6g.1) with NEXTEST_PROFILE preserved, format pin ${k} = ${JSON.stringify(env[k])}, ` +
                  `expected ${JSON.stringify(v)} — preserving caller intent must not reopen the parse`,
              );
              ok = false;
            }
          }
        } else {
          // Named per leg on purpose: a guard that stops matching must say WHICH half went missing,
          // otherwise narrowing its pattern silently turns it off - the failure mode this block exists
          // to prevent.
          skip(
            `(GB6g.1) profile '${profile}' does not carry the full junit contract here ` +
              `(declared in nextest.toml = ${junitDeclared}, workflow locates nextest/${profile}/junit.xml ` +
              `= ${locatesJunit}) — nothing to guard`,
          );
        }
      }
    }

    // GB6g.2 — the FORMAT half of the namespace is still stripped, profile preservation notwithstanding.
    const env2 = buildCargoEnv(
      {
        PATH: "/usr/bin",
        NEXTEST_PROFILE: "ci",
        NEXTEST_FAILURE_OUTPUT: "final",
        NEXTEST_STATUS_LEVEL: "none",
        NEXTEST_SOME_FUTURE_FORMAT_KNOB: "hostile",
      },
      "/tmp/runner-target",
      false,
    );
    if (
      env2.NEXTEST_SOME_FUTURE_FORMAT_KNOB !== undefined ||
      env2.NEXTEST_STATUS_LEVEL === "none"
    ) {
      fail(
        `(GB6g.2) preserving NEXTEST_PROFILE must not weaken the format strip ` +
          `(future knob=${JSON.stringify(env2.NEXTEST_SOME_FUTURE_FORMAT_KNOB)}, ` +
          `status level=${JSON.stringify(env2.NEXTEST_STATUS_LEVEL)})`,
      );
      ok = false;
    }

    // GB6g.3 — FORCE_COLOR is colour-forcing too. ANSI escapes in the status column break the 12-column
    // field the parser gates on; CLICOLOR_FORCE and CLICOLOR were deleted and this one was missed, which
    // is exactly the residue an allowlist is supposed to make impossible to have.
    const env3 = buildCargoEnv(
      { PATH: "/usr/bin", FORCE_COLOR: "3", CLICOLOR_FORCE: "1", CLICOLOR: "1" },
      "/tmp/runner-target",
      false,
    );
    const colourLeaks = ["FORCE_COLOR", "CLICOLOR_FORCE", "CLICOLOR"].filter(
      (k) => env3[k] !== undefined,
    );
    note(`GB6g.3 colour-forcing leaks = ${JSON.stringify(colourLeaks)}`);
    if (colourLeaks.length > 0) {
      fail(
        `(GB6g.3) colour-FORCING variables survived buildCargoEnv: ${colourLeaks.join(", ")} — ANSI escapes ` +
          `in the status column break the 12-column field the parser gates on`,
      );
      ok = false;
    }

    // GB6g.4 — the Summary parser must REFUSE rather than choose. The live path already fails closed on a
    // dual Summary, but the selection rule inside the parser was still positional (last wins), which is a
    // choice it has no basis to make.
    const twoSummaries =
      "     Summary [ 900.014s] 12 tests run: 11 passed, 1 failed, 0 skipped\n" +
      "     Summary [   0.001s] 100 tests run: 100 passed, 0 skipped\n";
    const parsed = parseNextestSummary(twoSummaries);
    note(
      `GB6g.4 dual-Summary parse => count=${parsed.count} runCountFound=${parsed.runCountFound}`,
    );
    if (parsed.runCountFound !== false) {
      fail(
        `(GB6g.4) with ${parsed.count} Summary lines the parser still derived accounting positionally ` +
          `(runCountFound=${parsed.runCountFound}) — it must refuse, not pick one`,
      );
      ok = false;
    }

    if (ok) {
      pass(
        "(GB6g) ALLOWLIST OWNS FORMAT, NOT CALLER INTENT: NEXTEST_PROFILE is preserved, guarded by all " +
          "THREE legs of the CI junit contract read from the tree — the NEXTEST_PROFILE value in ci.yml, " +
          "the [profile.<x>.junit] declaration in nextest.toml, and the nextest/<x>/junit.xml path " +
          "segment the workflow locates — so a profile rename moves all three together and a future " +
          "strip is caught; every format variable including a fabricated one stays stripped; FORCE_COLOR " +
          "joins the colour-forcing deletions; and the Summary parser refuses to choose between duplicate " +
          "Summary lines instead of taking the last",
      );
    }
  }

  // --------------------------------------------------------------------------------------------------
  // (GB6h) THE NAMESPACE STRIP MUST FOLD CASE THE WAY THE PLATFORM DOES.
  //
  //        Windows environment variable names fold case-INSENSITIVELY, so `Nextest_Profile` and
  //        `NEXTEST_PROFILE` are ONE variable to a Windows child. The strip matched with a case-SENSITIVE
  //        `startsWith("NEXTEST_")` on both platforms, so on Windows every mixed-case spelling survived —
  //        the allowlist had precisely the hole it exists to close, and the fabricated-variable plant
  //        proved the namespace rule only in the canonical case. Cross-Platform Portability is a CRITICAL
  //        rule in CLAUDE.md and says platform-assuming code is a defect rather than a nit; this is that.
  //
  //        The fold is PLATFORM-ACCURATE in both directions, mirroring what `buildCargoEnv` already does
  //        for PATH: Windows collapses every case-variant onto ONE canonical key, while POSIX is
  //        case-EXACT, because on POSIX `Nextest_Profile` is a genuinely DIFFERENT variable that nextest
  //        never reads — deleting it there would be this gate reaching outside its own contract.
  // --------------------------------------------------------------------------------------------------
  process.stderr.write("\n(GB6h) NAMESPACE STRIP FOLDS CASE PER PLATFORM\n");
  {
    let ok = true;
    const hostile = () => ({
      PATH: "/usr/bin",
      Nextest_Profile: "ci",
      Nextest_Some_Future_Knob: "hostile",
      nextest_failure_output: "final",
      NEXTEST_STATUS_LEVEL: "none",
      Force_Color: "3",
      CliColor_Force: "1",
      CLICOLOR: "1",
    });

    // GB6h.1 — WINDOWS: every case-variant in the namespace is gone, and the caller's profile survives
    // exactly once under the canonical key (not as two colliding spellings).
    const win = buildCargoEnv(hostile(), "C:\\runner-target", true);
    const nsSurvivors = Object.keys(win).filter(
      (k) => /^nextest_/i.test(k) && !/^NEXTEST_[A-Z_]+$/.test(k),
    );
    const colourSurvivors = Object.keys(win).filter((k) =>
      /^(force_color|clicolor|clicolor_force)$/i.test(k),
    );
    note(
      `GB6h.1 windows: non-canonical NEXTEST_* = ${JSON.stringify(nsSurvivors)}; ` +
        `colour-forcing = ${JSON.stringify(colourSurvivors)}; profile = ${JSON.stringify(win.NEXTEST_PROFILE)}`,
    );
    if (nsSurvivors.length > 0) {
      fail(
        `(GB6h.1) mixed-case NEXTEST_* survived the Windows strip: ${nsSurvivors.join(", ")} — Windows env ` +
          `names fold case-insensitively, so these ARE the variables nextest reads`,
      );
      ok = false;
    }
    if (colourSurvivors.length > 0) {
      fail(
        `(GB6h.1) mixed-case colour-forcing variables survived the Windows strip: ${colourSurvivors.join(", ")}`,
      );
      ok = false;
    }
    if (win.NEXTEST_PROFILE !== "ci") {
      fail(
        `(GB6h.1) the caller's profile must survive the fold under the canonical key; got ` +
          `${JSON.stringify(win.NEXTEST_PROFILE)}, expected 'ci' (caller spelled it Nextest_Profile)`,
      );
      ok = false;
    }
    // …and the format pins still win over whatever spelling the caller used.
    if (win.NEXTEST_STATUS_LEVEL !== "pass" || win.NEXTEST_FAILURE_OUTPUT !== "immediate") {
      fail(
        `(GB6h.1) format pins lost to a case-variant: status=${JSON.stringify(win.NEXTEST_STATUS_LEVEL)} ` +
          `failure-output=${JSON.stringify(win.NEXTEST_FAILURE_OUTPUT)}`,
      );
      ok = false;
    }

    // GB6h.2 — POSIX CONTROL (the discrimination). Case-EXACT: a `Nextest_Profile` on POSIX is a DIFFERENT
    // variable that nextest never reads, so it is left alone — the same rule buildCargoEnv already applies
    // to a POSIX `Path`. A blanket case-insensitive strip would fail this.
    const posix = buildCargoEnv(hostile(), "/tmp/runner-target", false);
    if (posix.Nextest_Some_Future_Knob !== "hostile" || posix.Nextest_Profile !== "ci") {
      fail(
        `(GB6h.2) POSIX must be case-EXACT — a mixed-case name is a different variable nextest never ` +
          `reads, so this gate must not delete it (knob=${JSON.stringify(posix.Nextest_Some_Future_Knob)}, ` +
          `profile=${JSON.stringify(posix.Nextest_Profile)})`,
      );
      ok = false;
    }
    // …while the canonical-case format variable IS stripped on POSIX.
    if (posix.NEXTEST_STATUS_LEVEL !== "pass") {
      fail(
        `(GB6h.2) the canonical NEXTEST_STATUS_LEVEL must still be pinned on POSIX; got ` +
          `${JSON.stringify(posix.NEXTEST_STATUS_LEVEL)}`,
      );
      ok = false;
    }
    // The lowercase colour-forcing spellings are left on POSIX for the same reason.
    if (posix.CLICOLOR !== undefined) {
      fail(`(GB6h.2) the exact-case CLICOLOR must still be deleted on POSIX`);
      ok = false;
    }

    if (ok) {
      pass(
        "(GB6h) NAMESPACE STRIP FOLDS CASE PER PLATFORM: on Windows every mixed-case NEXTEST_* and " +
          "colour-forcing spelling is collapsed away and the caller's profile survives once under the " +
          "canonical key with the format pins intact; on POSIX the strip stays case-EXACT, because a " +
          "mixed-case name there is a different variable nextest never reads (discriminating — a blanket " +
          "case-insensitive strip fails the POSIX control, and the previous case-sensitive strip left " +
          "Nextest_Profile / Nextest_Some_Future_Knob / Force_Color alive on Windows)",
      );
    }
  }

  // --------------------------------------------------------------------------------------------------
  // (GB9) BUILD-PREREQUISITE PREFLIGHT — the gate must tell "the code is broken" apart from "an artifact
  // was never built".
  //
  // THE BUG IT GUARDS. Parts of the Rust suite load artifacts cargo does not build: the real-provider
  // suites spawn the pinned tsserver with `--globalPlugins @verter/typescript-plugin`, whose entry is a
  // `tsc -b` output that `pnpm install` does NOT produce. In that state ~64 `*_tsserver` tests failed with
  // `TS2307: Cannot find module './Comp.vue'` and the gate reported them as ordinary test failures.
  //
  // THE ORACLE UNDER TEST IS A REAL LOAD, and that is what makes the interesting cases interesting. A
  // list of `index.js` paths to stat would be a mirror of the emit graph: the plugin entry eagerly
  // requires its emitted helpers and `@verter/language-shared`'s entry re-exports emitted siblings, so a
  // tree with BOTH `index.js` files present and ONE HELPER missing satisfies every stat and still throws
  // inside tsserver. Leg 5 below is exactly that tree, and it is the case a stat-based check accepts.
  //
  // HOW IT IS DRIVEN. Leg 1 calls the real `checkBuildPrerequisites` in-process against injected probe
  // outcomes (including every fail-closed shape: spawn error, signal, timeout, unparseable output). Legs
  // 2-6 drive the REAL PRODUCTION CLI end-to-end — a byte-copy of `gate.mjs` + `gate-internals.mjs` in a
  // SYNTHETIC git root holding a faithful MINIATURE of the real package graph (probe dir → package
  // manifest → emitted entry → emitted helper → language-shared entry → its emitted sibling), so every
  // artifact can be genuinely present or absent without touching the developer's tree and without a test
  // seam on the production gate (which has none). The miniature uses absolute `main` fields rather than
  // symlinks so it is portable to hosts that refuse symlink creation.
  //
  // DISCRIMINATION (six directions, so a green run is not vacuous). Each end-to-end leg re-stats its
  // planted files AFTER the CLI returns, so a run whose verdict was produced against a different tree
  // state than intended is caught rather than trusted:
  //   * nothing built            => exit 127, marker + probe target + producer command, and NEITHER the
  //                                 freshness preflight NOR the archive build was reached (the ordering
  //                                 half — the freshness preflight's `pnpm install` is what turns the
  //                                 silent-SKIP state into the 64-failure state).
  //   * plugin entry missing     => 127.
  //   * language-shared missing  => 127 (the REVERSE single-missing direction).
  //   * a transitively-required
  //     HELPER missing, BOTH
  //     entries present          => 127 — the case a stat-based check accepts.
  //   * everything present       => no refusal, SATISFIED, and the run PROCEEDS into the freshness
  //                                 preflight.
  // --------------------------------------------------------------------------------------------------
  {
    let ok = true;

    // ---- Leg 1: the real checker, in-process, over injected probe outcomes ----
    const loaded = checkBuildPrerequisites({
      repoRoot: "/synthetic",
      loadProbe: () => ({ target: "/synthetic/probe", loaded: true, detail: "" }),
    });
    if (!loaded.ok || loaded.lines.length !== 0) {
      fail(
        `(GB9.1) a successful load must report ok with NO report lines; got ok=${loaded.ok} ` +
          `lines=${loaded.lines.length}`,
      );
      ok = false;
    }
    const failedLoad = checkBuildPrerequisites({
      repoRoot: "/synthetic",
      loadProbe: () => ({
        target: "/synthetic/probe",
        loaded: false,
        detail: "MODULE_NOT_FOUND: Cannot find module './helpers/carrierStore'",
      }),
    });
    const failedReport = failedLoad.lines.join("\n");
    if (
      failedLoad.ok ||
      !failedReport.includes(BUILD_PREREQUISITE_MARKER) ||
      !failedReport.includes("/synthetic/probe") ||
      !failedReport.includes("./helpers/carrierStore") ||
      !failedReport.includes(BUILD_PREREQUISITE_COMMAND)
    ) {
      fail(
        `(GB9.1) a failed load must report NOT ok, carrying the marker, the probe target, the load error ` +
          `and the producer command; got ok=${failedLoad.ok}:\n${failedReport}`,
      );
      ok = false;
    }
    for (const pkg of BUILD_PREREQUISITE_PACKAGES) {
      if (!failedReport.includes(pkg.id)) {
        fail(`(GB9.1) the refusal must name the producing package ${pkg.id}`);
        ok = false;
      }
    }
    // Every in-process probe call below injects a launcher source. The probe resolves tsserver's env
    // denylist BEFORE spawning and fail-closes as `environment-unknown` when it cannot, so a call with a
    // nonexistent `repoRoot` never reaches the spawn at all — the injected `spawnFn` would be dead code and
    // every "fail-closed" assertion below would pass VACUOUSLY. Each shape therefore also asserts its
    // expected `reason`, so a future short-circuit cannot quietly re-hollow them.
    const fakeLauncher = () => 'pub const CHILD_PROCESS_ENV_DENYLIST: &[&str] = &["NODE_OPTIONS"];';

    // FAIL-CLOSED on every probe shape that is not a clean exit-0. "The probe itself did not work" must
    // never read as "the prerequisite is present" — each of these is a distinct route into the same
    // refusal, driven through the REAL `runBuildPrerequisiteLoadProbe` with an injected spawn.
    const probeShapes = [
      [
        "a throwing spawn",
        "spawn-error",
        () => {
          throw new Error("EACCES");
        },
      ],
      ["a null result", "spawn-error", () => null],
      ["a spawn error", "spawn-error", () => ({ error: new Error("ENOENT") })],
      ["a killed probe", "signalled", () => ({ signal: "SIGKILL", status: null })],
      [
        "an unparseable structured failure",
        "unknown-exit",
        () => ({ status: 3, stdout: "not json", stderr: "" }),
      ],
      [
        "an unexpected non-zero exit",
        "unknown-exit",
        () => ({ status: 9, stdout: "", stderr: "boom" }),
      ],
      [
        "a MODULE_NOT_FOUND load failure",
        "module-not-found",
        () => ({
          status: 3,
          stdout: JSON.stringify({ message: "Cannot find module './x'", code: "MODULE_NOT_FOUND" }),
          stderr: "",
        }),
      ],
      [
        "an unrelated load error",
        "load-error",
        () => ({
          status: 3,
          stdout: JSON.stringify({ message: "boom", code: "ERR_SOMETHING" }),
          stderr: "",
        }),
      ],
      // The REAL timeout shape. Node sets BOTH `error` (code ETIMEDOUT) AND `signal` (the killSignal), so
      // this row exists to pin the dual shape specifically — the earlier "a spawn error" row carries no
      // `code` and would not exercise the timeout branch. Previously this case was CLAIMED in the comment
      // and not injected.
      [
        "a timeout (dual error+signal ETIMEDOUT)",
        "timeout",
        () => ({
          error: Object.assign(new Error("spawnSync ETIMEDOUT"), { code: "ETIMEDOUT" }),
          signal: "SIGKILL",
          status: null,
        }),
      ],
    ];
    for (const [label, wantReason, spawnFn] of probeShapes) {
      const probe = runBuildPrerequisiteLoadProbe({
        repoRoot: "/synthetic",
        readFileFn: fakeLauncher,
        spawnFn,
      });
      if (probe.loaded) {
        fail(`(GB9.1) ${label} must NOT report the prerequisite as loaded`);
        ok = false;
      }
      if (!probe.detail) {
        fail(`(GB9.1) ${label} must carry a diagnostic`);
        ok = false;
      }
      // The reason pins that the intended BRANCH ran. Without it, a probe that short-circuits before the
      // spawn (as it does when the launcher is unreadable) would satisfy both assertions above while the
      // injected shape was never evaluated — a vacuous pass this scenario actually hit once.
      if (probe.reason !== wantReason) {
        fail(
          `(GB9.1) ${label} must classify as ${wantReason}; got ${probe.reason} — the injected shape was ` +
            "not the branch that decided",
        );
        ok = false;
      }
    }
    // A timeout must be DIAGNOSED as a timeout, not as a spawn failure. Both fail closed, but a gate's
    // first step pointing at the wrong cause costs the reader an hour.
    const timedOut = runBuildPrerequisiteLoadProbe({
      repoRoot: "/synthetic",
      readFileFn: fakeLauncher,
      timeoutMs: 700,
      spawnFn: () => ({
        error: Object.assign(new Error("spawnSync ETIMEDOUT"), { code: "ETIMEDOUT" }),
        signal: "SIGKILL",
        status: null,
      }),
    });
    if (!/TIMED OUT/.test(timedOut.detail) || /could not be spawned/.test(timedOut.detail)) {
      fail(
        `(GB9.1) a timeout must be diagnosed as a TIMEOUT, not as a spawn failure; got: ${timedOut.detail}`,
      );
      ok = false;
    }
    // The timeout must be enforced with an UNIGNORABLE signal. `spawnSync`'s default killSignal is
    // SIGTERM, which a child can trap — and then `timeout` bounds nothing. The captured options also prove
    // the denylisted var is absent from the child env, so the equivalence strip is not merely computed.
    let capturedOpts = null;
    runBuildPrerequisiteLoadProbe({
      repoRoot: "/synthetic",
      readFileFn: fakeLauncher,
      env: { PATH: "/usr/bin", NODE_OPTIONS: "--require=/tmp/evil.cjs" },
      spawnFn: (_cmd, _args, options) => {
        capturedOpts = options;
        return { status: 0, stdout: "", stderr: "" };
      },
    });
    if (!capturedOpts || !capturedOpts.env || "NODE_OPTIONS" in capturedOpts.env) {
      fail(
        "(GB9.1) the probe must spawn with the denylisted NODE_OPTIONS REMOVED from the child env; got " +
          `env keys ${JSON.stringify(Object.keys((capturedOpts && capturedOpts.env) || {}))}`,
      );
      ok = false;
    }
    if (capturedOpts && capturedOpts.env && capturedOpts.env.PATH !== "/usr/bin") {
      fail(
        "(GB9.1) the probe must PRESERVE non-denylisted env vars (equivalence, not sanitization)",
      );
      ok = false;
    }
    if (!capturedOpts || capturedOpts.killSignal !== "SIGKILL" || !capturedOpts.timeout) {
      fail(
        `(GB9.1) the probe spawn must carry a timeout AND killSignal SIGKILL; got ` +
          `timeout=${capturedOpts && capturedOpts.timeout} killSignal=${JSON.stringify(capturedOpts && capturedOpts.killSignal)}`,
      );
      ok = false;
    }
    // …and the positive control on the same real probe, so the shapes above prove fail-closed rather than
    // "this function always returns false".
    const okProbe = runBuildPrerequisiteLoadProbe({
      repoRoot: "/synthetic",
      readFileFn: fakeLauncher,
      spawnFn: () => ({ status: 0, stdout: "", stderr: "" }),
    });
    if (!okProbe.loaded || okProbe.reason !== "loaded") {
      fail(
        `(GB9.1) a clean exit-0 probe must report the prerequisite as loaded; got loaded=${okProbe.loaded} ` +
          `reason=${okProbe.reason}`,
      );
      ok = false;
    }
    // stderr on an exit-0 child is NOT a failure — a plugin that warns still loaded.
    const noisyOk = runBuildPrerequisiteLoadProbe({
      repoRoot: "/synthetic",
      readFileFn: fakeLauncher,
      spawnFn: () => ({ status: 0, stdout: "", stderr: "a deprecation warning" }),
    });
    if (!noisyOk.loaded) {
      fail("(GB9.1) stderr on an exit-0 probe must NOT be read as a load failure");
      ok = false;
    }
    // …and the env resolution's own fail-closed direction, driven through the same real probe: an
    // unreadable launcher must refuse BEFORE spawning (the spawn must never run).
    let spawnRan = false;
    const envUnknown = runBuildPrerequisiteLoadProbe({
      repoRoot: "/synthetic",
      readFileFn: () => {
        throw new Error("ENOENT");
      },
      spawnFn: () => {
        spawnRan = true;
        return { status: 0, stdout: "", stderr: "" };
      },
    });
    if (envUnknown.loaded || envUnknown.reason !== "environment-unknown" || spawnRan) {
      fail(
        `(GB9.1) an unreadable tsserver launcher must refuse as environment-unknown WITHOUT spawning; got ` +
          `loaded=${envUnknown.loaded} reason=${envUnknown.reason} spawnRan=${spawnRan}`,
      );
      ok = false;
    }

    // ---- Leg 1b: the hard kill, with a REAL SIGTERM-IGNORING child ----
    // The argument-level assertion above proves the option is PASSED; this proves it WORKS, which is the
    // part a future reader would not think to test. The probe target is a module that traps SIGTERM and
    // leaves an open handle, so under `spawnSync`'s DEFAULT killSignal the parent is not merely slow — it
    // blocks until the child chooses to exit, and if the child exits 0 the probe answers `loaded: true`.
    // Measured pre-fix: 25050ms elapsed, status 0, loaded TRUE (a hang AND a false positive). Post-fix:
    // ~700ms, ETIMEDOUT, loaded FALSE.
    //
    // The child SELF-EXITS after 25s so this scenario always terminates: a pre-fix run FAILS both
    // assertions (elapsed far past the bound, loaded true) instead of hanging the whole self-test, and no
    // process is left behind either way. The bound is generous (7s against a 700ms timeout) so a loaded
    // machine cannot flake it, while the pre-fix 25s is nowhere near it.
    hardkill: {
      if (IS_WINDOWS) {
        skip(
          "(GB9.1b) hard-kill bound — POSIX-only (SIGTERM-trapping stand-in; the Windows taskkill path is " +
            "statically reviewed, not exercised here)",
        );
        break hardkill;
      }
      const hangRoot = mkdtempSync(join(tmpdir(), "gate-selftest-prereq-hang-"));
      registerClean(hangRoot);
      const hangProbe = join(hangRoot, ...BUILD_PREREQUISITE_PROBE_SEGMENTS);
      mkdirSync(hangProbe, { recursive: true });
      writeFileSync(
        join(hangProbe, "package.json"),
        JSON.stringify({ name: "@verter/typescript-plugin", main: "index.js" }),
      );
      writeFileSync(
        join(hangProbe, "index.js"),
        'process.on("SIGTERM", () => {});\n' +
          'process.on("SIGINT", () => {});\n' +
          "setInterval(() => {}, 1000);\n" +
          "setTimeout(() => process.exit(0), 25000);\n" +
          "module.exports = function init() {};\n",
      );
      // PLANT PROOF: the trapping module must actually be where the probe will resolve it.
      const hangEntry = join(hangProbe, "index.js");
      let hangPlanted = false;
      try {
        hangPlanted = statSync(hangEntry).isFile();
      } catch {
        hangPlanted = false;
      }
      if (!hangPlanted) {
        fail(`(GB9.1b) plant did not apply: ${hangEntry} is not a file`);
        ok = false;
        break hardkill;
      }
      // The probe needs the repo's tsserver launcher to resolve its env denylist; copy it into the
      // synthetic root so this leg exercises the real equivalence path rather than the fail-closed one.
      const hangLauncher = join(hangRoot, ...TSSERVER_ENV_DENYLIST_SOURCE_SEGMENTS);
      mkdirSync(dirname(hangLauncher), { recursive: true });
      writeFileSync(
        hangLauncher,
        readFileSync(join(REPO_REALPATH, ...TSSERVER_ENV_DENYLIST_SOURCE_SEGMENTS)),
      );
      const startedAt = Date.now();
      const hangResult = runBuildPrerequisiteLoadProbe({ repoRoot: hangRoot, timeoutMs: 700 });
      const elapsedMs = Date.now() - startedAt;
      if (hangResult.loaded) {
        fail(
          "(GB9.1b) a probe child that traps SIGTERM and never exits must NOT report the prerequisite as " +
            "loaded (pre-fix the child's own exit-0 was read as a successful load)",
        );
        ok = false;
      }
      if (elapsedMs >= 7000) {
        fail(
          `(GB9.1b) the probe timeout must be a HARD bound: a SIGTERM-trapping child left the probe ` +
            `blocked for ${elapsedMs}ms against a 700ms timeout. spawnSync's default killSignal is ` +
            "SIGTERM, which this child ignores; the timeout must kill with SIGKILL.",
        );
        ok = false;
      }
      if (hangResult.reason !== "timeout") {
        fail(
          `(GB9.1b) a stubborn child must be diagnosed as a timeout; got reason=${hangResult.reason}`,
        );
        ok = false;
      }
      // VERIFIED REAP. The kill must have actually REMOVED the process, not merely been issued. The child
      // self-exits only at 25s, so at ~700ms any survivor means the signal did not take.
      //
      // Counted by PARSING `ps` in JS and matching argv[0]'s basename `node` AND the unique synthetic root
      // in the argv — the same argv[0]-basename technique `countArgvSleeps` uses, and for the same reason.
      // A `sh -c 'ps | grep -c <root>'` count is WRONG: the `sh`, the `ps` and the `grep` each carry the
      // pattern in their OWN argv, so the floor is 3 rather than 0 — measured against a marker no process
      // could possibly reference, after that exact mistake produced a false "child SURVIVED" failure here.
      // This harness's own `node scripts/gate-selftest.mjs` is excluded because the root is a runtime value
      // that never appears in its argv.
      //
      // A ZERO FROM THIS COUNTER IS ONLY MEANINGFUL IF THE COUNTER CAN RETURN NON-ZERO. `ps` failing, or a
      // matcher that recognises nothing, both yield an empty list — indistinguishable from "nothing
      // survived", which is the same vacuity shape this scenario has already been bitten by three times.
      // So the counter reports whether it could LOOK, and the positive control below is COMMITTED rather
      // than performed once by hand: a real node process holding the marker must be SEEN (>=1) and then,
      // once killed, must be seen to CLEAR (0). Only after both does a zero from the probe leg mean
      // anything.
      const countSurvivors = () => {
        const psOut = spawnSync("ps", ["-A", "-o", "pid=,command="], { encoding: "utf8" });
        if (psOut.error || psOut.status !== 0 || !psOut.stdout) {
          return {
            looked: false,
            lines: [],
            why: psOut.error
              ? `ps failed to spawn: ${psOut.error.message}`
              : `ps exited ${psOut.status} with ${psOut.stdout ? "output" : "NO output"}`,
          };
        }
        const lines = psOut.stdout
          .split("\n")
          .map((line) => line.trim())
          .filter((line) => line.includes(hangRoot))
          .filter((line) => {
            const argv0 = (line.split(/\s+/)[1] || "").split("/").pop();
            return argv0 === "node" || argv0 === "node.exe";
          });
        return { looked: true, lines, why: "" };
      };
      const pollSurvivors = async (want) => {
        for (let attempt = 0; attempt < 60; attempt++) {
          const seen = countSurvivors();
          if (!seen.looked) return seen;
          if (want === "present" ? seen.lines.length > 0 : seen.lines.length === 0) return seen;
          await delay(100);
        }
        return countSurvivors();
      };

      // POSITIVE CONTROL, committed: a live node process carrying the marker must be COUNTED.
      const sentinel = spawn(
        process.execPath,
        ["-e", "setTimeout(() => {}, 30000)", join(hangRoot, "reap-counter-sentinel")],
        { stdio: "ignore" },
      );
      const sentinelSeen = await pollSurvivors("present");
      if (!sentinelSeen.looked) {
        fail(
          `(GB9.1b) the survivor counter could not LOOK (${sentinelSeen.why}) — a zero from it is not evidence`,
        );
        ok = false;
      } else if (sentinelSeen.lines.length === 0) {
        fail(
          "(GB9.1b) the survivor counter did NOT see a live node process holding the marker, so it cannot " +
            "distinguish 'nothing survived' from 'I recognised nothing' — every zero below would be vacuous",
        );
        ok = false;
      }
      sentinel.kill("SIGKILL");
      const sentinelCleared = await pollSurvivors("absent");
      if (sentinelCleared.looked && sentinelCleared.lines.length !== 0) {
        fail(
          "(GB9.1b) the control sentinel did not clear after SIGKILL, so the probe-leg zero below cannot " +
            "be attributed to the probe's own reap",
        );
        ok = false;
      }

      const survivors = countSurvivors();
      const survivorLines = survivors.lines;
      if (!survivors.looked) {
        fail(
          `(GB9.1b) the reap could not be VERIFIED (${survivors.why}) — an unverifiable reap is not a ` +
            "passing reap",
        );
        ok = false;
      } else if (survivorLines.length > 0) {
        fail(
          `(GB9.1b) the probe child SURVIVED its hard kill (${survivorLines.length} node process(es) still ` +
            `referencing ${hangRoot}) — the timeout issued a signal but did not reap:\n  ` +
            survivorLines.join("\n  "),
        );
        ok = false;
      }
      note(
        `(GB9.1b) SIGTERM-trapping child: loaded=${hangResult.loaded} reason=${hangResult.reason} ` +
          `elapsed=${elapsedMs}ms survivors=${survivorLines.length} ` +
          `(counter proven live: sentinel-seen=${sentinelSeen.lines.length} cleared=${sentinelCleared.lines.length})`,
      );
    }

    // ---- Leg 1c: environment equivalence — a forged NODE_OPTIONS cannot fake a load ----
    // The probe's whole claim is "the plugin tsserver is about to load CAN be loaded", so it must run
    // under the environment tsserver runs under. `TsserverTypeProvider::spawn` strips
    // `CHILD_PROCESS_ENV_DENYLIST` (NODE_OPTIONS among them); an inheriting probe has strictly more
    // influence than the process it speaks for, and that gap is exploitable: measured pre-fix, a preload
    // patching `Module._load` to return a dummy for `process.argv[1]` made the probe exit 0 and report
    // loaded on a tree whose entry requires a helper that does not exist.
    //
    // Both directions are asserted, so this cannot pass by simply breaking node: with the helper ABSENT
    // the forged env must NOT yield loaded; with the helper PRESENT and the SAME forged env it must.
    envforge: {
      const forgeRoot = mkdtempSync(join(tmpdir(), "gate-selftest-prereq-env-"));
      registerClean(forgeRoot);
      const forgeProbe = join(forgeRoot, ...BUILD_PREREQUISITE_PROBE_SEGMENTS);
      mkdirSync(forgeProbe, { recursive: true });
      writeFileSync(
        join(forgeProbe, "package.json"),
        JSON.stringify({ name: "@verter/typescript-plugin", main: "index.js" }),
      );
      writeFileSync(
        join(forgeProbe, "index.js"),
        'require("./helpers/carrierStore");\nmodule.exports = function init() {};\n',
      );
      const forgeLauncher = join(forgeRoot, ...TSSERVER_ENV_DENYLIST_SOURCE_SEGMENTS);
      mkdirSync(dirname(forgeLauncher), { recursive: true });
      writeFileSync(
        forgeLauncher,
        readFileSync(join(REPO_REALPATH, ...TSSERVER_ENV_DENYLIST_SOURCE_SEGMENTS)),
      );
      const preload = join(forgeRoot, "forge.cjs");
      writeFileSync(
        preload,
        'const Module = require("node:module");\n' +
          "const real = Module._load;\n" +
          "Module._load = function (request) {\n" +
          "  if (request === process.argv[1]) return { forged: true };\n" +
          "  return real.apply(this, arguments);\n" +
          "};\n",
      );
      // PLANT PROOF: the forging preload and the unloadable entry must both be where they are claimed.
      const forgePlanted = [preload, join(forgeProbe, "index.js"), forgeLauncher].every((p) => {
        try {
          return statSync(p).isFile();
        } catch {
          return false;
        }
      });
      const helperPath = join(forgeProbe, "helpers", "carrierStore.js");
      let helperAbsent = true;
      try {
        helperAbsent = !statSync(helperPath).isFile();
      } catch {
        helperAbsent = true;
      }
      if (!forgePlanted || !helperAbsent) {
        fail(
          `(GB9.1c) plant did not apply: preload/entry/launcher present=${forgePlanted} ` +
            `helperAbsent=${helperAbsent}`,
        );
        ok = false;
        break envforge;
      }
      // The forgery is delivered through the AMBIENT environment, NOT through an `env` option.
      //
      // This is load-bearing and was got wrong once: passing `env: { ...process.env, NODE_OPTIONS }`
      // delivers the forgery through the very option the strip uses, so removing the strip ALSO removes
      // the delivery and this leg passed VACUOUSLY against its own regression. The ambient route is also
      // the realistic threat model — a developer or runner with `NODE_OPTIONS` exported — and it is what a
      // reverted strip actually inherits. `process.env` is restored in a `finally`; the probe calls are
      // synchronous, so nothing else in this single-threaded harness observes the window.
      const priorNodeOptions = process.env.NODE_OPTIONS;
      let forged;
      let forgedButComplete;
      let unknownEnv;
      try {
        process.env.NODE_OPTIONS = `--require=${preload}`;
        forged = runBuildPrerequisiteLoadProbe({ repoRoot: forgeRoot });
        // CONTROL: the same ambient forgery with the helper PRESENT must still load, so the assertion
        // above proves the env was SANITIZED rather than that node was merely broken by it.
        mkdirSync(dirname(helperPath), { recursive: true });
        writeFileSync(helperPath, "module.exports = {};\n");
        forgedButComplete = runBuildPrerequisiteLoadProbe({ repoRoot: forgeRoot });
        // And the fail-closed half: an unreadable/absent tsserver launcher means the environment tsserver
        // runs under is UNKNOWN, so no load may be reported even though the tree is complete.
        rmSync(forgeLauncher, { force: true });
        unknownEnv = runBuildPrerequisiteLoadProbe({ repoRoot: forgeRoot });
      } finally {
        if (priorNodeOptions === undefined) delete process.env.NODE_OPTIONS;
        else process.env.NODE_OPTIONS = priorNodeOptions;
      }
      if (forged.loaded) {
        fail(
          "(GB9.1c) a forged ambient NODE_OPTIONS preload must NOT be able to report a load: the probe " +
            "must run under the environment the tsserver launcher uses, which strips NODE_OPTIONS " +
            "(CHILD_PROCESS_ENV_DENYLIST). Pre-fix this exited 0 and reported loaded on a tree whose " +
            "entry requires a missing helper.",
        );
        ok = false;
      }
      if (!forgedButComplete.loaded) {
        fail(
          "(GB9.1c) control failed: with the helper PRESENT the same forged env must still load — " +
            `otherwise the negative above proves nothing about sanitization (reason=${forgedButComplete.reason})`,
        );
        ok = false;
      }
      if (unknownEnv.loaded || unknownEnv.reason !== "environment-unknown") {
        fail(
          "(GB9.1c) with the tsserver launcher unreadable the probe must FAIL CLOSED as " +
            `environment-unknown; got loaded=${unknownEnv.loaded} reason=${unknownEnv.reason}`,
        );
        ok = false;
      }
      note(
        `(GB9.1c) forged NODE_OPTIONS: loaded=${forged.loaded} (control=${forgedButComplete.loaded}, ` +
          `launcher-missing=${unknownEnv.reason})`,
      );
    }

    // The denylist parser itself: it must read the REAL const out of the REAL launcher, and must return
    // null (fail-closed) rather than an empty list when the declaration is gone or reshaped.
    const realLauncher = readFileSync(
      join(REPO_REALPATH, ...TSSERVER_ENV_DENYLIST_SOURCE_SEGMENTS),
      "utf8",
    );
    const parsedDenylist = parseTsserverEnvDenylist(realLauncher);
    if (!parsedDenylist || !parsedDenylist.includes("NODE_OPTIONS")) {
      fail(
        `(GB9.1d) the denylist parser must extract NODE_OPTIONS from the real tsserver launcher; got ` +
          `${JSON.stringify(parsedDenylist)}`,
      );
      ok = false;
    }
    for (const [label, source] of [
      ["a source without the const", "fn main() {}\n"],
      ["a const with no string literals", "pub const CHILD_PROCESS_ENV_DENYLIST: &[&str] = &[];\n"],
      ["a non-string input", 42],
      // DECLARATION-BOUNDED. A commented-out declaration is DEAD CODE, and latching onto one reintroduces
      // exactly the drift that reading the live Rust const exists to prevent — silently, with a plausible
      // list. A bare mention (the `for var in CHILD_PROCESS_ENV_DENYLIST` loop) must not latch either.
      [
        "a line-commented declaration only",
        '// pub const CHILD_PROCESS_ENV_DENYLIST: &[&str] = &["UNRELATED"];\n',
      ],
      [
        "a block-commented declaration only",
        '/* pub const CHILD_PROCESS_ENV_DENYLIST: &[&str] = &["UNRELATED"]; */\n',
      ],
      [
        "a bare mention with an unrelated array nearby",
        'for var in CHILD_PROCESS_ENV_DENYLIST {\n    let other = ["UNRELATED"];\n}\n',
      ],
    ]) {
      if (parseTsserverEnvDenylist(source) !== null) {
        fail(`(GB9.1d) ${label} must parse to null (fail-closed), not to a usable list`);
        ok = false;
      }
    }
    // The decisive case: a commented-out decoy EARLIER in the file must not win over the real declaration
    // later in it. Pre-fix this returned ["UNRELATED"].
    const decoyed = parseTsserverEnvDenylist(
      '// pub const CHILD_PROCESS_ENV_DENYLIST: &[&str] = &["UNRELATED"];\n' + realLauncher,
    );
    if (!decoyed || decoyed.includes("UNRELATED") || !decoyed.includes("NODE_OPTIONS")) {
      fail(
        `(GB9.1d) a commented-out decoy before the real declaration must not win; got ` +
          `${JSON.stringify(decoyed)}`,
      );
      ok = false;
    }

    // The probe budget must be the gate's OWN remaining wallclock, clamped — an independent constant can
    // outlive the `--timeout` deadline the probe sits inside, which is not a bound.
    for (const [label, deadline, now, want] of [
      ["a long deadline clamps to the cap", 10_000_000, 0, BUILD_PREREQUISITE_PROBE_MAX_MS],
      ["a short deadline shortens the probe", 5_000, 0, 5_000],
      // NO FLOOR. A floor let an expired deadline buy the probe time to hold the single-flight mutex past
      // the gate's own wallclock limit; the budget must go non-positive and the probe must then refuse.
      ["an exhausted deadline yields zero", 10_000, 10_000, 0],
      ["an already-passed deadline goes negative", 0, 10_000, -10_000],
    ]) {
      const got = probeBudgetMs(deadline, now);
      if (got !== want) {
        fail(`(GB9.1e) ${label}: probeBudgetMs(${deadline}, ${now}) = ${got}, expected ${want}`);
        ok = false;
      }
    }
    // …and a non-positive budget must refuse WITHOUT SPAWNING. Launching would hold the mutex past the
    // deadline, and it would also be UNBOUNDED: Node applies `spawnSync`'s timeout only when it is `> 0`,
    // so a 0/negative value silently disables it — an expired deadline becoming an unlimited probe.
    for (const budget of [0, -1, -10_000]) {
      let spawnAttempted = false;
      const refused = runBuildPrerequisiteLoadProbe({
        repoRoot: "/synthetic",
        readFileFn: fakeLauncher,
        timeoutMs: budget,
        spawnFn: () => {
          spawnAttempted = true;
          return { status: 0, stdout: "", stderr: "" };
        },
      });
      if (refused.loaded || refused.reason !== "timeout" || spawnAttempted) {
        fail(
          `(GB9.1e) a ${budget}ms budget must refuse as a timeout WITHOUT spawning; got ` +
            `loaded=${refused.loaded} reason=${refused.reason} spawnAttempted=${spawnAttempted}`,
        );
        ok = false;
      }
    }

    // ---- Legs 2-6: the REAL production CLI against a synthetic repo root ----
    let gitAvailable = true;
    const synthRoot = mkdtempSync(join(tmpdir(), "gate-selftest-prereq-"));
    registerClean(synthRoot);
    try {
      execFileSync("git", ["init", "-q", synthRoot], { stdio: "ignore" });
    } catch {
      gitAvailable = false;
    }

    if (!gitAvailable) {
      // TRUE skip (counted in SKIP, never in PASS): without git the production CLI cannot resolve a
      // synthetic repo root, so the end-to-end legs cannot run. The in-process leg above still ran.
      skip(
        "(GB9.2-6) end-to-end build-prerequisite legs SKIPPED — `git init` is unavailable, so the " +
          "production CLI cannot resolve a synthetic repo root",
      );
    } else {
      const synthScripts = join(synthRoot, "scripts");
      mkdirSync(synthScripts, { recursive: true });
      // A BYTE-COPY of the production CLI and its internals — the real code path, rooted elsewhere.
      for (const name of ["gate.mjs", "gate-internals.mjs"]) {
        writeFileSync(join(synthScripts, name), readFileSync(join(SELFTEST_DIR, name)));
      }
      const synthGate = join(synthScripts, "gate.mjs");
      const synthTarget = join(synthRoot, "target", "gate-runner");
      const commonGateArgs = ["--timeout", "120s", "--stall", "60s", "--target-dir", synthTarget];
      const gateArgs = [...commonGateArgs, "--build-jobs", "7", "--test-threads", "9"];
      const memoryTierGateArgs = [
        ...commonGateArgs,
        "--memory-limit",
        "12GiB",
        "--test-threads",
        "9",
      ];
      const gateEnv = { VERTER_GATE_LOCK: join(synthRoot, "gate.lock.d") };

      // Freshness shims, so the freshness preflight resolves "already-present" and never attempts a
      // `pnpm install` inside the synthetic root. Both the POSIX (extensionless) and the Windows (.CMD)
      // spellings are written so the leg is deterministic on either host.
      const synthBin = join(synthRoot, "node_modules", ".bin");
      mkdirSync(synthBin, { recursive: true });
      for (const tool of ["buf", "oxfmt"]) {
        writeFileSync(join(synthBin, tool), "");
        writeFileSync(join(synthBin, `${tool}.CMD`), "");
      }

      // Oracle-cache shim: this scenario exercises the BUILD-prerequisite preflight, not the (separate)
      // oracle-cache one — but the real production `gate.mjs` byte-copy runs BOTH in sequence, so leg 6
      // ("everything built") would otherwise fail the oracle-cache preflight here (no real
      // `.oracle-npm-cache` in a synthetic root) and never reach the freshness-preflight line this
      // scenario asserts on. Plant a trivial always-succeeding `ensureOracleDomain` at the exact module
      // path the real preflight probe imports, so it resolves SATISFIED without any real npm/network work
      // — this is a stand-in for the ORACLE-CACHE preflight, proven separately and for real by GB11.
      const oracleCacheStub = join(synthRoot, ...ORACLE_CACHE_PROBE_MODULE_SEGMENTS);
      mkdirSync(dirname(oracleCacheStub), { recursive: true });
      writeFileSync(
        oracleCacheStub,
        "export function ensureOracleDomain(framework) {\n" +
          '  return { installDir: "/synthetic-oracle/" + framework, realizedClosureSha256: "stub" };\n' +
          "}\n",
      );

      // This GB9 synthetic root is scoped to build-prerequisite discrimination. Give the newly-required
      // harness-smoke phase a receipt-only stand-in so the successful GB9 leg can continue to the freshness
      // marker it owns; GB15 executes both REAL harness modes through the production CLI.
      const synthSmoke = join(
        synthRoot,
        "packages",
        "framework-conformance-harness",
        "bin",
        "gate-smoke.mjs",
      );
      mkdirSync(dirname(synthSmoke), { recursive: true });
      writeFileSync(
        synthSmoke,
        'const mode = process.argv[2];\nprocess.stdout.write(JSON.stringify({ schema: "verter-harness-smoke/v1", mode, ok: true }));\n',
      );

      // The MINIATURE package graph. Every edge the real chain has, and nothing else:
      //   <probe dir>/package.json  --main-->  <plugin>/dist/index.js
      //   <plugin>/dist/index.js    requires   ./helpers/carrierStore   (an EMITTED sibling)
      //   <plugin>/dist/index.js    requires   @verter/language-shared  (via <plugin>/node_modules)
      //   <language-shared>/dist/index.js requires ./carrier/store      (an EMITTED sibling)
      // `main` fields are ABSOLUTE so no symlink is needed (portable to hosts that refuse them); Node
      // resolves `main` with path.resolve, so an absolute value is honoured.
      const pluginPkg = join(synthRoot, "packages", "typescript-plugin");
      const sharedPkg = join(synthRoot, "packages", "language-shared");
      const pluginEntry = join(pluginPkg, "dist", "index.js");
      const pluginHelper = join(pluginPkg, "dist", "helpers", "carrierStore.js");
      const sharedEntry = join(sharedPkg, "dist", "index.js");
      const sharedSibling = join(sharedPkg, "dist", "carrier", "store.js");
      const probeDir = join(synthRoot, ...BUILD_PREREQUISITE_PROBE_SEGMENTS);
      const sharedLink = join(pluginPkg, "node_modules", "@verter", "language-shared");

      const writeFile = (p, body) => {
        mkdirSync(dirname(p), { recursive: true });
        writeFileSync(p, body);
      };
      const isFile = (p) => {
        try {
          return statSync(p).isFile();
        } catch {
          return false;
        }
      };
      // The probe resolves the environment tsserver runs under from the Rust launcher, so the miniature
      // carries a copy: without it every leg would fail closed as `environment-unknown` and leg 6 could
      // never reach SATISFIED — the legs would still refuse, but for the wrong reason, which is a
      // vacuous version of this scenario.
      writeFile(
        join(synthRoot, ...TSSERVER_ENV_DENYLIST_SOURCE_SEGMENTS),
        readFileSync(join(REPO_REALPATH, ...TSSERVER_ENV_DENYLIST_SOURCE_SEGMENTS), "utf8"),
      );
      // The static scaffolding: manifests and the emitted siblings that never move between legs.
      writeFile(
        join(probeDir, "package.json"),
        JSON.stringify({ name: "@verter/typescript-plugin", main: pluginEntry }),
      );
      writeFile(
        join(pluginPkg, "package.json"),
        JSON.stringify({ name: "@verter/typescript-plugin", main: pluginEntry }),
      );
      writeFile(
        join(sharedLink, "package.json"),
        JSON.stringify({ name: "@verter/language-shared", main: sharedEntry }),
      );
      writeFile(
        join(sharedPkg, "package.json"),
        JSON.stringify({ name: "@verter/language-shared", main: sharedEntry }),
      );

      // The four EMITTED files a build produces. `plant(state)` installs exactly the requested subset and
      // PROVES the resulting tree by stat-ing all four — a plant that silently failed to apply would
      // otherwise be indistinguishable from correct behavior.
      const emitted = [
        [
          pluginEntry,
          'require("./helpers/carrierStore");\nrequire("@verter/language-shared");\nmodule.exports = function init() {};\n',
        ],
        [pluginHelper, "module.exports = { carrierStore: true };\n"],
        [sharedEntry, 'require("./carrier/store");\nmodule.exports = { languageShared: true };\n'],
        [sharedSibling, "module.exports = { store: true };\n"],
      ];
      const plant = (label, present) => {
        for (const [p, body] of emitted) {
          if (present.includes(p)) writeFile(p, body);
          else rmSync(p, { force: true });
        }
        for (const [p] of emitted) {
          const want = present.includes(p);
          if (isFile(p) !== want) {
            throw new Error(
              `(GB9) plant "${label}" did not apply: ${p} should be ${want ? "present" : "absent"}`,
            );
          }
        }
      };
      // Re-stat AFTER the CLI returns: the verdict must have been produced against the tree we planted.
      const assertUnchanged = (label, present) => {
        for (const [p] of emitted) {
          const want = present.includes(p);
          if (isFile(p) !== want) {
            fail(
              `(GB9.${label}) the tree changed under the run: ${p} is no longer ${want ? "present" : "absent"}`,
            );
            ok = false;
          }
        }
      };

      const allEmitted = emitted.map(([p]) => p);
      const refusalLegs = [
        ["2", "nothing built", []],
        ["3", "the plugin entry missing", allEmitted.filter((p) => p !== pluginEntry)],
        [
          "4",
          "language-shared missing (the REVERSE single-missing direction)",
          allEmitted.filter((p) => p !== sharedEntry),
        ],
        [
          "5",
          "a transitively-required HELPER missing while BOTH entries are present",
          allEmitted.filter((p) => p !== pluginHelper),
        ],
      ];
      for (const [id, label, present] of refusalLegs) {
        plant(label, present);
        const run = runGateCapture(synthGate, id === "3" ? memoryTierGateArgs : gateArgs, gateEnv);
        if (run.code !== EXIT_USAGE || !run.out.includes(BUILD_PREREQUISITE_MARKER)) {
          fail(
            `(GB9.${id}) with ${label} the gate must FAIL SETUP (127) carrying the marker; got ` +
              `${run.code}\n${run.out}`,
          );
          ok = false;
        }
        if (
          id === "3" &&
          !run.out.includes(
            "resource ceiling: cargo build jobs=8, test threads=9, active child-tree RSS=12.00 GiB",
          )
        ) {
          fail(
            `(GB9.3) an omitted build-job value must follow the explicit 12-GiB memory tier while the ` +
              `test-thread override remains independent; output was:\n${run.out}`,
          );
          ok = false;
        }
        if (!run.out.includes(probeDir) || !run.out.includes(BUILD_PREREQUISITE_COMMAND)) {
          fail(`(GB9.${id}) the refusal must name the probe target and the producer command`);
          ok = false;
        }
        if (
          id === "2" &&
          !run.out.includes("resource ceiling: cargo build jobs=7, test threads=9,")
        ) {
          fail(
            `(GB9.2) the real production CLI must preserve explicit resource overrides before its ` +
              `cargo-free prerequisite refusal; output was:\n${run.out}`,
          );
          ok = false;
        }
        // The refusal must be about a MISSING MODULE, not about the probe being unable to answer. Without
        // this, a miniature that lost its tsserver-launcher copy would refuse as `environment-unknown` and
        // every leg above would still pass while testing nothing about missing artifacts.
        if (!run.out.includes("MODULE_NOT_FOUND")) {
          fail(
            `(GB9.${id}) the refusal must report MODULE_NOT_FOUND (a missing artifact), not a probe that ` +
              `could not answer:\n${run.out}`,
          );
          ok = false;
        }
        // ORDERING, the load-bearing half: the refusal precedes the freshness preflight (whose `pnpm
        // install` is exactly what turns the silent-skip state into the 64-failure state) and any cargo.
        if (
          run.out.includes("freshness-tooling preflight:") ||
          run.out.includes("archiving workspace test universe")
        ) {
          fail(
            `(GB9.${id}) the refusal must run BEFORE the freshness preflight and before the archive ` +
              `build; the run reached one of them:\n${run.out}`,
          );
          ok = false;
        }
        assertUnchanged(id, present);
      }

      // Leg 6 — EVERYTHING BUILT. The check must pass and the run must PROCEED (not stop quietly).
      plant("everything built", allEmitted);
      const allThere = runGateCapture(synthGate, gateArgs, gateEnv);
      if (allThere.out.includes(BUILD_PREREQUISITE_MARKER)) {
        fail(`(GB9.6) with the whole closure loadable the refusal must NOT fire:\n${allThere.out}`);
        ok = false;
      }
      if (!allThere.out.includes("build-prerequisite preflight: SATISFIED")) {
        fail(`(GB9.6) the satisfied preflight must be reported:\n${allThere.out}`);
        ok = false;
      }
      if (!allThere.out.includes("freshness-tooling preflight:")) {
        fail(
          `(GB9.6) a satisfied build-prerequisite preflight must let the gate PROCEED into the freshness ` +
            `preflight; it did not:\n${allThere.out}`,
        );
        ok = false;
      }
      assertUnchanged("6", allEmitted);

      // @ai-generated - Drives the real production CLI to prove startup reporting owns a deadline that
      // is separate from the canonical build/test timeout and cannot replace the canonical exit verdict.
      // The first Cargo capability probe is a real SIGTERM-trapping process. It must consume the ONE
      // aggregate startup-reporting allowance, be hard-killed and reaped, and leave every later startup
      // probe unavailable without spawning. Only AFTER collectStartup settles may the production gate
      // establish its canonical --timeout deadline and reach the deliberately failing archive build.
      // Deleting the startup deadline launches all three 2s probe plants; moving the canonical deadline
      // back above collectStartup exhausts it before the build-prerequisite probe. Both mutations fail.
      const telemetryBin = join(synthRoot, "telemetry-bin");
      mkdirSync(telemetryBin, { recursive: true });
      const fakeCargo = join(telemetryBin, IS_WINDOWS ? "cargo.exe" : "cargo");
      copyFileSync(process.execPath, fakeCargo);
      if (!IS_WINDOWS) chmodSync(fakeCargo, 0o755);
      const telemetryProbeLog = join(synthRoot, "telemetry-probes.log");
      const telemetryProbePid = join(synthRoot, "telemetry-probe.pid");
      const archiveStarted = join(synthRoot, "archive-started.marker");
      const trappingProbeBody =
        'const { appendFileSync, writeFileSync } = require("node:fs");\n' +
        `const probeLog = ${JSON.stringify(telemetryProbeLog)};\n` +
        `const pidFile = ${JSON.stringify(telemetryProbePid)};\n` +
        `const archiveMarker = ${JSON.stringify(archiveStarted)};\n` +
        "const args = process.argv.slice(2);\n" +
        'if (args.includes("--help")) {\n' +
        '  appendFileSync(probeLog, `PROBE ${Date.now()} ${args.join(" ")}\\n`);\n' +
        "  writeFileSync(pidFile, String(process.pid));\n" +
        '  process.on("SIGTERM", () => {});\n' +
        "  setInterval(() => {}, 1000);\n" +
        "} else {\n" +
        '  appendFileSync(probeLog, `BUILD ${Date.now()} ${args.join(" ")}\\n`);\n' +
        '  writeFileSync(archiveMarker, "started\\n");\n' +
        "  process.exit(9);\n" +
        "}\n";
      writeFile(join(synthRoot, "nextest"), trappingProbeBody);
      writeFile(join(synthRoot, "check"), trappingProbeBody);
      // This leg owns startup telemetry ordering, so keep the already-separately-tested Vue macro oracle
      // checks as successful production-path stand-ins and let the run reach the archive discriminator.
      writeFile(
        join(synthRoot, "scripts", "gen-vue-macro-runtime-oracle.mjs"),
        "process.exit(0);\n",
      );
      writeFile(
        join(synthRoot, "scripts", "vue-macro-runtime-oracle", "oracle.test.mjs"),
        "process.exit(0);\n",
      );
      rmSync(telemetryProbeLog, { force: true });
      rmSync(telemetryProbePid, { force: true });
      rmSync(archiveStarted, { force: true });
      const telemetryPath = `${telemetryBin}${pathDelimiterFor(IS_WINDOWS)}${process.env.PATH || ""}`;
      const telemetryStartedAtMs = Date.now();
      const telemetryRun = runGateCapture(
        synthGate,
        // The trapping startup probe consumes >2.5s, so a wrongly early canonical deadline still expires
        // before the archive marker. Two seconds leaves deterministic headroom for the synthetic
        // prerequisite/oracle/harness front half after the correctly delayed deadline starts; the previous
        // 1s bound raced that legitimate front half on a loaded Windows host and produced rc=124 instead of
        // reaching the deliberate archive exit 9.
        ["--timeout", "2s", "--stall", "60s", "--target-dir", synthTarget],
        {
          ...gateEnv,
          PATH: telemetryPath,
        },
        { cwd: synthRoot },
      );
      const telemetryElapsedMs = Date.now() - telemetryStartedAtMs;
      const telemetryProbeLines = existsSync(telemetryProbeLog)
        ? readFileSync(telemetryProbeLog, "utf8").trim().split(/\r?\n/).filter(Boolean)
        : [];
      const probeStarts = telemetryProbeLines.filter((line) => line.startsWith("PROBE "));
      const buildStarts = telemetryProbeLines.filter((line) => line.startsWith("BUILD "));
      const firstProbeAtMs = Number.parseInt(probeStarts[0]?.split(" ")[1] || "", 10);
      const firstBuildAtMs = Number.parseInt(buildStarts[0]?.split(" ")[1] || "", 10);
      const startupToBuildMs = firstBuildAtMs - firstProbeAtMs;
      const trappedPid = existsSync(telemetryProbePid)
        ? Number.parseInt(readFileSync(telemetryProbePid, "utf8"), 10)
        : null;
      note(
        `(GB9.7) startup telemetry isolation => rc=${telemetryRun.code}, elapsed=${telemetryElapsedMs}ms, ` +
          `probeStarts=${probeStarts.length}, buildStarts=${buildStarts.length}, ` +
          `startupToBuild=${startupToBuildMs}ms, trappedPid=${trappedPid}`,
      );
      if (
        telemetryRun.code !== EXIT_FAIL ||
        !telemetryRun.out.includes("workspace did not compile")
      ) {
        fail(
          `(GB9.7) unavailable startup telemetry must not replace the deliberate archive-build verdict ` +
            `(exit 1); got rc=${telemetryRun.code}:\n${telemetryRun.out}`,
        );
        ok = false;
      }
      if (!existsSync(archiveStarted) || buildStarts.length !== 1) {
        fail(
          `(GB9.7) startup reporting must consume ZERO milliseconds from the canonical --timeout: the ` +
            `real gate must still reach exactly one archive build; marker=${existsSync(archiveStarted)} ` +
            `buildStarts=${JSON.stringify(buildStarts)}`,
        );
        ok = false;
      }
      if (probeStarts.length !== 1 || !probeStarts[0].includes("archive --help")) {
        fail(
          `(GB9.7) all startup probes must share one hard aggregate deadline: only the first trapping ` +
            `probe may spawn; got ${JSON.stringify(probeStarts)}`,
        );
        ok = false;
      }
      if ((telemetryRun.out.match(/GATE TELEMETRY WARNING:/g) || []).length < 2) {
        fail(
          `(GB9.7) timed-out and then unavailable startup probes must remain report-only warnings:\n` +
            telemetryRun.out,
        );
        ok = false;
      }
      if (!Number.isFinite(startupToBuildMs) || startupToBuildMs >= 5_500) {
        fail(
          `(GB9.7) aggregate startup reporting exceeded its hard bound before the canonical build ` +
            `started (${startupToBuildMs}ms); ` +
            `three independent 2s probe budgets were likely serialized`,
        );
        ok = false;
      }
      if (!Number.isInteger(trappedPid) || pidAlive(trappedPid)) {
        fail(
          `(GB9.7) the SIGTERM-trapping startup telemetry probe must be hard-killed and leave no ` +
            `survivor; pid=${trappedPid}`,
        );
        ok = false;
      }
    }

    if (ok) {
      pass(
        "(GB9) BUILD-PREREQUISITE PREFLIGHT: the gate refuses, loudly and as its FIRST step, when the " +
          "tsserver plugin the real-provider suites load cannot be loaded from this tree — naming the " +
          "probe target, the load error and the producer command (exit 127), instead of running the suite " +
          "and reporting ~64 opaque `TS2307: Cannot find module './Comp.vue'` failures. The oracle is a " +
          "REAL LOAD, so the discriminator a stat-based check FAILS is covered: both entries present with " +
          "one emitted HELPER missing is still a refusal. Six directions through the REAL production CLI " +
          "on a synthetic miniature of the package graph — nothing built / plugin entry missing / " +
          "language-shared missing / helper missing => 127 before the freshness preflight and before " +
          "cargo; everything built => SATISFIED and the run proceeds — plus every fail-closed probe shape " +
          "(spawn error, signal, timeout, unparseable output) in-process. A seventh real-production leg " +
          "proves slow/trapping/unavailable startup telemetry is aggregate-bounded, reaped, report-only, " +
          "and consumes none of the canonical build/test timeout. Every plant is stat-proven applied and " +
          "re-stated after the run.",
      );
    }
  }

  // --------------------------------------------------------------------------------------------------
  // (GB10) MEMORY-CEILING SELF-TEST WIRING — scripts/gate-memory-selftest.mjs asserts parseMemorySize,
  // deriveGateResourceLimits, buildCargoEnv's job-cap clamp, the POSIX/Windows process-table RSS parsers,
  // and runContainedStep's real MEMORY / MEMORY_MONITOR reap behavior (including the MEMORY-vs-TIMEOUT/
  // STALL reap-grace discrimination). Run it here, as a subprocess, so it rides the SAME canonical
  // self-test entrypoint (`node scripts/gate-selftest.mjs`) instead of sitting unreferenced by anything.
  // --------------------------------------------------------------------------------------------------
  {
    const r = spawnSync(process.execPath, [MEMORY_SELFTEST], {
      env: process.env,
      encoding: "utf8",
    });
    if (r.status === 0) {
      pass(
        "(GB10) gate-memory-selftest.mjs: memory-ceiling parsing/derivation/reap assertions all pass, " +
          "including the MEMORY-reap-escalates-faster-than-TIMEOUT/STALL timing discrimination.",
      );
    } else {
      fail(
        `(GB10) gate-memory-selftest.mjs exited ${r.status === null ? `signal ${r.signal}` : r.status} — ` +
          `memory-ceiling assertions failed:\n${r.stdout || ""}${r.stderr || ""}`,
      );
    }
  }

  // --------------------------------------------------------------------------------------------------
  // (GB18) MEASURED RESOURCE + PREPARE RUNTIME ENVIRONMENT — cargo-free behavioral authorities for the
  // memory-tiered build cap / independent 12-thread cap, immutable nextest serialization lanes, and the
  // Windows proc-macro first-launch
  // environment. Proc-macro suites are REAL test suites (41 tests in the measured archive), so the helper
  // must keep them warmable and prepend nextest's listed host libdir rather than filtering/tolerating them.
  // --------------------------------------------------------------------------------------------------
  process.stderr.write("\n(GB18) MEASURED RESOURCE + WINDOWS PREPARE RUNTIME ENVIRONMENT\n");
  {
    let ok = true;
    const hostLibdir = "C:\\Rust\\host-lib";
    const rustBuildMeta = {
      platforms: {
        host: { libdir: { status: "available", path: hostLibdir } },
      },
    };
    const procMacroIds = [
      "verter_no_storedspan_derive",
      "verter_no_typeexpr_derive",
      "verter_session_oracle_macro",
      "future_derive",
    ];
    for (const binaryId of procMacroIds) {
      const baseEnv = {
        PaTh: "E:\\decoy",
        PATH: "D:\\tools\\bin",
        KEEP: `keep-${binaryId}`,
      };
      const result = buildPrepareWarmSpawnEnv({
        suite: {
          "binary-id": binaryId,
          kind: "proc-macro",
          "build-platform": "host",
        },
        rustBuildMeta,
        baseEnv,
        windows: true,
      });
      const pathKeys = result.ok
        ? Object.keys(result.env).filter((key) => key.toUpperCase() === "PATH")
        : [];
      if (
        !result.ok ||
        result.env.PATH !== `${hostLibdir};D:\\tools\\bin` ||
        pathKeys.length !== 1 ||
        pathKeys[0] !== "PATH" ||
        result.env.KEEP !== `keep-${binaryId}`
      ) {
        fail(
          `(GB18.1) ${binaryId} must remain warmable with one canonical PATH, metadata host libdir ` +
            `prepended exactly once; got ${JSON.stringify(result)}`,
        );
        ok = false;
      }
    }

    const deduplicated = buildPrepareWarmSpawnEnv({
      suite: {
        "binary-id": "future_derive",
        kind: "proc-macro",
        "build-platform": "host",
      },
      rustBuildMeta,
      baseEnv: { PATH: "c:\\rust\\HOST-LIB;D:\\tools\\bin", KEEP: "yes" },
      windows: true,
    });
    if (
      !deduplicated.ok ||
      deduplicated.env.PATH !== `${hostLibdir};D:\\tools\\bin` ||
      deduplicated.env.KEEP !== "yes"
    ) {
      fail(
        `(GB18.2) metadata libdir must be first and de-duplicated case-insensitively; got ` +
          JSON.stringify(deduplicated),
      );
      ok = false;
    }

    const ordinaryBase = { PATH: "D:\\tools\\bin", KEEP: "ordinary" };
    const ordinary = buildPrepareWarmSpawnEnv({
      suite: { "binary-id": "ordinary", kind: "lib", "build-platform": "target" },
      rustBuildMeta,
      baseEnv: ordinaryBase,
      windows: true,
    });
    const posixProcMacro = buildPrepareWarmSpawnEnv({
      suite: {
        "binary-id": "future_derive",
        kind: "proc-macro",
        "build-platform": "host",
      },
      rustBuildMeta,
      baseEnv: ordinaryBase,
      windows: false,
    });
    if (
      !ordinary.ok ||
      ordinary.env !== ordinaryBase ||
      !posixProcMacro.ok ||
      posixProcMacro.env !== ordinaryBase
    ) {
      fail(
        `(GB18.3) non-proc-macro and non-Windows warm environments must remain byte-owned by the ` +
          `constructed base env; got ordinary=${JSON.stringify(ordinary)} posix=${JSON.stringify(posixProcMacro)}`,
      );
      ok = false;
    }

    const invalidMetadata = [
      ["missing-platform", { platforms: {} }],
      [
        "unavailable",
        { platforms: { host: { libdir: { status: "unavailable", path: hostLibdir } } } },
      ],
      ["empty", { platforms: { host: { libdir: { status: "available", path: "" } } } }],
      [
        "relative",
        { platforms: { host: { libdir: { status: "available", path: "relative\\lib" } } } },
      ],
      [
        "drive-relative",
        { platforms: { host: { libdir: { status: "available", path: "C:relative" } } } },
      ],
      [
        "root-relative",
        { platforms: { host: { libdir: { status: "available", path: "\\relative" } } } },
      ],
    ];
    for (const [label, meta] of invalidMetadata) {
      const result = buildPrepareWarmSpawnEnv({
        suite: {
          "binary-id": `invalid-${label}`,
          kind: "proc-macro",
          "build-platform": "host",
        },
        rustBuildMeta: meta,
        baseEnv: ordinaryBase,
        windows: true,
      });
      if (result.ok || !result.detail?.includes(`invalid-${label}`) || "env" in result) {
        fail(
          `(GB18.4) malformed ${label} proc-macro metadata must fail closed before spawn with suite ` +
            `attribution; got ${JSON.stringify(result)}`,
        );
        ok = false;
      }
    }

    const childSpec = buildPrepareWarmSpawnEnv({
      suite: {
        "binary-id": "child-receipt",
        kind: "proc-macro",
        "build-platform": "host",
      },
      rustBuildMeta,
      baseEnv: { ...process.env, PaTh: "E:\\decoy", PATH: "D:\\tools\\bin" },
      windows: true,
    });
    const expectedChildPath = `${hostLibdir};D:\\tools\\bin`;
    const child = childSpec.ok
      ? spawnSync(
          process.execPath,
          ["-e", `if (process.env.PATH !== ${JSON.stringify(expectedChildPath)}) process.exit(23)`],
          { env: childSpec.env, encoding: "utf8" },
        )
      : { status: null, signal: null };
    if (!childSpec.ok || child.status !== 0) {
      fail(
        `(GB18.5) a real cargo-free child must receive the helper-produced PATH exactly; ` +
          `spec=${JSON.stringify(childSpec)} status=${child.status} signal=${child.signal}`,
      );
      ok = false;
    }

    const cleanSuccesses = [
      classifyPrepareWarmResult({ status: 0, signal: null, error: null }),
      classifyPrepareWarmResult({ status: 0 }),
    ];
    const contradictoryZeroFailures = [
      [classifyPrepareWarmResult({ status: 0, signal: "SIGTERM" }), "exit 0; signal SIGTERM"],
      [
        classifyPrepareWarmResult({ status: 0, signal: null, error: new Error("boom") }),
        "exit 0; spawn error boom",
      ],
    ];
    const strictFailures = [
      classifyPrepareWarmResult({ status: 3221225781, signal: null }),
      classifyPrepareWarmResult({ status: 1, signal: null }),
      classifyPrepareWarmResult({ status: null, signal: null }),
      classifyPrepareWarmResult({ status: null, signal: "SIGTERM" }),
      classifyPrepareWarmResult(undefined),
    ];
    if (
      cleanSuccesses.some((result) => !result.ok) ||
      contradictoryZeroFailures.some(
        ([result, expectedDetail]) => result.ok || result.detail !== expectedDetail,
      ) ||
      strictFailures.some((result) => result.ok || !result.detail)
    ) {
      fail(
        `(GB18.6) only exact status 0 with no signal/spawn error may count as warmed; ` +
          `successes=${JSON.stringify(cleanSuccesses)} contradictory=${JSON.stringify(
            contradictoryZeroFailures,
          )} failures=${JSON.stringify(strictFailures)}`,
      );
      ok = false;
    }

    const nextestToml = readFileSync(join(REPO_REALPATH, ".config", "nextest.toml"), "utf8");
    const count = (pattern) => (nextestToml.match(pattern) || []).length;
    const expectedSharedFilter =
      "test(/shared_provider_live::(shared_provider_serves_real_vue_macro_carrier|" +
      "shared_provider_serves_dual_claimant_carrier_with_real_types|" +
      "shared_provider_carrier_never_leaks_to_editor|" +
      "shared_provider_reconnect_mints_fresh_engine_no_split_brain|" +
      "composite_overlays_shared_diagnostics_via_live_resolver|" +
      "composite_successful_shared_route_never_activates_managed_fallback|" +
      "composite_shared_template_answers_are_typed_single_project|" +
      "composite_shared_template_answers_are_typed_monorepo_nested_leaf)$/)";
    const overrideRows = [];
    const overrideRe =
      /\[\[profile\.(default|ci)\.overrides\]\]\s*\r?\n([\s\S]*?)(?=\r?\n\[\[|\r?\n\[(?!\[)|$)/g;
    for (const match of nextestToml.matchAll(overrideRe)) {
      const filter = /^filter\s*=\s*'([^']+)'\s*$/m.exec(match[2])?.[1] || null;
      const testGroup = /^test-group\s*=\s*'([^']+)'\s*$/m.exec(match[2])?.[1] || null;
      const platform = /^platform\s*=\s*'([^']+)'\s*$/m.exec(match[2])?.[1] || null;
      const slowTimeout = /^slow-timeout\s*=\s*(\{[^\r\n]+\})\s*$/m.exec(match[2])?.[1] || null;
      overrideRows.push({ profile: match[1], filter, testGroup, platform, slowTimeout });
    }
    const exactOverrideCount = (profile, filter, testGroup, platform = null, slowTimeout = null) =>
      overrideRows.filter(
        (row) =>
          row.profile === profile &&
          row.filter === filter &&
          row.testGroup === testGroup &&
          row.platform === platform &&
          row.slowTimeout === slowTimeout,
      ).length;
    const configOk =
      count(/^shared-provider-live\s*=\s*\{\s*max-threads\s*=\s*1\s*\}\s*$/gm) === 0 &&
      count(/^lsp-server-unit\s*=\s*\{\s*max-threads\s*=\s*1\s*\}\s*$/gm) === 0 &&
      exactOverrideCount(
        "default",
        expectedSharedFilter,
        "shared-provider-live",
        "cfg(windows)",
      ) === 0 &&
      exactOverrideCount("ci", expectedSharedFilter, "shared-provider-live", "cfg(windows)") ===
        0 &&
      exactOverrideCount(
        "default",
        expectedSharedFilter,
        null,
        null,
        '{ period = "60s", terminate-after = 6 }',
      ) === 1 &&
      exactOverrideCount(
        "ci",
        expectedSharedFilter,
        null,
        null,
        '{ period = "60s", terminate-after = 6 }',
      ) === 1 &&
      exactOverrideCount(
        "default",
        "test(/^server::server_tests::/)",
        "lsp-server-unit",
        "cfg(windows)",
      ) === 0 &&
      exactOverrideCount(
        "ci",
        "test(/^server::server_tests::/)",
        "lsp-server-unit",
        "cfg(windows)",
      ) === 0 &&
      exactOverrideCount(
        "default",
        "test(/^cases::g_compile::compile_fail::/)",
        null,
        null,
        '{ period = "120s", terminate-after = 3 }',
      ) === 1 &&
      exactOverrideCount(
        "ci",
        "test(/^cases::g_compile::compile_fail::/)",
        null,
        null,
        '{ period = "120s", terminate-after = 3 }',
      ) === 1 &&
      exactOverrideCount(
        "default",
        "test(/^cases::resolver_observation_compile_fail::/)",
        null,
        null,
        '{ period = "120s", terminate-after = 3 }',
      ) === 1 &&
      exactOverrideCount(
        "ci",
        "test(/^cases::resolver_observation_compile_fail::/)",
        null,
        null,
        '{ period = "120s", terminate-after = 3 }',
      ) === 1;
    if (!configOk) {
      fail(
        `(GB18.7) full CI capacity forbids both Windows-only serialized nextest groups and their ` +
          `default/ci assignments while preserving the all-platform shared-provider timeout and every ` +
          `platform-neutral trybuild timeout override; parsed rows=` +
          JSON.stringify(overrideRows),
      );
      ok = false;
    }

    const gateSource = readFileSync(join(SELFTEST_DIR, "gate.mjs"), "utf8");
    const prepareStart = gateSource.indexOf("async function runPrepare(ctx)");
    const prepareEnd = gateSource.indexOf("\nasync function ", prepareStart + 1);
    const prepareSource =
      prepareStart >= 0 && prepareEnd > prepareStart
        ? gateSource.slice(prepareStart, prepareEnd)
        : "";
    if (
      !prepareSource.includes("for (const s of suites)") ||
      prepareSource.includes("suites.filter(") ||
      !prepareSource.includes("const warmEnvResult = buildPrepareWarmSpawnEnv({") ||
      !prepareSource.includes("baseEnv: ctx.cargoEnv") ||
      !prepareSource.includes("if (!warmEnvResult.ok)") ||
      !prepareSource.includes("warmFailures++") ||
      !prepareSource.includes("env: warmEnv")
    ) {
      fail(
        `(GB18.8) production runPrepare must keep the full suite loop, fail closed on warm-env errors, ` +
          `and pass the helper-produced env to the real spawnSync; bounded source was:\n${prepareSource}`,
      );
      ok = false;
    }

    const laneSelftest = spawnSync(process.execPath, [LANE_SELFTEST], {
      env: process.env,
      encoding: "utf8",
      timeout: 120_000,
      maxBuffer: 64 * 1024 * 1024,
    });
    if (laneSelftest.status !== 0 || laneSelftest.error) {
      fail(
        `(GB18.9) gate-lane-selftest.mjs must prove multi-root accounting, one aggregate watchdog, ` +
          `admission/cancellation fencing, ABA-safe completion, and exact native tree cleanup; ` +
          `status=${laneSelftest.status} signal=${laneSelftest.signal} error=${laneSelftest.error?.message || "none"}\n` +
          `${laneSelftest.stdout || ""}${laneSelftest.stderr || ""}`,
      );
      ok = false;
    }

    const supervisorFactoryCount = (gateSource.match(/createGateRunSupervisor\(\{/g) || []).length;
    const productionRunStepCount = (gateSource.match(/\.supervisor\.runStep\(/g) || []).length;
    const teardownStart = gateSource.indexOf("const teardown = () => {");
    const teardownEnd = gateSource.indexOf("const installSignalTraps", teardownStart);
    const teardownSource =
      teardownStart >= 0 && teardownEnd > teardownStart
        ? gateSource.slice(teardownStart, teardownEnd)
        : "";
    if (
      supervisorFactoryCount !== 1 ||
      productionRunStepCount !== 8 ||
      gateSource.includes("await runContainedStep({") ||
      !gateSource.includes('ctx.supervisor.runStep("surface-1", {') ||
      !gateSource.includes('ctx.supervisor.runStep("shipped-cfg", {') ||
      !teardownSource.includes('await supervisor.closeAndReapAll("GATE_TEARDOWN")') ||
      teardownSource.indexOf("await supervisor.closeAndReapAll") >
        teardownSource.indexOf("mutex.release()")
    ) {
      fail(
        `(GB18.10) production must construct exactly one supervisor, route all eight currently ` +
          `sequential contained commands through it (including Surface 1/shipped lanes), and await its ` +
          `close before mutex release; factory=${supervisorFactoryCount} runStep=${productionRunStepCount}`,
      );
      ok = false;
    }

    if (ok) {
      pass(
        "(GB18) measured build resources are CPU/memory-tiered while the independent 12-thread cap " +
          "remains CPU-clamped and both stay explicitly overrideable; both " +
          "Windows-only serialized nextest groups/selectors are forbidden while safety timeouts remain pinned; every Windows proc-macro suite (including a " +
          "novel future id) remains warmable with its listed host libdir prepended to one canonical PATH; " +
          "malformed metadata and every non-zero/no-status/signal outcome fail closed; a real cargo-free " +
          "child receives the environment; production wires it into the unfiltered suite loop; the `gate-lane` " +
          "authority proves aggregate containment and exact multi-forest cleanup.",
      );
    }
  }

  // --------------------------------------------------------------------------------------------------
  // (GB11) ORACLE-CACHE PREREQUISITE PREFLIGHT — the dedicated BF2 lane must tell "the offline oracle
  // npm cache is absent or unusable" apart from "the compiled output genuinely diverges from the pin".
  //
  // THE BUG IT GUARDS. `verter_session/bf2-authoritative` gates authoritative tests — including the ENTIRE
  // `svelte_official_conformance_gate` suite — that realize their Vue/Svelte oracles OFFLINE from a
  // gitignored `.oracle-npm-cache`. In a fresh checkout that cache does not exist, and the harness does
  // NOT fail loudly on a missing/unusable cache: it records the affected axis as
  // `"authoritative mode: link axis skipped (oracle install unavailable: …)"` and keeps comparing every
  // other axis — an environment absence that reads as a compiled-output DIVERGENCE. Measured on a fresh
  // worktree: 5 failures that read exactly like conformance regressions with the cache missing, versus 2
  // real ones with it present.
  //
  // HOW IT IS DRIVEN. Leg 1 calls the real `checkOracleCachePrerequisite` in-process against injected
  // probe outcomes covering all four states the real probe can report (proven for real against this very
  // repo below, and separately in the commit's own report — missing cache, corrupt/present-but-unusable
  // cache, valid cache, and an import-time infra failure). Leg 2 drives the REAL
  // `runOracleCacheLoadProbe` with an injected `spawnFn`, mirroring GB9.1's fail-closed probe-shape matrix
  // (spawn error, null result, signal kill, unparseable structured failure, unexpected exit, the dual
  // error+signal ETIMEDOUT timeout shape) plus the two structured-success/structured-failure exit shapes.
  // Leg 3 pins `oracleCacheProbeBudgetMs`'s cap-and-no-floor contract, the same shape as `probeBudgetMs`.
  // --------------------------------------------------------------------------------------------------
  process.stderr.write("\n(GB11) ORACLE-CACHE PREREQUISITE PREFLIGHT\n");
  {
    let ok = true;

    // ---- Leg 1: the real checker, in-process, over injected probe outcomes ----
    const realized = checkOracleCachePrerequisite({
      repoRoot: "/synthetic",
      loadProbe: () => ({
        ok: true,
        target: "/synthetic/oracle-install.mjs",
        realized: {
          vue: "/synthetic/.oracle-installs/vue",
          svelte: "/synthetic/.oracle-installs/svelte",
        },
      }),
    });
    if (!realized.ok || realized.lines.length !== 0 || realized.realized.vue === undefined) {
      fail(
        `(GB11.1) a successful realize must report ok with NO report lines and the realized install dirs; ` +
          `got ok=${realized.ok} lines=${realized.lines.length} realized=${JSON.stringify(realized.realized)}`,
      );
      ok = false;
    }

    // MISSING cache: `OracleCacheUnprovisionedError` — must name the marker, the provisioning command, and
    // say "not provisioned" (never "invalid"/"unusable" — a missing cache and a corrupt one are DIFFERENT
    // operator stories, both loud, but distinguishable in the message).
    const missingReport = checkOracleCachePrerequisite({
      repoRoot: "/synthetic",
      loadProbe: () => ({
        ok: false,
        target: "/synthetic/oracle-install.mjs",
        reason: "realize-error",
        errorName: "OracleCacheUnprovisionedError",
        framework: "vue",
        detail:
          "OracleCacheUnprovisionedError: oracle npm cache not provisioned at /synthetic/.oracle-npm-cache " +
          "— run `node packages/framework-conformance-harness/scripts/provision-oracle-npm-cache.mjs` first",
      }),
    }).lines.join("\n");
    if (
      !missingReport.includes(ORACLE_CACHE_PREREQUISITE_MARKER) ||
      !missingReport.includes(ORACLE_CACHE_PROVISION_COMMAND) ||
      !missingReport.includes("not provisioned") ||
      missingReport.includes("present but unusable")
    ) {
      fail(
        `(GB11.1) a MISSING cache must report the marker, the provisioning command, and "not provisioned" ` +
          `(and NOT the "present but unusable" wording) — got:\n${missingReport}`,
      );
      ok = false;
    }

    // CORRUPT/INVALID cache: present but `npm ci --offline` itself fails (e.g. ENOTCACHED), or the
    // realized tree fails `ensureOracleDomain`'s closure/drift validation (`PackageDriftError`). Must
    // report the marker, the SAME provisioning command (re-provisioning is the operator remedy either
    // way), and "present but unusable" — never claim the cache is simply absent.
    for (const errorName of ["Error", "PackageDriftError"]) {
      const invalidReport = checkOracleCachePrerequisite({
        repoRoot: "/synthetic",
        loadProbe: () => ({
          ok: false,
          target: "/synthetic/oracle-install.mjs",
          reason: "realize-error",
          errorName,
          framework: "svelte",
          detail: `${errorName}: the realized tree does not match the committed lockfile closure`,
        }),
      }).lines.join("\n");
      if (
        !invalidReport.includes(ORACLE_CACHE_PREREQUISITE_MARKER) ||
        !invalidReport.includes(ORACLE_CACHE_PROVISION_COMMAND) ||
        !invalidReport.includes("present but unusable") ||
        invalidReport.includes("is not provisioned")
      ) {
        fail(
          `(GB11.1) an INVALID cache (errorName=${errorName}) must report the marker, the provisioning ` +
            `command, and "present but unusable" (and NOT "is not provisioned") — got:\n${invalidReport}`,
        );
        ok = false;
      }
    }

    // IMPORT-time infra failure (the oracle-install.mjs module itself could not be loaded) — still a loud
    // refusal naming the marker and the provisioning command, never a silent pass-through.
    const importReport = checkOracleCachePrerequisite({
      repoRoot: "/synthetic",
      loadProbe: () => ({
        ok: false,
        target: "/synthetic/oracle-install.mjs",
        reason: "import-error",
        errorName: "Error",
        detail: "Cannot find module '/synthetic/oracle-install.mjs'",
      }),
    }).lines.join("\n");
    if (
      !importReport.includes(ORACLE_CACHE_PREREQUISITE_MARKER) ||
      !importReport.includes(ORACLE_CACHE_PROVISION_COMMAND)
    ) {
      fail(
        `(GB11.1) an import-time infra failure must still report the marker and provisioning command — got:\n${importReport}`,
      );
      ok = false;
    }

    // ---- Leg 2: runOracleCacheLoadProbe fail-closed over injected spawn shapes (mirrors GB9.1) ----
    const probeShapes = [
      [
        "a throwing spawn",
        "spawn-error",
        () => {
          throw new Error("EACCES");
        },
      ],
      ["a null result", "spawn-error", () => null],
      ["a spawn error", "spawn-error", () => ({ error: new Error("ENOENT") })],
      ["a killed probe", "signalled", () => ({ signal: "SIGKILL", status: null })],
      [
        "an unparseable structured failure",
        "unknown-exit",
        () => ({ status: 3, stdout: "not json", stderr: "" }),
      ],
      [
        "an unexpected non-zero exit",
        "unknown-exit",
        () => ({ status: 9, stdout: "", stderr: "boom" }),
      ],
      [
        "a timeout (dual error+signal ETIMEDOUT)",
        "timeout",
        () => ({
          error: Object.assign(new Error("spawnSync ETIMEDOUT"), { code: "ETIMEDOUT" }),
          signal: "SIGKILL",
          status: null,
        }),
      ],
    ];
    for (const [label, wantReason, spawnFn] of probeShapes) {
      const probe = runOracleCacheLoadProbe({ repoRoot: "/synthetic", spawnFn });
      if (probe.ok) {
        fail(`(GB11.2) ${label} must NOT report the cache as realized`);
        ok = false;
      }
      if (probe.reason !== wantReason) {
        fail(
          `(GB11.2) ${label} must classify as ${wantReason}; got ${probe.reason} — got: ${probe.detail}`,
        );
        ok = false;
      }
    }
    // A budget of 0 must refuse WITHOUT spawning (spawnFn is dead code if invoked).
    const noBudget = runOracleCacheLoadProbe({
      repoRoot: "/synthetic",
      timeoutMs: 0,
      spawnFn: () => {
        fail("(GB11.2) a non-positive budget must not spawn the probe at all");
        ok = false;
        return { status: 0, stdout: "{}" };
      },
    });
    if (noBudget.ok || noBudget.reason !== "timeout") {
      fail(
        `(GB11.2) a 0ms budget must refuse as a timeout WITHOUT spawning; got ok=${noBudget.ok} reason=${noBudget.reason}`,
      );
      ok = false;
    }
    // Structured success (exit 0, realized JSON on stdout).
    const okProbe = runOracleCacheLoadProbe({
      repoRoot: "/synthetic",
      spawnFn: () => ({
        status: 0,
        stdout: JSON.stringify({ ok: true, realized: { vue: "/x/vue", svelte: "/x/svelte" } }),
        stderr: "",
      }),
    });
    if (
      !okProbe.ok ||
      okProbe.realized.vue !== "/x/vue" ||
      okProbe.realized.svelte !== "/x/svelte"
    ) {
      fail(
        `(GB11.2) a clean exit-0 probe must report ok with the realized install dirs; got ${JSON.stringify(okProbe)}`,
      );
      ok = false;
    }
    // Structured failure at each `stage` — proves errorName/framework/reason are threaded through, not
    // just the raw message.
    const importFail = runOracleCacheLoadProbe({
      repoRoot: "/synthetic",
      spawnFn: () => ({
        status: 3,
        stdout: JSON.stringify({ stage: "import", name: "Error", message: "Cannot find module" }),
        stderr: "",
      }),
    });
    if (importFail.ok || importFail.reason !== "import-error" || importFail.errorName !== "Error") {
      fail(
        `(GB11.2) a stage:"import" exit-3 must classify as import-error with errorName threaded through; got ${JSON.stringify(importFail)}`,
      );
      ok = false;
    }
    const realizeFail = runOracleCacheLoadProbe({
      repoRoot: "/synthetic",
      spawnFn: () => ({
        status: 3,
        stdout: JSON.stringify({
          stage: "realize",
          framework: "svelte",
          name: "OracleCacheUnprovisionedError",
          message: "oracle npm cache not provisioned",
        }),
        stderr: "",
      }),
    });
    if (
      realizeFail.ok ||
      realizeFail.reason !== "realize-error" ||
      realizeFail.errorName !== "OracleCacheUnprovisionedError" ||
      realizeFail.framework !== "svelte"
    ) {
      fail(
        `(GB11.2) a stage:"realize" exit-3 must classify as realize-error with errorName+framework threaded through; got ${JSON.stringify(realizeFail)}`,
      );
      ok = false;
    }

    // ---- Leg 3: the probe budget contract (mirrors probeBudgetMs's cap-and-no-floor) ----
    if (oracleCacheProbeBudgetMs(1_000_000, 0) !== ORACLE_CACHE_PROBE_MAX_MS) {
      fail(
        `(GB11.3) a huge remaining window must cap at ORACLE_CACHE_PROBE_MAX_MS (${ORACLE_CACHE_PROBE_MAX_MS})`,
      );
      ok = false;
    }
    if (oracleCacheProbeBudgetMs(1000, 900) !== 100) {
      fail(
        `(GB11.3) a small remaining window must pass through uncapped; got ${oracleCacheProbeBudgetMs(1000, 900)}`,
      );
      ok = false;
    }
    if (oracleCacheProbeBudgetMs(1000, 5000) >= 0) {
      fail(
        `(GB11.3) an EXPIRED deadline must yield a NEGATIVE budget (no floor), not a floored positive one`,
      );
      ok = false;
    }

    if (ok) {
      pass(
        "(GB11) ORACLE-CACHE PREREQUISITE PREFLIGHT: checkOracleCachePrerequisite distinguishes a MISSING " +
          "cache from an INVALID (present-but-unusable) one from a successful realization, always naming " +
          "the marker and the exact (never auto-run) provisioning command; runOracleCacheLoadProbe fails " +
          "closed on every non-success spawn shape (spawn error, signal, timeout, unparseable output) and " +
          "correctly threads errorName/framework/stage through both structured exit shapes; the probe " +
          "budget caps-without-a-floor exactly like the build-prerequisite preflight's. All four REAL cache " +
          "states (missing / corrupt / valid, plus restore) were additionally proven against this actual " +
          "repository outside this in-process harness — see the change's own report.",
      );
    }
  }

  // --------------------------------------------------------------------------------------------------
  // (GB12) CORE FEATURE ISOLATION + DEDICATED BF2 EXACT INVENTORY + SHIPPED-CFG SOURCE SCAN.
  //
  // GB12.1 — the canonical archive must stay feature-default, while the separate BF2 command pins the
  // feature, exact module selector, source-derived `#[test]` inventory, and per-module nextest-list parity.
  // THE REGRESSION THIS CATCHES: BF2 drifts back onto the core critical path, or moving it out silently
  // leaves CI with a stale/partial/zero authoritative selection.
  //
  // GB12.2 — countTestAttributesInDir: the shipped-cfg guard's independent expected-test-inventory scan
  // (deletion-bar row "shipped configuration silently selects zero tests" -> required detector
  // "independent expected inventory", per the maintainer directive). THE REGRESSION THIS CATCHES: a
  // guard that only checks `runCount !== 0` cannot tell "ran every declared test" from "ran only the two
  // profile-sanity canaries because every behavioral test got compiled out" — both report a non-zero
  // count. A genuine before/after mutation on a synthetic fixture directory proves the scanner tracks the
  // actual `#[test]` count, not a value baked in at some earlier read.
  //
  // GB12.3 — decideShippedCfgGuardExpectedCountMatch: the actual comparison `runShippedCfgLane` (gate.mjs)
  // branches on, exercised directly (not reimplemented) against GB12.2's own fixture-derived count. Round-2
  // review finding: GB12.2 alone tested only the independent SCANNER, never the VERDICT gate.mjs derives
  // from it — reverting the live guard's comparison back to `runCount === 0` while leaving the scanner
  // untouched would have left the self-test green. `decideShippedCfgGuardExpectedCountMatch` is now the
  // SOLE place that comparison is made (gate.mjs contains no inline copy), so GB12.3 calling it directly IS
  // calling the production decision path.
  // --------------------------------------------------------------------------------------------------
  process.stderr.write("\n(GB12) CORE FEATURE ISOLATION + BF2/SHIPPED EXACT INVENTORY\n");
  {
    let ok = true;

    if (ARCHIVE_FEATURES.length !== 0) {
      fail(
        `(GB12.1) core ARCHIVE_FEATURES must be empty so BF2 remains off the core critical path; got ` +
          JSON.stringify(ARCHIVE_FEATURES),
      );
      ok = false;
    }
    for (const cargoProfile of [null, "no-debug-assertions"]) {
      const args = buildNextestArchiveArgs({
        buildJobs: 4,
        cargoProfile,
        archiveFile: "/synthetic/a.tar.zst",
        runnerTarget: "/synthetic/target",
      });
      if (args.includes("--features")) {
        fail(
          `(GB12.1) core buildNextestArchiveArgs({cargoProfile:${JSON.stringify(cargoProfile)}}) must ` +
            `not emit --features; got ${JSON.stringify(args)}`,
        );
        ok = false;
      }
    }

    const bf2Source = scanBf2AuthoritativeSourceInventory(REPO_REALPATH);
    if (
      JSON.stringify(bf2Source.modules) !== JSON.stringify(BF2_AUTHORITATIVE_MODULES) ||
      !(bf2Source.total > 0)
    ) {
      fail(
        `(GB12.1) BF2 source discovery must exactly match the pinned non-empty module inventory; got ` +
          JSON.stringify(bf2Source),
      );
      ok = false;
    }
    for (const mode of ["list", "run"]) {
      const args = buildBf2NextestArgs(mode);
      const featureAt = args.indexOf("--features");
      if (
        featureAt < 0 ||
        args[featureAt + 1] !== BF2_AUTHORITATIVE_FEATURE ||
        !args.includes("-E") ||
        args.some((arg) => /threads|jobs/.test(arg))
      ) {
        fail(
          `(GB12.1) dedicated BF2 ${mode} argv must pin the feature/filter with no capacity cap; got ` +
            JSON.stringify(args),
        );
        ok = false;
      }
    }
    const syntheticListed = countBf2AuthoritativeListTests({
      "rust-suites": Object.fromEntries(
        BF2_AUTHORITATIVE_MODULES.map((module) => [
          module,
          {
            testcases: Object.fromEntries(
              Array.from({ length: bf2Source.countByModule[module] || 0 }, (_, index) => [
                `compile::map_equality_tests::${module}::synthetic_${index}`,
                { "filter-match": { status: "matches" } },
              ]),
            ),
          },
        ]),
      ),
    });
    if (decideBf2AuthoritativeInventoryMatch(syntheticListed, bf2Source) !== null) {
      fail("(GB12.1) an exact per-module BF2 source/list inventory must be admitted");
      ok = false;
    }
    const fewerListed = { ...syntheticListed, total: syntheticListed.total - 1 };
    if (
      !/selected .* declares/.test(
        decideBf2AuthoritativeInventoryMatch(fewerListed, bf2Source) || "",
      )
    ) {
      fail("(GB12.1) a partial BF2 nextest selection must fail closed naming both totals");
      ok = false;
    }

    // GB12.2: countTestAttributesInDir against a synthetic fixture tree, mutated in place.
    const fxRoot = mkdtempSync(join(tmpdir(), "gate-shipped-cfg-inventory-"));
    try {
      mkdirSync(join(fxRoot, "nested"), { recursive: true });
      writeFileSync(
        join(fxRoot, "lib.rs"),
        "#[cfg(test)]\nmod tests {\n    #[test]\n    fn a() {}\n\n    #[test]\n    fn b() {}\n}\n",
      );
      writeFileSync(join(fxRoot, "nested", "more.rs"), "#[test]\nfn c() {}\n");
      // A non-`.rs` file containing the literal text must NOT be counted — the scanner walks source
      // files, not arbitrary text.
      writeFileSync(join(fxRoot, "notes.md"), "#[test]\nfn decoy() {}\n");

      const before = countTestAttributesInDir(fxRoot);
      if (before !== 3) {
        fail(
          `(GB12.2) countTestAttributesInDir must count exactly the 3 #[test] attributes across ` +
            `lib.rs + nested/more.rs (and ignore notes.md); got ${before}`,
        );
        ok = false;
      }

      // THE MUTATION: add a fourth #[test] fn to the nested file. A scanner that cached its first read,
      // globbed only the top-level directory, or hardcoded a count would NOT observe this.
      writeFileSync(
        join(fxRoot, "nested", "more.rs"),
        "#[test]\nfn c() {}\n\n#[test]\nfn d() {}\n",
      );
      const after = countTestAttributesInDir(fxRoot);
      if (after !== 4) {
        fail(
          `(GB12.2) DISCRIMINATION: adding a #[test] fn to nested/more.rs must raise the count from 3 to ` +
            `4; got ${after} (before-mutation count was ${before}) — the scanner is not actually reading ` +
            "the current source tree.",
        );
        ok = false;
      }

      // GB12.3: decideShippedCfgGuardExpectedCountMatch — the actual VERDICT `runShippedCfgLane` (gate.mjs)
      // branches on, extracted into gate-internals.mjs as the SOLE place the comparison is made (no inline
      // copy left in gate.mjs). Exercised directly against `after` (4), the SAME count GB12.2 just proved
      // `countTestAttributesInDir` tracks live — not a hand-picked number disconnected from the scanner.
      //
      // THE REGRESSION THIS CATCHES (round-2 review counterexample): reverting `runShippedCfgLane`'s check
      // back to a bare `runCount === 0` while leaving `countTestAttributesInDir` untouched. Because
      // `runShippedCfgLane` now has no inline comparison of its own — it only calls this function — that
      // revert IS a revert of this function, and GB12.3 calls it directly.
      if (decideShippedCfgGuardExpectedCountMatch(after, after) !== null) {
        fail(
          `(GB12.3) decideShippedCfgGuardExpectedCountMatch(${after}, ${after}) (exact match) must return ` +
            `null (proceed); got ${JSON.stringify(decideShippedCfgGuardExpectedCountMatch(after, after))}`,
        );
        ok = false;
      }
      // THE NAMED REGRESSION CLASS: nextest selected FEWER tests than the independent scanner counted
      // (e.g. an accidental cfg(debug_assertions) compiled out a behavioral #[test] while the two
      // profile-sanity canaries stayed intact — a bare `runCount !== 0` check would miss this entirely).
      const fewer = decideShippedCfgGuardExpectedCountMatch(after - 1, after);
      if (
        !fewer ||
        fewer.exit !== EXIT_USAGE ||
        !/selected 3/.test(fewer.message) ||
        !/found 4/.test(fewer.message)
      ) {
        fail(
          `(GB12.3) DISCRIMINATION: decideShippedCfgGuardExpectedCountMatch(${after - 1}, ${after}) ` +
            `(nextest selected fewer than the scanner counted) must fail closed with exit ${EXIT_USAGE} ` +
            `and name both counts in the message; got ${JSON.stringify(fewer)}`,
        );
        ok = false;
      }
      // A superset is equally untrusted (means the scan missed a source file nextest did compile) — must
      // also fail, not be waved through because it is "more, not fewer".
      const more = decideShippedCfgGuardExpectedCountMatch(after + 1, after);
      if (!more || more.exit !== EXIT_USAGE) {
        fail(
          `(GB12.3) decideShippedCfgGuardExpectedCountMatch(${after + 1}, ${after}) (a superset) must also ` +
            `fail closed with exit ${EXIT_USAGE}; got ${JSON.stringify(more)}`,
        );
        ok = false;
      }
      // A zero expected-inventory (the scanner itself broken, or the crate's tests deleted/moved) must
      // fail closed regardless of what nextest reported running.
      const zeroExpected = decideShippedCfgGuardExpectedCountMatch(2, 0);
      if (
        !zeroExpected ||
        zeroExpected.exit !== EXIT_USAGE ||
        !/ZERO #\[test\]/.test(zeroExpected.message)
      ) {
        fail(
          `(GB12.3) decideShippedCfgGuardExpectedCountMatch(2, 0) (broken/empty expected-inventory scan) ` +
            `must fail closed naming the zero-inventory setup failure; got ${JSON.stringify(zeroExpected)}`,
        );
        ok = false;
      }

      // An empty directory (the shape a regression that deletes every source file produces) must report
      // zero, not throw and not silently report a stale non-zero value.
      const emptyRoot = mkdtempSync(join(tmpdir(), "gate-shipped-cfg-inventory-empty-"));
      try {
        const emptyCount = countTestAttributesInDir(emptyRoot);
        if (emptyCount !== 0) {
          fail(`(GB12.2) an empty directory must count 0; got ${emptyCount}`);
          ok = false;
        }
      } finally {
        rmSync(emptyRoot, { recursive: true, force: true });
      }

      // A missing directory (the shape a renamed crate produces) must report zero rather than throw —
      // `decideShippedCfgGuardExpectedCountMatch`'s zero-expected-inventory check (GB12.3 above) is what
      // turns that into a loud failure, not an uncaught exception here.
      const missingCount = countTestAttributesInDir(join(fxRoot, "does-not-exist"));
      if (missingCount !== 0) {
        fail(`(GB12.2) a missing directory must count 0 (not throw); got ${missingCount}`);
        ok = false;
      }
    } finally {
      rmSync(fxRoot, { recursive: true, force: true });
    }

    if (ok) {
      pass(
        "(GB12) the core archive emits no BF2 feature; the dedicated BF2 command pins the feature and " +
          "selector without a worker cap, discovers the exact gated module/source inventory, admits an " +
          "exact per-module nextest listing, and rejects a partial listing; " +
          "countTestAttributesInDir counts #[test] attributes across a real multi-file source tree, " +
          "ignores non-.rs files, and is proven to track a live mutation (3 -> 4) rather than a cached or " +
          "hardcoded value, plus the empty/missing-directory edge cases report 0 without throwing; " +
          "decideShippedCfgGuardExpectedCountMatch (the SOLE production comparison, called directly, not " +
          "reimplemented) proceeds on an exact match, fails closed on a fewer-selected-than-expected " +
          "regression naming both counts, fails closed on a superset, and fails closed on a zero " +
          "expected-inventory scan.",
      );
    }
  }

  // --------------------------------------------------------------------------------------------------
  // (GB12.4) WIRING PROOF — GB12.3 above proves `decideShippedCfgGuardExpectedCountMatch` behaves
  // correctly, but calling it directly never touches gate.mjs's real call site (`runShippedCfgLane`,
  // around the `const parityFailure = decideShippedCfgGuardExpectedCountMatch(...)` line). THE REGRESSION
  // THIS CATCHES: someone reverts that ONE call site back to an inline `runCount === 0` (or any other
  // inline) comparison while leaving `decideShippedCfgGuardExpectedCountMatch` itself untouched — GB12.3
  // would stay green because it never observes the call site, only the function. A full CLI drive here
  // would mean faking BOTH `cargo check --profile no-debug-assertions` and
  // `cargo nextest run -p verter_shipped_cfg_contract` behind a synthetic `cargo` on PATH — disproportionate
  // for proving one call site invokes one named function. Instead this statically scans the PRODUCTION
  // gate.mjs source for `runShippedCfgLane`'s function body and asserts (a) it calls
  // `decideShippedCfgGuardExpectedCountMatch(` and (b) it contains no inline `runCount` comparison
  // (`===`/`!==`/`==`/`!=` against `0`) that could stand in for that call — the two-sided check a
  // one-sided "does it call the function" assertion would miss (the revert keeps the call in scope
  // elsewhere, or the reverted code compares a differently-named local, without actually restoring the old
  // behavior at this call site).
  // --------------------------------------------------------------------------------------------------
  process.stderr.write("\n(GB12.4) SHIPPED-CFG GUARD CALL-SITE WIRING (static source scan)\n");
  {
    const gateSource = readFileSync(GATE, "utf8");
    const fnStart = gateSource.indexOf("async function runShippedCfgLane(");
    if (fnStart === -1) {
      fail("(GB12.4) could not find `async function runShippedCfgLane(` in gate.mjs");
    } else {
      // The function ends at the next top-level `\n}\n` followed by a blank line — bounded by scanning for
      // the next line that is exactly `}` at column 0 after fnStart (functions in this file are not
      // nested at top level, so the first column-0 `}` after the opening brace closes this function).
      const afterStart = gateSource.slice(fnStart);
      const closeMatch = afterStart.match(/\n}\n/);
      if (!closeMatch) {
        fail("(GB12.4) could not find the closing `}` of runShippedCfgLane in gate.mjs");
      } else {
        const fnBody = afterStart.slice(0, closeMatch.index);
        const callsExtracted = fnBody.includes("decideShippedCfgGuardExpectedCountMatch(");
        // Scan CODE lines only — this function's own doc comments discuss the historical
        // `runCount === 0` / `runCount !== 0` shape by name (that is exactly what they were replaced
        // by), so scanning raw source text (comments included) would false-positive on the prose
        // describing the fix rather than the fix itself. Drop every line whose trimmed content starts
        // with `//` before matching.
        const fnBodyCodeOnly = fnBody
          .split("\n")
          .filter((line) => !line.trim().startsWith("//"))
          .join("\n");
        // Matches an inline runCount comparison against 0, e.g. `runCount === 0`, `runCount !== 0`,
        // `guard.summary.runCount == 0` — the shape the extraction was supposed to remove. Deliberately
        // does NOT flag `guard.summary.runCount` used merely as an ARGUMENT to
        // `decideShippedCfgGuardExpectedCountMatch(...)` (no comparison operator there).
        const hasInlineComparison = /runCount\s*[=!]==?\s*0\b/.test(fnBodyCodeOnly);
        if (!callsExtracted) {
          fail(
            "(GB12.4) runShippedCfgLane's body in gate.mjs no longer calls " +
              "decideShippedCfgGuardExpectedCountMatch(...) — the extracted function GB12.3 verifies is no " +
              "longer wired to the real call site.",
          );
        } else if (hasInlineComparison) {
          fail(
            "(GB12.4) runShippedCfgLane's body in gate.mjs contains an inline `runCount ... 0` comparison " +
              "in addition to (or instead of) calling decideShippedCfgGuardExpectedCountMatch(...) — this is " +
              "exactly the round-2 regression: an inline check that bypasses the verified extracted function.",
          );
        } else {
          pass(
            "(GB12.4) runShippedCfgLane's real call site in gate.mjs calls " +
              "decideShippedCfgGuardExpectedCountMatch(...) directly with no inline runCount comparison " +
              "standing in for it.",
          );
        }
      }
    }
  }

  // --------------------------------------------------------------------------------------------------
  // (GB13) TRYBUILD EXCLUSION (interim, pending maintainer disposition) — the registry, the filter/skip-arg
  // builders, and the per-row zero-match LOUD-failure discriminator that `verifyTrybuildExclusionCoverage`
  // in gate.mjs gates every surface on.
  //
  // THE REGRESSION THIS CATCHES: a trybuild file is renamed/moved/deleted and TRYBUILD_EXCLUDED_SUITES is
  // not updated — the exclusion silently stops covering it (SILENT SKIP of the exclusion itself, not of a
  // test) while every OTHER row still matches, so a naive "total > 0" check would stay green. The per-row
  // `missing` discriminator is what makes that a hard, named setup failure instead.
  // --------------------------------------------------------------------------------------------------
  process.stderr.write("\n(GB13) TRYBUILD EXCLUSION (interim)\n");
  {
    let ok = true;

    // The registry has one row per excluded driver across its six packages. Compiler coverage is checked
    // independently against every source file that actually calls trybuild::TestCases::new().
    const wantPackages = [
      "verter_session",
      "verter_language",
      "verter_identity",
      "verter_compiler",
      "verter_audit",
      "verter_type_runtime",
    ];
    if (TRYBUILD_EXCLUDED_SUITES.length !== wantPackages.length) {
      fail(
        `(GB13.1) TRYBUILD_EXCLUDED_SUITES must have ${wantPackages.length} rows across its six ` +
          `registered packages; got ${TRYBUILD_EXCLUDED_SUITES.length}: ` +
          JSON.stringify(TRYBUILD_EXCLUDED_SUITES),
      );
      ok = false;
    }
    const gotPackages = TRYBUILD_EXCLUDED_SUITES.map((s) => s.package).sort();
    if (JSON.stringify(gotPackages) !== JSON.stringify([...wantPackages].sort())) {
      fail(
        `(GB13.1) TRYBUILD_EXCLUDED_SUITES packages must be exactly ${JSON.stringify([...wantPackages].sort())} ` +
          `(order-independent); got ${JSON.stringify(gotPackages)}`,
      );
      ok = false;
    }
    for (const row of TRYBUILD_EXCLUDED_SUITES) {
      if (!row.modulePrefix.startsWith("cases::") || !row.modulePrefix.endsWith("::")) {
        fail(
          `(GB13.1) every row's modulePrefix must be a "cases::...::" module path (so it anchors a whole ` +
            `module, not a partial name); got ${JSON.stringify(row)}`,
        );
        ok = false;
      }
    }

    const discoveredCompilerSources = discoverCompilerTrybuildSourceModulePrefixes(REPO_REALPATH);
    const registeredCompilerSources = TRYBUILD_EXCLUDED_SUITES.filter(
      (row) => row.package === "verter_compiler",
    )
      .map((row) => row.modulePrefix)
      .sort();
    if (JSON.stringify(registeredCompilerSources) !== JSON.stringify(discoveredCompilerSources)) {
      fail(
        `(GB13.1) verter_compiler registry rows must exactly cover every source module that calls ` +
          `trybuild::TestCases::new(); registered=${JSON.stringify(registeredCompilerSources)} ` +
          `discovered=${JSON.stringify(discoveredCompilerSources)}`,
      );
      ok = false;
    }
    if (
      !compilerTrybuildDriverUsesCanonicalConstructor(
        "fn guard() { let tests = trybuild::TestCases::new(); }",
        "synthetic-canonical.rs",
      )
    ) {
      fail("(GB13.1) the canonical fully-qualified trybuild constructor must be discovered");
      ok = false;
    }
    for (const [label, source] of [
      ["imported", "use trybuild::TestCases; fn guard() { let tests = TestCases::new(); }"],
      [
        "renamed-import",
        "use trybuild::TestCases as Cases; fn guard() { let tests = Cases::new(); }",
      ],
      ["crate-alias", "use trybuild as tb; fn guard() { let tests = tb::TestCases::new(); }"],
      [
        "constructor-reference",
        "fn guard() { let constructor = trybuild::TestCases::new; let tests = constructor(); }",
      ],
    ]) {
      try {
        compilerTrybuildDriverUsesCanonicalConstructor(source, `synthetic-${label}.rs`);
        fail(`(GB13.1) unsupported ${label} trybuild constructor spelling must fail closed`);
        ok = false;
      } catch (error) {
        if (!/must call `trybuild::TestCases::new/.test(String(error))) {
          fail(`(GB13.1) unsupported ${label} constructor reported the wrong error: ${error}`);
          ok = false;
        }
      }
    }

    // The filterset: `not (...)`, one `(package(pkg) and test(/^prefix/))` arm per row, parenthesized so
    // composing it into the current Surface-1 filter via `and` cannot change precedence.
    const filterExpr = buildTrybuildExclusionFilterExpr();
    if (!filterExpr.startsWith("not (") || !filterExpr.endsWith(")")) {
      fail(`(GB13.2) the filter must be a single negated group "not (...)"; got '${filterExpr}'`);
      ok = false;
    }
    for (const row of TRYBUILD_EXCLUDED_SUITES) {
      const arm = `(package(${row.package}) and test(/^${row.modulePrefix}/))`;
      if (!filterExpr.includes(arm)) {
        fail(`(GB13.2) the filter must contain the arm ${arm}; got '${filterExpr}'`);
        ok = false;
      }
    }

    // Legacy Surface-2 selftest fixture: direct libtest had no `-E`, only `--skip <prefix>`; gate.mjs no
    // longer uses this helper.
    const sessionSkip = trybuildSkipArgsForPackage("verter_session");
    if (
      sessionSkip.length !== 2 ||
      sessionSkip[0] !== "--skip" ||
      sessionSkip[1] !== "cases::g_compile::compile_fail::"
    ) {
      fail(
        `(GB13.3) trybuildSkipArgsForPackage("verter_session") must be exactly ["--skip", ` +
          `"cases::g_compile::compile_fail::"]; got ${JSON.stringify(sessionSkip)}`,
      );
      ok = false;
    }
    const noRowsSkip = trybuildSkipArgsForPackage("verter_workspace");
    if (noRowsSkip.length !== 0) {
      fail(
        `(GB13.3) a package with NO registered rows must get zero --skip args (discriminates a real match ` +
          `from an accidental blanket skip); got ${JSON.stringify(noRowsSkip)}`,
      );
      ok = false;
    }

    // countTrybuildExclusionMatches: build a synthetic archive listing with exactly one real testcase per
    // registered row, PLUS two adversarial false-positive testcases that must NEVER be counted — mirroring
    // the two real same-substring, different-module tests this exclusion must never touch
    // (verter_lsp's external_ts::membership_reconciler::tests::absent_compile_failed_removes and
    // verter_session's types::tests::compile_failure_code_classification).
    const completeSuites = [];
    for (const row of TRYBUILD_EXCLUDED_SUITES) {
      completeSuites.push({
        "package-name": row.package,
        testcases: { [`${row.modulePrefix}some_harness_fn`]: { kind: "test", ignored: false } },
      });
    }
    completeSuites.push({
      "package-name": "verter_lsp",
      testcases: {
        "external_ts::membership_reconciler::tests::absent_compile_failed_removes": {
          kind: "test",
          ignored: false,
        },
      },
    });
    completeSuites.push({
      "package-name": "verter_session",
      testcases: {
        "types::tests::compile_failure_code_classification": { kind: "test", ignored: false },
      },
    });
    const complete = countTrybuildExclusionMatches(completeSuites);
    if (complete.total !== TRYBUILD_EXCLUDED_SUITES.length) {
      fail(
        `(GB13.4) a complete listing must count exactly ${TRYBUILD_EXCLUDED_SUITES.length} trybuild ` +
          `testcase(s) (one per row) and must NOT count the two adversarial same-substring false positives; ` +
          `got total=${complete.total}`,
      );
      ok = false;
    }
    if (complete.missing.length !== 0) {
      fail(
        `(GB13.4) a complete listing must report zero missing rows; got ${JSON.stringify(complete.missing)}`,
      );
      ok = false;
    }

    // THE ZERO-MATCH LOUD-FAILURE DISCRIMINATOR — a renamed/moved/deleted trybuild file (here: dropping the
    // verter_audit testcase, modelling `attribution_compile_fail.rs` renamed without updating the registry)
    // must be reported as exactly that ONE missing row, never silently folded into a still-nonzero total.
    const staleSuites = completeSuites.filter(
      (s) =>
        !(
          s["package-name"] === "verter_audit" &&
          "cases::attribution_compile_fail::some_harness_fn" in (s.testcases || {})
        ),
    );
    const stale = countTrybuildExclusionMatches(staleSuites);
    if (
      stale.missing.length !== 1 ||
      stale.missing[0].package !== "verter_audit" ||
      stale.missing[0].modulePrefix !== "cases::attribution_compile_fail::"
    ) {
      fail(
        "(GB13.5) dropping the verter_audit testcase must report missing=[{package:verter_audit, " +
          `modulePrefix:'cases::attribution_compile_fail::'}]; got ${JSON.stringify(stale.missing)} ` +
          "— this is the exact discriminator gate.mjs's verifyTrybuildExclusionCoverage fails the gate on " +
          "(a stale row is a hard setup failure on every surface, never a silent pass).",
      );
      ok = false;
    }
    if (stale.total !== TRYBUILD_EXCLUDED_SUITES.length - 1) {
      fail(
        `(GB13.5) dropping one row's testcase must reduce total by exactly 1 (the other rows still match); ` +
          `got total=${stale.total}`,
      );
      ok = false;
    }

    if (ok) {
      pass(
        "(GB13) TRYBUILD EXCLUSION: TRYBUILD_EXCLUDED_SUITES names the excluded trybuild drivers across " +
          "six registered packages and exactly covers the compiler's source-derived driver set; " +
          "buildTrybuildExclusionFilterExpr emits one package+test arm per row inside a single " +
          "negated group; trybuildSkipArgsForPackage returns the exact --skip pair for verter_session and " +
          "nothing for an unregistered package; countTrybuildExclusionMatches counts exactly the registered " +
          "rows against a real listing shape while ignoring two adversarial same-substring lookalikes, and " +
          "DISCRIMINATES a single stale/renamed row as a named missing entry rather than folding it into a " +
          "still-nonzero total.",
      );
    }
  }

  // --------------------------------------------------------------------------------------------------
  // (GB14) GATE-FAILURE TRIAGE — pure parsing/classification contract of triage-gate-failure.mjs
  // (triage-gate-internals.mjs). PLATFORM-INDEPENDENT and cargo-free: fixture text in, no spawn, no
  // filesystem beyond what is already imported. The REAL end-to-end proof — planted REAL/FLAKY/INTERACTION
  // tests, genuine `cargo nextest` isolation reruns, true classification — is NOT here (this suite runs no
  // workspace cargo, per its own header); it lives in the dedicated, cargo-using
  // `triage-gate-failure-selftest.mjs`, run separately.
  // --------------------------------------------------------------------------------------------------
  {
    let ok = true;

    // (GB14.1) VERDICT PARSING discriminates FAIL / PASS / PASS-WITH-TOLERATED / no-verdict-at-all, and
    // the FAIL block ends at the first non-matching line (never swallows unrelated trailing log text).
    const failLog =
      "[gate] some narrative line\n" +
      "[gate][error] VERDICT: FAIL — 2 non-tolerated failure(s):\n" +
      "[gate][error]   [nextest] cases::foo::bar\n" +
      "[gate][error]   [libtest:verter_session::main] cases::baz::qux\n" +
      "[gate] this line is NOT part of the block\n";
    const failParsed = parseGateVerdict(failLog);
    if (
      failParsed.kind !== "fail" ||
      failParsed.failures.length !== 2 ||
      failParsed.failures[0].surface !== "nextest" ||
      failParsed.failures[0].name !== "cases::foo::bar" ||
      failParsed.failures[1].surface !== "libtest:verter_session::main" ||
      failParsed.failures[1].name !== "cases::baz::qux"
    ) {
      fail(`(GB14.1) FAIL-verdict parse mismatch: ${JSON.stringify(failParsed)}`);
      ok = false;
    }
    const passParsed = parseGateVerdict(
      `[gate] VERDICT: PASS (surface 1 green; ${SHIPPED_CFG_SKIP_VERDICT_NOTE})\n`,
    );
    if (passParsed.kind !== "pass") {
      fail(`(GB14.1) PASS verdict must parse kind=pass, got ${JSON.stringify(passParsed)}`);
      ok = false;
    }
    const toleratedParsed = parseGateVerdict("[gate] VERDICT: PASS-WITH-TOLERATED (...)\n");
    if (toleratedParsed.kind !== "pass") {
      fail(
        `(GB14.1) PASS-WITH-TOLERATED must parse kind=pass, got ${JSON.stringify(toleratedParsed)}`,
      );
      ok = false;
    }
    const noneParsed = parseGateVerdict("nothing resembling a gate verdict here\n");
    if (noneParsed.kind !== "none") {
      fail(
        `(GB14.1) a log with no VERDICT line must parse kind=none, got ${JSON.stringify(noneParsed)}`,
      );
      ok = false;
    }
    const emptyBlockParsed = parseGateVerdict(
      "[gate][error] VERDICT: FAIL — 0 non-tolerated failure(s):\n",
    );
    if (emptyBlockParsed.kind !== "fail" || emptyBlockParsed.failures.length !== 0) {
      fail(
        `(GB14.1) a FAIL verdict with zero following [surface] lines must parse failures=[] (the CLI, not ` +
          `this parser, treats that as the zero-selection error) — got ${JSON.stringify(emptyBlockParsed)}`,
      );
      ok = false;
    }
    if (ok) {
      pass(
        "(GB14.1) VERDICT PARSING: FAIL/PASS/PASS-WITH-TOLERATED/no-verdict-at-all all discriminate " +
          "correctly, the FAIL failure block stops at the first non-matching line, and a verdict with zero " +
          "named failures parses to an empty (not missing) failures array.",
      );
    }
  }

  {
    let ok = true;
    // (GB14.2) SYNTHETIC-NAME DETECTION — a `<...>` name is a gate.mjs-manufactured diagnostic (crash
    // summary / tolerance-refused / unaccounted), never a real test id.
    if (!isSyntheticFailureName("<run did not complete: 1 of 2 selected test(s) never ran>")) {
      fail("(GB14.2) a <...>-wrapped name must be classified synthetic");
      ok = false;
    }
    if (isSyntheticFailureName("cases::foo::bar")) {
      fail("(GB14.2) an ordinary test name must NOT be classified synthetic");
      ok = false;
    }
    // A name that merely CONTAINS angle brackets mid-string (a legitimate, if unusual, Rust test name)
    // must not be swept up by a substring check — only a full wrap counts.
    if (isSyntheticFailureName("cases::generic::Foo<Bar>::works")) {
      fail(
        "(GB14.2) a name with embedded angle brackets that does not WRAP the whole name is not synthetic",
      );
      ok = false;
    }
    if (ok) {
      pass(
        "(GB14.2) SYNTHETIC-NAME DETECTION: a full `<...>`-wrapped name is synthetic, an ordinary test id " +
          "is not, and a name merely containing angle brackets (not wrapping it) is not swept up.",
      );
    }
  }

  {
    let ok = true;
    // (GB14.3) ISOLATION FILTER/ARGV — binary-id recovery from a real nextest recap segment (reusing the
    // SAME extractNextestTerminalFailures the live gate uses, not a second parser), surface routing to the
    // right cargo profile (dev vs shipped-cfg vs libtest's own dev archive), and the caveat set exactly
    // when binary-id could not be recovered.
    const surface1Text =
      "        FAIL [   0.010s] verter_span uri::tests::a\n" +
      "       PASS [   0.010s] verter_span uri::tests::b\n" +
      "────────────\n" +
      "     Summary [   1.000s] 2 tests run: 1 passed, 1 failed, 0 skipped\n";
    const shippedCfgText =
      "        FAIL [   0.010s] verter_shipped_cfg_contract cases::shared::x\n" +
      "────────────\n" +
      "     Summary [   1.000s] 1 tests run: 0 passed, 1 failed, 0 skipped\n";
    const { targets, unclassifiable } = resolveIsolationTargets({
      failures: [
        { surface: "nextest", name: "uri::tests::a" },
        { surface: "shipped-cfg/nextest", name: "cases::shared::x" },
        { surface: "shipped-cfg/nextest", name: "cases::shared::NOT_IN_RECAP" },
        { surface: "libtest:verter_session::main", name: "cases::other::y" },
        { surface: "nextest", name: "uri::tests::MISSING_FROM_RECAP" },
        { surface: "nextest", name: "<run did not complete: 1 of 2 never ran>" },
        { surface: "some-unrecognized-surface", name: "cases::z" },
      ],
      surfaces: { surface1: surface1Text, shippedCfg: shippedCfgText },
      extractNextestTerminalFailures,
    });
    if (targets.length !== 5 || unclassifiable.length !== 2) {
      fail(
        `(GB14.3) expected 5 isolatable targets + 2 unclassifiable, got ${targets.length}/${unclassifiable.length}`,
      );
      ok = false;
    }
    const byName = Object.fromEntries(targets.map((t) => [t.name, t]));
    if (
      !byName["uri::tests::a"] ||
      byName["uri::tests::a"].binaryId !== "verter_span" ||
      byName["uri::tests::a"].cargoProfile !== null ||
      byName["uri::tests::a"].packageScope !== null ||
      byName["uri::tests::a"].caveat !== ""
    ) {
      fail(
        `(GB14.3) SURFACE 1 target should recover binary-id 'verter_span', dev profile, no package ` +
          `scope, no caveat`,
      );
      ok = false;
    }
    if (
      !byName["cases::shared::x"] ||
      byName["cases::shared::x"].binaryId !== "verter_shipped_cfg_contract" ||
      byName["cases::shared::x"].cargoProfile !== "no-debug-assertions" ||
      byName["cases::shared::x"].packageScope !== "verter_shipped_cfg_contract" ||
      !byName["cases::shared::x"].runArgs.includes("-p")
    ) {
      fail(
        `(GB14.3) shipped-cfg target should recover binary-id + the shipped-cfg profile + package scope ` +
          `verter_shipped_cfg_contract, with -p threaded into runArgs`,
      );
      ok = false;
    }
    // THE REGRESSION THIS DISCRIMINATES: a shipped-cfg failure whose name is NOT in the (correctly
    // segmented) raw recap — binary-id recovery legitimately fails — must NOT degrade to a bare
    // whole-workspace name-only rerun; it must stay package-scoped regardless.
    const notInRecap = byName["cases::shared::NOT_IN_RECAP"];
    if (
      !notInRecap ||
      notInRecap.binaryId !== null ||
      notInRecap.packageScope !== "verter_shipped_cfg_contract" ||
      !notInRecap.runArgs.includes("-p") ||
      notInRecap.runArgs[notInRecap.runArgs.indexOf("-p") + 1] !== "verter_shipped_cfg_contract"
    ) {
      fail(
        "(GB14.3) DISCRIMINATION: a shipped-cfg failure with NO binary-id recovered must still carry " +
          `packageScope=verter_shipped_cfg_contract and '-p verter_shipped_cfg_contract' in runArgs — got ` +
          `${JSON.stringify(notInRecap)}`,
      );
      ok = false;
    }
    if (
      !byName["cases::other::y"] ||
      byName["cases::other::y"].binaryId !== "verter_session::main"
    ) {
      fail(
        `(GB14.3) libtest-surface target's binary-id comes directly from the surface tag, no recap search`,
      );
      ok = false;
    }
    const missing = byName["uri::tests::MISSING_FROM_RECAP"];
    if (!missing || missing.binaryId !== null || missing.packageScope !== null || !missing.caveat) {
      fail(
        `(GB14.3) a SURFACE 1 name absent from its surface's recap must degrade to a name-only filter ` +
          `WITH a caveat and no package scope (surface 1 is not package-scoped)`,
      );
      ok = false;
    }
    if (missing && missing.filter !== "test(=uri::tests::MISSING_FROM_RECAP)") {
      fail(`(GB14.3) the degraded filter must be name-only, got '${missing && missing.filter}'`);
      ok = false;
    }
    if (
      !unclassifiable.some((u) => u.name.startsWith("<")) ||
      !unclassifiable.some((u) => u.surface === "some-unrecognized-surface")
    ) {
      fail(
        "(GB14.3) both the synthetic name and the unrecognized-surface tag must land in unclassifiable",
      );
      ok = false;
    }
    if (
      quoteNextestFilterValue("plain_ident::ok") !== "plain_ident::ok" ||
      quoteNextestFilterValue('has "quote" and \\backslash') !==
        '"has \\"quote\\" and \\\\backslash"'
    ) {
      fail(
        "(GB14.3) quoteNextestFilterValue must pass bare-safe names through and escape unsafe ones",
      );
      ok = false;
    }
    if (buildIsolationFilter(null, "x::y") !== "test(=x::y)") {
      fail("(GB14.3) buildIsolationFilter with no binary-id must be name-only");
      ok = false;
    }
    if (buildIsolationFilter("pkg::bin", "x::y") !== "binary_id(pkg::bin) & test(=x::y)") {
      fail("(GB14.3) buildIsolationFilter with a binary-id must AND it with the name filter");
      ok = false;
    }
    if (ok) {
      pass(
        "(GB14.3) ISOLATION FILTER/ARGV: binary-id recovery is scoped to the failure's OWNING surface " +
          "segment (never a different surface's), routes to the right cargo profile per surface " +
          "(nextest=dev, shipped-cfg/nextest=no-debug-assertions, libtest:<id>=dev via the tag directly), " +
          "degrades to a caveated name-only filter when the recap does not name the test, a shipped-cfg " +
          "target stays package-scoped to verter_shipped_cfg_contract EVEN when binary-id recovery fails " +
          "(never a whole-workspace fallback), and both a synthetic diagnostic name and an unrecognized " +
          "surface tag land in unclassifiable rather than being silently dropped or crashing the resolver.",
      );
    }
  }

  {
    let ok = true;
    // (GB14.4) CLASSIFICATION — the exact REAL/FLAKY/INTERACTION/INCONCLUSIVE contract from N isolated
    // attempt outcomes. Discriminating: each case must reject the OTHER three classifications, not just
    // accept its own.
    const attempt = (outcome) => ({ outcome });
    const cases = [
      { attempts: [attempt("fail"), attempt("fail"), attempt("fail")], want: "REAL" },
      { attempts: [attempt("pass"), attempt("pass"), attempt("pass")], want: "INTERACTION" },
      { attempts: [attempt("pass"), attempt("fail"), attempt("pass")], want: "FLAKY" },
      { attempts: [attempt("abort"), attempt("abort")], want: "INCONCLUSIVE" },
      // a MIX of abort + a clean pass/fail split still classifies from the valid subset only.
      { attempts: [attempt("abort"), attempt("fail"), attempt("fail")], want: "REAL" },
      { attempts: [attempt("abort"), attempt("pass"), attempt("fail")], want: "FLAKY" },
    ];
    for (const c of cases) {
      const got = classifyAttempts(c.attempts).classification;
      if (got !== c.want) {
        fail(
          `(GB14.4) classifyAttempts(${JSON.stringify(c.attempts.map((a) => a.outcome))}) = ${got}, want ${c.want}`,
        );
        ok = false;
      }
    }
    const partial = classifyAttempts([attempt("fail"), attempt("abort"), attempt("fail")]);
    if (partial.classification !== "REAL" || partial.complete !== false || partial.aborted !== 1) {
      fail(
        `(GB14.4) a partially-aborted REAL run must still classify REAL and report complete=false, aborted=1`,
      );
      ok = false;
    }
    if (ok) {
      pass(
        "(GB14.4) CLASSIFICATION: N/N fail => REAL, N/N pass => INTERACTION, a genuine mixed split => " +
          "FLAKY, zero valid attempts => INCONCLUSIVE (never guessed), and aborted attempts are excluded " +
          "from the vote but still recorded (never silently treated as a pass or a fail).",
      );
    }
  }

  {
    let ok = true;
    // (GB14.5) SURFACE SEGMENTATION — a nextest recap for a given test-name is read ONLY from that test's
    // OWNING surface segment, never from a different surface's (which could name the same test string
    // under a DIFFERENT profile/outcome and silently cross-contaminate binary-id recovery).
    // Header/body text matched byte-for-byte against what gate.mjs's log()/`runShippedCfgLane` actually
    // print (log() prefixes every line with "[gate] ") — NOT the deleted SURFACE 2 / old package-filtered
    // SURFACE 3 shape. A stale fixture here would pass against itself while the real regex silently never
    // matches a real gate log — the failure mode this test exists to catch.
    const log =
      "[gate] SURFACE 1: nextest run from the archive (process isolation) …\n" +
      "        FAIL [   0.010s] pkg_a cases::shared::x\n" +
      "────────────\n" +
      "     Summary [   1.000s] 1 tests run: 0 passed, 1 failed, 0 skipped\n" +
      "[gate] SHIPPED-CFG GUARD: cargo check --workspace --all-targets --profile no-debug-assertions …\n" +
      "[gate] SHIPPED-CFG GUARD: compile check clean in 5s\n" +
      "[gate] SHIPPED-CFG GUARD: cargo nextest run -p verter_shipped_cfg_contract --cargo-profile no-debug-assertions …\n" +
      "        FAIL [   0.010s] pkg_b cases::shared::x\n" +
      "────────────\n" +
      "     Summary [   1.000s] 1 tests run: 0 passed, 1 failed, 0 skipped\n" +
      "[gate][error] VERDICT: FAIL — 2 non-tolerated failure(s):\n";
    const segs = splitGateLogSurfaces(log);
    const s1 = extractNextestTerminalFailures(segs.surface1);
    const sc = extractNextestTerminalFailures(segs.shippedCfg);
    if (
      s1.failures.length !== 1 ||
      s1.failures[0].binaryId !== "pkg_a" ||
      sc.failures.length !== 1 ||
      sc.failures[0].binaryId !== "pkg_b"
    ) {
      fail(
        `(GB14.5) surface1/shippedCfg segments must each contain ONLY their own surface's recap ` +
          `(got s1=${JSON.stringify(s1.failures)}, shippedCfg=${JSON.stringify(sc.failures)})`,
      );
      ok = false;
    }
    // DISCRIMINATION: the compile-check line ("SHIPPED-CFG GUARD: cargo check …") precedes the
    // nextest-run header and must NOT itself be mistaken for the segment start — proves the header regex
    // matches the specific "cargo nextest run" line, not any "SHIPPED-CFG GUARD:" line.
    if (!segs.shippedCfg.startsWith("[gate] SHIPPED-CFG GUARD: cargo nextest run")) {
      fail(
        `(GB14.5) DISCRIMINATION: the shippedCfg segment must start at the "cargo nextest run" header, ` +
          `not the earlier "cargo check" line; got segment starting: ${JSON.stringify(segs.shippedCfg.slice(0, 80))}`,
      );
      ok = false;
    }
    if (ok) {
      pass(
        "(GB14.5) SURFACE SEGMENTATION: the SAME test name failing on two different surfaces (dev vs " +
          "shipped-cfg) resolves to two DIFFERENT binary-ids because each surface's raw recap is searched " +
          "in isolation — never a cross-surface false match — and the shipped-cfg segment starts at its " +
          "OWN 'cargo nextest run' header, not the earlier 'cargo check' compile-only line under the same " +
          "'SHIPPED-CFG GUARD:' prefix.",
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
