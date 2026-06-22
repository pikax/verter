#!/usr/bin/env node
// gate.mjs — canonical agent Rust gate runner (production CLI).
//
// PRINCIPLE (gate-correctness/security): the production gate binary runs ONLY the real gate — archive →
// nextest run → direct libtest → verdict (plus the gate-adjacent `--prepare` warm utility, which is a
// PREPARE, never a gate PASS). It exposes NO test-seam, NO classifier hook, NO custom-command mode, and NO
// environment variable that can make `node scripts/gate.mjs <anything>` return the gate success contract
// without actually building and running the test suite. The reusable internals (the classifiers, the
// mutex, the contained-step runner, the multi-step seam) live in `gate-internals.mjs` and are imported
// here AND imported DIRECTLY by `gate-selftest.mjs`; the self-test drives the cargo-free seam/classifier
// scenarios in-process, never via a magic flag on this CLI.
//
// OPERATION-SCOPED EXIT SEMANTICS (read this before trusting an exit 0)
//   `exit 0` means "the requested OPERATION succeeded" — it is scoped to the mode you ran, NOT a blanket
//   gate pass. Concretely:
//     * ONLY `node scripts/gate.mjs` (no mode flag) is THE GATE. Its exit 0 means the FULL test suite built
//       AND passed (except the env-only typeinfo freshness PAIR, by exact name, AND only when the freshness-
//       tooling preflight below proves `pnpm` is not resolvable AND `buf` is not resolvable — the condition
//       under which the Rust byte-pin test skips; see FRESHNESS-TOOLING PREFLIGHT). That, and only that, is
//       the gate-pass contract.
//     * `--prepare` is a WARM-PASS only. Its exit 0 means PREPARED (the archive built + the first-launch
//       assessment was warmed) — tests were NOT run, so it is NEVER a gate pass. Its success output carries
//       the `PREPARED_NOT_GATE` marker and contains no `PASS` token precisely so a CI `grep PASS` cannot
//       mistake it for a verdict.
//     * `--help` exits 0 after printing this usage — also not a gate pass.
//   `--help` and `--prepare` are both MUTUALLY EXCLUSIVE and argv-strict (a stray flag/positional alongside
//   either is a usage error, exit 127), so neither exit-0 mode can be reached with junk arguments.
//
// PURPOSE
//   Builds the whole workspace test universe ONCE (via `cargo nextest archive`) and runs BOTH
//   verification surfaces from the SAME archived artifacts:
//     1. nextest run (per-test PROCESS ISOLATION) — surfaces nothing that survives a fork, catches the
//        ordinary regression set.
//     2. the verter_session libtest binaries executed DIRECTLY (in-process / multi-test-per-process) —
//        surfaces shared-process state bugs the isolated path cannot.
//   Because the only build command issued is the single `--workspace` archive build, the gate NEVER
//   issues the package-scoped `cargo test -p verter_session` resolution and so structurally cannot incur
//   the recompile that resolution caused (see "Canonical feature set" below).
//
// CANONICAL FEATURE SET (why no `-p verter_session`)
//   `cargo nextest run --workspace` and `cargo test --workspace` SHARE Cargo feature unification, which
//   activates `verter_session`'s `session_metrics` feature (a downstream crate — `verter_lsp` — depends on
//   `verter_session` with `features = ["session_metrics"]`, so the real LSP binary forces it ON in the
//   workspace build; `verter_napi` only exposes an opt-in `session_metrics` forwarding feature, default off).
//   The package-scoped `cargo test -p verter_session` resolution builds `verter_session` with
//   `session_metrics` OFF (its default) and a different dev-dep closure ⇒ a different unit hash ⇒ an
//   artifact-reuse miss ⇒ a full recompile of the verter_session reverse-dependency chain on the very next
//   gate command. This gate deliberately tests the workspace-unified (`session_metrics` ON) configuration —
//   the PRODUCTION-REACHABLE one (what the shipped LSP binary uses; also reachable from an opt-in
//   `verter_napi/session_metrics` build) — which is exactly why it
//   never issues the package-scoped resolution. It does NOT use `--all-features` (the repo has slow/external
//   feature gates) and does NOT mutate any Cargo.toml.
//
// EQUIVALENCE TO THE TWO-COMMAND GATE
//   The legacy gate was: `cargo nextest run --workspace` then `cargo test -p verter_session --tests`.
//   Here: the nextest run from the archive == surface 1; the direct execution of every `verter_session`
//   suite whose kind is `lib` or `test` (i.e. the lib unit-test binary + every `tests/*.rs` integration
//   binary — exactly what `cargo test --tests` builds; `bin`/`bench` excluded) == surface 2. Surface 2 runs
//   with cwd = the verter_session package manifest dir (what Cargo sets) and the runtime Cargo env those
//   tests actually read (CARGO_MANIFEST_DIR + CARGO_TARGET_DIR — verified complete for this suite), modulo
//   the `session_metrics` cfg (ON here, the production configuration).
//
// SAFETY MODEL (pure Node + OS-native tools; ZERO new compiled binaries)
//   1. Runner-owned target dir: every cargo step runs with CARGO_TARGET_DIR + --target-dir forced to
//      <repo>/target/gate-runner (override via --target-dir / VERTER_GATE_TARGET_DIR), so the gate's
//      .cargo-lock is fully runner-owned and cleanup can never hit a developer's cargo / rust-analyzer
//      (which write the default target/debug). User target overrides are scrubbed.
//   2. Single-flight mutex: an atomic mkdir lockdir with a gate-owned sentinel (storing the owning repo
//      realpath) + owner.json + start-identity. A LIVE holder => REFUSE (LOCK-REFUSED). A dead/stale
//      holder => reclaim via atomic rename (never bare rm of a live holder's dir), defeating PID reuse via
//      process start identity. EVERY reclaim path (including a crashed mid-init lock with no owner.json)
//      refuses unless the sentinel's stored repo realpath equals ours — a foreign checkout's lock is never
//      deleted.
//   3. Process containment: POSIX => the step is spawned detached (its own process group, PGID==PID) and
//      reaped with a negative-PGID SIGTERM→grace→SIGKILL (the whole cargo→rustc→test-binary tree inherits
//      the PGID), then a VERIFICATION poll confirms the group is actually dead. Windows => `taskkill /PID
//      <pid> /T /F` (tree kill) + a re-query poll. This is NOT a hostile-code sandbox: a build script that
//      deliberately setsid/daemonizes can escape — the provenance sweep is the backstop (the bash runner
//      has the same limitation).
//   4. Provenance sweep: after any abnormal termination, TERM→KILL any cargo/rustc/cargo-nextest/nextest
//      process whose command line references the RUNNER-OWNED target dir (NOT the repo root), so a
//      developer's interactive cargo / rust-analyzer (which carry the repo root but write target/debug) is
//      never touched.
//   5. Whole-gate hard timeout (default 50m, --timeout) — a deadline for the ENTIRE gate, not per-step. On
//      expiry the active step's tree is reaped + a sweep runs; exit 124.
//   6. Stall detector with SEPARATE build vs test phases:
//        BUILD phase (the archive build): progress = stdout/stderr byte growth OR runner-owned target-tree
//          artifact growth (file-count + newest-mtime, bounded scan). A long silent rustc is NOT a stall.
//        TEST phase (the nextest run + the direct libtest execs): progress = stdout/stderr byte growth
//          ONLY. Target-tree growth is NOT a valid test liveness signal; a silent test binary IS a hang.
//      Default stall 12m (--stall). On stall: reap + sweep; exit 125.
//   7. Spotlight marker (macOS): a <runnerTarget>/.metadata_never_index file is written so Spotlight does
//      not index the build tree (a harmless no-op file on Linux/Windows).
//
// FRESHNESS-TOOLING PREFLIGHT + VERDICT-GATED TOLERANCE (gate mode only)
//   The two `typeinfo_proto_ts_freshness` byte-equality tests regenerate the committed TS proto bindings
//   through the workspace `buf` + `oxfmt` binaries (resolved under `node_modules/.bin` first, PATH second).
//   In a fresh `git worktree` nobody runs `pnpm install`, so those binaries can be absent. With `buf` absent the
//   Rust byte-pin pair SKIPS and PASSES (no FAIL line); the real risk is the OPPOSITE — a blanket env-tolerance
//   would swallow (a) a trivially-fixable missing `pnpm install` and (b) a GENUINE stale-binding regression
//   (tools present, bindings drifted, the test RUNS and FAILS), which shares the same two test names. So AFTER the gate mutex is held and BEFORE
//   the archive build, the gate runs a pure-Node preflight (inside the SAME deadline/stall/teardown model):
//   Tolerance is BUF-ABSENCE-ONLY: it is allowed ONLY when `buf` is not resolvable — exactly the condition
//   under which the Rust byte-pin test SKIPS (`locate_buf_binary(root)?` early-returns). With `buf` present
//   the test RUNS; `oxfmt` is the test's CONDITIONAL formatter, so a missing `oxfmt` with `buf` available is a
//   LOUD setup failure (exit 127), NOT tolerated and NOT a degraded un-oxfmt'd run. Whether to ATTEMPT the
//   install is a POSITIVE-RESOLVE-BEFORE-INSTALL fact (`pnpm` resolved via PATH — platform-aware, with the
//   Windows .CMD/.cmd/.exe/.bat suffixes — and the RESOLVED path is the exact binary the launcher runs:
//   directly on POSIX / that resolved `.cmd` path quoted under `cmd.exe /d /s /c` on Windows; a local
//   `node_modules/.bin/pnpm` shim the launcher never invokes does NOT count), NOT inferred from an install
//   spawn failure:
//     * both `node_modules/.bin` shims present up front      → tolerance DISABLED (no install).
//     * a shim missing → resolve `pnpm` via PATH (platform-aware) as a POSITIVE fact:
//         - pnpm NOT resolvable → re-resolve `buf`/`oxfmt` (the install is NOT run):
//             · `buf` NOT resolvable                          → the Rust freshness pair SKIPS gracefully and
//               PASSES (no `FAIL` line), so the gate reports an ORDINARY PASS. The verdict-gated tolerance is
//               flipped ON here as a LATENT safety net — it surfaces PASS-WITH-TOLERATED only in the unusual
//               case the pair produces a tolerated `FAIL` despite `buf` being absent (the skip path does not).
//             · `buf` present + `oxfmt` present               → tolerance DISABLED (path-fallback).
//             · `buf` present + `oxfmt` MISSING               → SETUP FAILURE, exit 127 (LOUD; ensure `oxfmt`)
//               — never tolerated, never a degraded run.
//         - pnpm IS resolvable → `pnpm install --frozen-lockfile` (never mutates the lockfile), then:
//             · watchdog (TIMEOUT/STALL)                      → propagated, never tolerated.
//             · spawnError / launched non-zero                → SETUP FAILURE, exit 127 (LOUD, never
//               PASS-WITH-TOLERATED) — e.g. a frozen-lockfile mismatch.
//             · exit 0 → RE-RESOLVE `buf`/`oxfmt`: both present → tolerance DISABLED; `buf` missing OR `oxfmt`
//               missing → SETUP FAILURE, exit 127.
//   The verdict boundary is GATED on that result: PASS-WITH-TOLERATED can be reached ONLY when the preflight
//   ALLOWED the tolerance (pnpm not resolvable AND `buf` not resolvable) AND the freshness pair actually
//   produced a tolerated `FAIL` line. On a real buf-less runner the Rust pair SKIPS (no `FAIL`), so the gate
//   reports an ordinary PASS and the allowance is never consumed — it is a latent net, not the normal
//   buf-less verdict. Tools present/installed ⇒ a freshness-pair FAIL is a HARD failure; any other test,
//   abnormal exit, missing summary, or count mismatch stays hard regardless.
//   In CI deps are already installed, so the preflight is a cheap no-op.
//
// USAGE
//   node scripts/gate.mjs [--timeout 50m] [--stall 12m] [--target-dir <DIR>] [--no-fail-fast]
//                         [--test-threads N]                # THE GATE — exit 0 = suite built + passed.
//   node scripts/gate.mjs --prepare [--target-dir <DIR>] [--timeout 50m] [--stall 12m]
//                                             # warm-pass: archive + list (+ a one-shot warm of the macOS
//                                             # first-launch assessment). Prints the `PREPARED_NOT_GATE`
//                                             # marker — this is a PRE-WARM (tests were NOT run), NOT a gate
//                                             # pass, and is not counted in a timed gate. --prepare combines
//                                             # ONLY with --target-dir/--timeout/--stall; any other flag or a
//                                             # positional argument is a usage error (exit 127).
//   node scripts/gate.mjs --help              # prints this usage + exits 0. Accepts no other argument
//                                             # (a bare --help only); --help with any other token => 127.
//     durations: s/m/h suffix or bare seconds (e.g. 50m, 12m, 5s, 90).
//
//   This CLI accepts ONLY the flags above. There is intentionally NO test-seam / classifier-hook /
//   custom-command mode — every accepted invocation either runs the real gate, runs the `--prepare`
//   warm-pass, or prints help. An unknown flag is a USAGE error (exit 127), never a silent success.
//
// EXIT CODES (distinct, documented; exit 0 is OPERATION-scoped — see OPERATION-SCOPED EXIT SEMANTICS above)
//   0   PASS / PASS-WITH-TOLERATED  (the GATE: a real `node scripts/gate.mjs` run); OR a successful
//       --prepare warm-pass (PREPARED_NOT_GATE — NOT a gate pass); OR --help after printing usage
//   1   FAIL          (a build/test command failed / a non-tolerated test failed)
//   124 TIMEOUT       (whole-gate wallclock deadline tripped)
//   125 STALL         (no progress within the stall window)
//   126 LOCK-REFUSED  (another gate holds the single-flight mutex and is alive / lock uninspectable)
//   127 USAGE/SETUP   (bad arguments, repo root not found, archive/list setup failure)
//
// ENV VARS HONORED
//   VERTER_GATE_LOCK / MOM_GATE_LOCK   lockdir path (default: OS temp dir keyed by repo realpath)
//   VERTER_GATE_TARGET_DIR             runner-owned target dir (default <repo>/target/gate-runner)
//   CARGO_TARGET_DIR / CARGO_BUILD_TARGET_DIR / CARGO_BUILD_BUILD_DIR are SCRUBBED and forced to the
//     runner-owned dir.
//   (No environment variable can divert this CLI to a non-gate success path.)

import { fileURLToPath } from "node:url";
import {
  // exit-code constants (EXIT_STALL is mapped inside mapStepReason, not referenced directly here)
  EXIT_PASS,
  EXIT_FAIL,
  EXIT_TIMEOUT,
  EXIT_LOCK_REFUSED,
  EXIT_USAGE,
  // logging + time
  log,
  warn,
  err,
  nowMs,
  parseDuration,
  // --prepare success output (warm-pass marker — never a gate PASS token)
  preparedSuccessLines,
  // setup
  resolveRepoRoot,
  defaultLockDir,
  buildCargoEnv,
  // mutex + teardown
  Mutex,
  reapActiveStep,
  provenanceSweep,
  // contained step + analysis
  runContainedStep,
  mapStepReason,
  analyzeNextestSurface,
  analyzeLibtestSurface,
  // freshness-tooling preflight (verdict-gating authority)
  preflightFreshnessTooling,
  pnpmInstallCommand,
  selectSessionSuites,
  deriveSuitePkgInfo,
  buildSuiteEnv,
  resolveSuiteBinary,
  parseNextestListJson,
  // fs + path (re-exported from gate-internals so this CLI imports one module)
  mkdirSync,
  writeFileSync,
  readFileSync,
  existsSync,
  spawnSync,
  join,
  dirname,
  isAbsolute,
} from "./gate-internals.mjs";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));

// ----------------------------------------------------------------------------------------------------
// Argument parsing. The production CLI accepts ONLY the real-gate flags + --prepare + --help. There is NO
// `-- <cmd>` custom-command path, NO `--selftest-*` hook, and NO `--internal-selftest-seam` seam: those
// would be modes that can return success without running the gate, which this binary must never expose.
// An unknown argument is a USAGE error (exit 127), never a silent success.
//
// OPERATION-SCOPED EXIT SEMANTICS — the two NON-gate modes (`--help`, `--prepare`) each legitimately exit 0
// on success, but ONLY `node scripts/gate.mjs` with NO mode flag carries the gate-pass contract. To keep
// those two modes from being confusable with a gate pass, BOTH are MUTUALLY EXCLUSIVE and argv-strict:
//   --help / -h : accepts NO other argv token whatsoever. `gate.mjs --help --anything` (a flag OR a
//     positional) is a USAGE error (exit 127) — only a bare `gate.mjs --help` prints usage and exits 0, so
//     a stray flag can never be silently swallowed under the exit-0 help mode.
//   --prepare   : accepts ONLY the companion flags the prepare warm-pass actually uses (--target-dir,
//     --timeout, --stall, each with its value); ANY other flag (e.g. --no-fail-fast / --test-threads — gate-
//     only) or ANY positional token is a USAGE error (exit 127). `gate.mjs --prepare junk` /
//     `--prepare --selftest-x` exit 127, so prepare's exit-0 cannot be reached with junk argv.
// The gate mode (no mode flag) accepts the full real-gate flag set.
// ----------------------------------------------------------------------------------------------------

// Flags --prepare is allowed to combine with (the warm-pass front half — archiveAndList — reads exactly
// these). Each takes a value argument. Gate-only flags (--no-fail-fast / --test-threads) are NOT here, so
// `--prepare --no-fail-fast` is a usage error rather than a silently-ignored flag.
const PREPARE_ALLOWED_VALUE_FLAGS = new Set(["--target-dir", "--timeout", "--stall"]);

function usageError(msg) {
  return new Error(msg);
}

function parseArgs(argv) {
  // --help / -h is mutually exclusive: a bare `--help` (the SOLE token) prints usage + exits 0; help
  // alongside ANY other token (flag or positional) is a usage error. This is checked FIRST so a stray
  // trailing flag (e.g. `--help --bad-flag`) can never ride the exit-0 help mode.
  if (argv.includes("--help") || argv.includes("-h")) {
    if (argv.length !== 1) {
      throw usageError(
        "--help/-h is mutually exclusive: it accepts no other argument. Run a bare `node scripts/gate.mjs " +
          "--help` for usage; any flag or positional alongside --help is a usage error.",
      );
    }
    return {
      mode: "help",
      timeoutSecs: parseDuration("50m"),
      stallSecs: parseDuration("12m"),
      targetDir: process.env.VERTER_GATE_TARGET_DIR || "",
      noFailFast: true,
      testThreads: null,
    };
  }

  // --prepare is mutually exclusive: it combines ONLY with its warm-pass companion flags
  // (--target-dir/--timeout/--stall) and accepts NO positional argument and NO gate-only flag. This is the
  // non-gate warm-pass; rejecting stray argv keeps its exit-0 unreachable with junk arguments.
  if (argv.includes("--prepare")) {
    const opts = {
      mode: "prepare",
      timeoutSecs: parseDuration("50m"),
      stallSecs: parseDuration("12m"),
      targetDir: process.env.VERTER_GATE_TARGET_DIR || "",
      noFailFast: true,
      testThreads: null,
    };
    let i = 0;
    while (i < argv.length) {
      const a = argv[i];
      if (a === "--prepare") {
        // the mode selector itself; already handled.
      } else if (PREPARE_ALLOWED_VALUE_FLAGS.has(a)) {
        const v = argv[++i];
        if (v === undefined) {
          throw usageError(`--prepare: '${a}' requires a value`);
        }
        if (a === "--target-dir") opts.targetDir = v;
        else if (a === "--timeout") opts.timeoutSecs = parseDuration(v);
        else if (a === "--stall") opts.stallSecs = parseDuration(v);
      } else {
        throw usageError(
          `--prepare accepts only --target-dir/--timeout/--stall (and no positional argument); got ` +
            `'${a}'. --prepare is the warm-pass, NOT the gate — gate-only flags and stray tokens are rejected.`,
        );
      }
      i++;
    }
    return opts;
  }

  // Gate mode — the real-gate flag set. No mode flag, so the gate-pass contract applies.
  const opts = {
    mode: "gate",
    timeoutSecs: parseDuration("50m"),
    stallSecs: parseDuration("12m"),
    targetDir: process.env.VERTER_GATE_TARGET_DIR || "",
    noFailFast: true,
    testThreads: null,
  };
  let i = 0;
  while (i < argv.length) {
    const a = argv[i];
    if (a === "--timeout") {
      opts.timeoutSecs = parseDuration(argv[++i]);
    } else if (a === "--stall") {
      opts.stallSecs = parseDuration(argv[++i]);
    } else if (a === "--target-dir") {
      opts.targetDir = argv[++i];
    } else if (a === "--no-fail-fast") {
      opts.noFailFast = true;
    } else if (a === "--test-threads") {
      opts.testThreads = argv[++i];
    } else {
      throw usageError(
        `unknown argument: '${a}'. This gate accepts only --timeout/--stall/--target-dir/` +
          `--no-fail-fast/--test-threads/--prepare/--help; it has no test-seam or custom-command mode.`,
      );
    }
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

  if (opts.mode === "help") {
    // Print ONLY the leading file-header doc-comment block (the `//` lines before the first import /
    // non-comment line) — not the in-body section-banner comments. The block starts after the shebang.
    const lines = readFileSync(fileURLToPath(import.meta.url), "utf8").split("\n");
    const header = [];
    for (let i = 0; i < lines.length; i++) {
      const l = lines[i];
      if (i === 0 && l.startsWith("#!")) continue; // skip the shebang
      if (l.startsWith("//")) {
        header.push(l.replace(/^\/\/ ?/, ""));
        continue;
      }
      if (l.trim() === "") {
        // A blank line inside the header block is kept; a blank line AFTER the block has already broken it.
        if (header.length > 0 && lines[i + 1] && !lines[i + 1].startsWith("//")) break;
        if (header.length > 0) header.push("");
        continue;
      }
      break; // first non-comment, non-blank line ends the header block.
    }
    process.stderr.write(header.join("\n") + "\n");
    process.exit(EXIT_PASS);
  }

  const repoRealpath = resolveRepoRoot(SCRIPT_DIR);
  if (!repoRealpath) {
    err(`could not determine repo root (git rev-parse failed from ${SCRIPT_DIR})`);
    process.exit(EXIT_USAGE);
  }

  const runnerTarget = opts.targetDir
    ? isAbsolute(opts.targetDir)
      ? opts.targetDir
      : join(repoRealpath, opts.targetDir)
    : join(repoRealpath, "target", "gate-runner");

  // Gate work dir (archive, list JSON, extract) lives under the runner target dir.
  const gateDir = join(runnerTarget, "gate-work");

  const lockdir =
    process.env.VERTER_GATE_LOCK || process.env.MOM_GATE_LOCK || defaultLockDir(repoRealpath);

  const token = `${process.pid}.${nowMs()}.${Math.floor(Math.random() * 1e9)}`;
  const cargoEnv = buildCargoEnv(process.env, runnerTarget);

  // Ensure the runner target dir exists + drop the Spotlight marker (macOS) — harmless no-op file elsewhere.
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

  // Teardown — idempotent. ORDER IS LOAD-BEARING for the signal path:
  //   1. Reap the ACTIVE step's WHOLE tree (negative-PGID TERM→grace→KILL / Windows taskkill /T /F) and
  //      VERIFY it is dead (the reap returns a confirmed-dead outcome). This is the SAME reapTree the
  //      watchdog uses, applied to the live child runContainedStep registered. The provenance sweep alone
  //      is NOT sufficient on the signal path — it skips direct libtest binaries and any non-build-tool
  //      child — so an external SIGTERM to ONLY the gate pid (not the group) would otherwise leave a
  //      running test tree orphaned. The mutex is NOT released until the tree is reaped (and we log if
  //      death could not be confirmed within the bound — we still release then, to avoid a permanent hang,
  //      but record the uncertainty rather than claim a clean teardown).
  //   2. Provenance sweep (the backstop for any detached build-tool descendant).
  //   3. Release the mutex (token-checked) — only AFTER the tree is reaped, so a second gate can never
  //      start while the old test process still runs.
  // Did THIS gate acquire the lock? Declared before teardown so the closure reads the live value. The
  // sweep + reap below are gated on it: a gate that REFUSED the lock (another gate holds it) shares the
  // SAME default runner target dir as the holder, so an UNCONDITIONAL provenanceSweep(runnerTarget) would
  // TERM/KILL the HOLDER's cargo/nextest/rustc tree — a non-acquiring gate killing the very build it
  // refused to contend with. ONLY a gate that acquired the lock (and thus ran cargo in its own runner
  // target) may reap/sweep that target; a LOCK-REFUSED / errored-before-acquire gate must touch NO other
  // process. (release() below is always safe: the mutex is token-checked and releases nothing it does not
  // own, so a non-acquiring gate's release is a no-op on the holder's lock.)
  let acquired = false;

  // Memoized so EVERY caller awaits the SAME completion. The signal handlers AND the main-flow `finally`
  // both invoke teardown; without memoization the second caller's short-circuit would let it race ahead to
  // `process.exit` while the FIRST caller's async reap/sweep/release was still in flight, cutting off the
  // lock release (an external SIGTERM then leaves the lockdir held). The shared promise makes both paths
  // block on the full teardown before any exit, so the mutex is ALWAYS released before exit.
  let teardownPromise = null;
  const teardown = () => {
    if (teardownPromise) return teardownPromise;
    teardownPromise = (async () => {
      // Only an ACQUIRING gate owns this runner target; a non-acquiring gate skips reap+sweep so it can
      // never touch the holder's (or any other) process tree.
      if (acquired) {
        try {
          const reap = await reapActiveStep();
          if (reap && reap.reaped && !reap.confirmedDead) {
            warn(
              "teardown could not CONFIRM the active step's process tree was reaped within the kill " +
                "budget — releasing the lock anyway to avoid a permanent hang, but the tree's death is " +
                "UNVERIFIED (a descendant may still be live). This is recorded, not claimed clean.",
            );
          }
        } catch {
          /* best-effort reap */
        }
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
  const installSignalTraps = () => {
    process.on("SIGINT", async () => {
      await teardown();
      process.exit(130);
    });
    process.on("SIGTERM", async () => {
      await teardown();
      process.exit(143);
    });
  };
  installSignalTraps();

  // Acquire the single-flight mutex FIRST. (`acquired` is declared above so teardown's closure reads it.)
  try {
    acquired = await mutex.acquire();
  } catch (e) {
    err(`mutex error: ${e.message}`);
    await teardown();
    process.exit(EXIT_USAGE);
  }
  if (!acquired) {
    err(`LOCK-REFUSED: ${mutex.refuseDetail} (lockdir=${lockdir})`);
    await teardown();
    process.exit(EXIT_LOCK_REFUSED);
  }
  log(`mutex acquired (token=${token} lockdir=${lockdir})`);
  log(`runner target dir: ${runnerTarget}`);

  const deadlineMs = nowMs() + opts.timeoutSecs * 1000;
  const stallMs = opts.stallSecs * 1000;

  let exitCode = EXIT_PASS;
  try {
    if (opts.mode === "prepare") {
      exitCode = await runPrepare({
        cargoEnv,
        repoRealpath,
        runnerTarget,
        gateDir,
        deadlineMs,
        stallMs,
      });
    } else {
      exitCode = await runGate(opts, {
        cargoEnv,
        repoRealpath,
        runnerTarget,
        gateDir,
        deadlineMs,
        stallMs,
      });
    }
  } catch (e) {
    err(`gate error: ${e && e.stack ? e.stack : e}`);
    exitCode = EXIT_USAGE;
  } finally {
    await teardown();
  }
  process.exit(exitCode);
}

// ----------------------------------------------------------------------------------------------------
// Archive + list — the shared front half of both the gate and --prepare. Returns the parsed list JSON +
// the extract dir, or an `{ error }` on setup/build failure.
// ----------------------------------------------------------------------------------------------------
async function archiveAndList(ctx) {
  const { cargoEnv, repoRealpath, runnerTarget, gateDir, deadlineMs, stallMs } = ctx;
  const archiveFile = join(gateDir, "nextest.tar.zst");
  const extractDir = join(gateDir, "extract");
  mkdirSync(gateDir, { recursive: true });
  // nextest's --extract-to canonicalizes the destination BEFORE extracting, so it must already exist.
  mkdirSync(extractDir, { recursive: true });

  // --- BUILD the whole workspace test universe ONCE (workspace unification => session_metrics ON) ---
  log("archiving workspace test universe (cargo nextest archive --workspace) …");
  const archiveRes = await runContainedStep({
    cmd: "cargo",
    args: [
      "nextest",
      "archive",
      "--workspace",
      "--archive-file",
      archiveFile,
      "--target-dir",
      runnerTarget,
      "--zstd-level",
      "-7",
    ],
    cwd: repoRealpath,
    env: cargoEnv,
    phase: "build",
    deadlineMs,
    stallMs,
    targetDir: runnerTarget,
  });
  if (archiveRes.reason) {
    return { error: mapStepReason(archiveRes), where: "archive", res: archiveRes };
  }
  if (archiveRes.code !== 0) {
    // Two distinct conditions hide behind a non-zero archive step:
    //   - the OS could not LAUNCH cargo (ENOENT/EACCES) => spawnError set => a SETUP/USAGE condition (127);
    //   - cargo RAN and the workspace failed to COMPILE => a real build failure, which per the exit
    //     contract is a GATE FAILURE (exit 1), NOT a setup error. A compile failure must be exit 1 so a
    //     red build is reported as a gate failure, not misclassified as a usage problem.
    if (archiveRes.spawnError) {
      err(`could not launch 'cargo' for the archive build (command not found / not executable)`);
      return { error: EXIT_USAGE, where: "archive", res: archiveRes };
    }
    // As with the list step: a SIGNAL-kill (signalName set, not a watchdog reason) is reported by its signal
    // name rather than the misleading synthesized "exit 128 — workspace did not compile". The verdict stays
    // a gate FAILURE (exit 1, fail-closed) either way — a build that was signal-killed is not a green build.
    err(
      archiveRes.signalName
        ? `cargo nextest archive build child terminated by signal ${archiveRes.signalName} (not a ` +
            "compile exit code) — workspace build did not complete"
        : `cargo nextest archive build failed (exit ${archiveRes.code}) — workspace did not compile`,
    );
    return { error: EXIT_FAIL, where: "archive", res: archiveRes };
  }
  log(`archive built in ${Math.round(archiveRes.durationMs / 1000)}s -> ${archiveFile}`);

  // --- LIST the suites from the archive (NO rebuild); JSON to a dedicated stdout capture ---
  log("listing suites from the archive (cargo nextest list --message-format json) …");
  const listRes = await runContainedStep({
    cmd: "cargo",
    args: [
      "nextest",
      "list",
      "--archive-file",
      archiveFile,
      "--extract-to",
      extractDir,
      "--extract-overwrite",
      "--workspace-remap",
      repoRealpath,
      "--message-format",
      "json",
    ],
    cwd: repoRealpath,
    env: cargoEnv,
    phase: "build", // extraction can be silent-ish; allow artifact-growth as progress
    deadlineMs,
    stallMs,
    targetDir: runnerTarget,
    captureStdoutSeparately: true, // keep JSON out of the mirrored stderr stream
  });
  if (listRes.reason) {
    return { error: mapStepReason(listRes), where: "list", res: listRes };
  }
  if (listRes.code !== 0) {
    // The list step reads an ALREADY-BUILT archive; a non-zero exit here is a setup/list failure (a corrupt
    // or unreadable archive, a nextest usage error, or — via spawnError — cargo not launchable). Either way
    // the gate cannot enumerate the suites, so it is a SETUP condition (127), not a build/test failure.
    //
    // Distinguish a SIGNAL-kill from a real nextest exit code. When the child was killed by an EXTERNAL
    // signal (signalName set) and this is NOT a watchdog TIMEOUT/STALL (reason already empty here — those
    // returned above), the synthesized "exit 128" is misleading: it implies nextest chose exit 128, but the
    // child was signal-killed (e.g. a flaky test binary SIGABRTing during `--list`). Report the SIGNAL name.
    err(
      listRes.spawnError
        ? `could not launch 'cargo' for the archive list (command not found / not executable)`
        : listRes.signalName
          ? `cargo nextest list child terminated by signal ${listRes.signalName} (not a nextest exit ` +
            "code) — cannot enumerate suites from the archive"
          : `cargo nextest list failed (exit ${listRes.code}) — cannot enumerate suites from the archive`,
    );
    return { error: EXIT_USAGE, where: "list", res: listRes };
  }
  let listJson;
  try {
    listJson = parseNextestListJson(listRes.stdout);
  } catch (e) {
    err(`could not parse nextest list JSON: ${e.message}`);
    return { error: EXIT_USAGE, where: "list-parse", res: listRes };
  }
  return { listJson, extractDir, archiveFile };
}

// ----------------------------------------------------------------------------------------------------
// --prepare: warm-pass. Run the archive build + list (the legitimate assessment pre-warm) and pre-touch
// the built binaries once (a one-shot first-launch that warms the macOS Gatekeeper assessment cache via
// the legitimate first-launch path). This is a PRE-WARM, not a gate PASS and not a cost removal: it does
// NOT disable Gatekeeper; it only moves the legitimate first-launch assessment earlier, out of a timed
// gate. STRICT warm counting: only `status === 0` counts as warmed; a non-zero/unexpected status during
// the warm `--list` is REPORTED (warn), never counted as success, and a warm-list FAILURE in ANY suite
// makes the prepare a fail-setup (exit 127) rather than silently swallowing it. On success it prints the
// `PREPARED_NOT_GATE` marker (and NO `PASS` token) so it is NEVER confused with a gate VERDICT PASS — a CI
// `grep PASS` of prepare's output cannot mistake the warm-pass for a verdict.
// ----------------------------------------------------------------------------------------------------
async function runPrepare(ctx) {
  const out = await archiveAndList(ctx);
  if (out.error) return out.error;
  const { listJson, extractDir } = out;
  const buildMetaTargetDir =
    listJson["rust-build-meta"] && listJson["rust-build-meta"]["target-directory"];
  const suites = Object.values(listJson["rust-suites"] || {});
  // One-shot warm: launch each suite binary with --list (no test execution) so the OS first-launch
  // assessment for that binary is performed now via the legitimate path. STRICT: a successful warm is
  // EXACTLY `status === 0`; libtest's `--list` exits 0 on success, so 0 is the only success code here. A
  // non-zero status, a signal, or a missing/unresolvable binary is a warm FAILURE — reported, never
  // counted as warmed, and it makes the whole prepare a fail-setup.
  let warmed = 0;
  let warmFailures = 0;
  let missing = 0;
  for (const s of suites) {
    const bin = resolveSuiteBinary(s["binary-path"], buildMetaTargetDir, extractDir);
    if (!bin || !existsSync(bin)) {
      missing++;
      warn(
        `prepare: suite binary not found for ${s["binary-id"] || "?"} (path=${s["binary-path"]}) — ` +
          "cannot warm its first-launch assessment",
      );
      continue;
    }
    const r = spawnSync(bin, ["--list"], { encoding: "utf8", windowsHide: true, timeout: 30000 });
    if (r.status === 0) {
      warmed++;
    } else {
      warmFailures++;
      const how = r.signal
        ? `signal ${r.signal}`
        : r.status === null
          ? "no exit status (spawn/timeout)"
          : `exit ${r.status}`;
      warn(
        `prepare: warm '--list' of ${s["binary-id"] || bin} did NOT exit 0 (${how}) — NOT counted as ` +
          "warmed (a warm-list failure is reported, never swallowed as success)",
      );
    }
  }
  if (warmFailures > 0 || missing > 0) {
    // STRICT warm counting: a warm-list failure / missing binary is NEVER swallowed as success — it is a
    // fail-setup (exit 127). This is NOT a gate verdict (the gate is `node scripts/gate.mjs`); it means
    // prepare could not complete its warm-pass.
    err(
      `prepare: ${warmFailures} warm-list failure(s) + ${missing} missing binary/-ies — a warm FAILURE is ` +
        "not swallowed; reporting fail-setup (exit 127). This is NOT a gate verdict, it is an incomplete warm.",
    );
    return EXIT_USAGE;
  }
  // PREPARED_NOT_GATE — the success output. It is unmistakably NOT a gate pass: the marker is
  // PREPARED_NOT_GATE and NO line contains the token `PASS` (so a CI `grep PASS` of prepare's output cannot
  // mistake it for a gate verdict). The lines are produced + guarded centrally in gate-internals.mjs.
  for (const line of preparedSuccessLines(suites.length, warmed, warmFailures, missing)) {
    log(line);
  }
  return EXIT_PASS;
}

// ----------------------------------------------------------------------------------------------------
// runGate: the full canonical gate.
//   1. archive (build ONCE) + list (parse rust-suites).
//   2. SURFACE 1 — nextest run from the archive (process isolation).
//   3. SURFACE 2 — directly exec every verter_session suite (kind ∈ {lib,test}) with cwd = its package
//      manifest dir (the in-process / libtest surface). ZERO recompile (reads the archived artifacts).
//   4. Aggregate failures across both surfaces; tolerated-only => PASS-WITH-TOLERATED.
// ----------------------------------------------------------------------------------------------------
async function runGate(opts, ctx) {
  const { cargoEnv, repoRealpath, runnerTarget, deadlineMs, stallMs } = ctx;

  // ---------- FRESHNESS-TOOLING PREFLIGHT (verdict-gating authority) ----------
  // BEFORE the archive build (and inside the held mutex + containment model), self-ensure the typeinfo
  // freshness tools (`buf` + `oxfmt`). The two byte-equality freshness tests regenerate the committed TS
  // proto bindings through those binaries; in a fresh `git worktree` nobody runs `pnpm install`, so the
  // tools can be absent — and with `buf` absent the Rust byte-pin pair SKIPS-and-PASSES (no FAIL line). The
  // gate ensures the tooling so the byte-pin RUNS GENUINELY; it must NOT blanket-tolerate the env-only pair,
  // because a GENUINE drift (tools present, bindings stale, the test RUNS and FAILS) shares those two names. It
  // installs the frozen lockfile here and DISABLES the freshness tolerance when the tools are present, so a
  // present/installed run treats a freshness FAIL as a HARD regression. Tolerance stays ENABLED only when
  // pnpm is not resolvable AND `buf` is not resolvable (the Rust byte-pin would skip). `oxfmt` absence NEVER
  // grants tolerance — with `buf` present, a missing `oxfmt` is a LOUD setup failure (a degraded un-oxfmt'd
  // byte-compare can false-positive). A deterministic install failure (e.g. a frozen-lockfile mismatch) also
  // FAILS LOUD as a setup error — never PASS-WITH-TOLERATED.
  //
  // `pnpm install --frozen-lockfile` runs through `runContainedStep` so it inherits the SAME whole-gate
  // deadline + stall + teardown the cargo steps use (NOT an unbounded pre-mutex mutation). `--frozen-
  // lockfile` never mutates the lockfile, so CI (deps already installed) makes the preflight a cheap no-op.
  const preflight = await preflightFreshnessTooling({
    repoRoot: repoRealpath,
    // The preflight's tool RESOLVER must see the SAME PATH the Rust test execution sees: `cargoEnv`
    // (built at the top of runGate via `buildCargoEnv(process.env, …)`) has had implicit-CWD PATH
    // components stripped, so neither the preflight verdict NOR the executed cargo/nextest/libtest tests
    // resolve a tool from CWD. Passing the RAW `process.env` here would let the preflight's empty-PATH-
    // skip decide "buf absent ⇒ tolerate" while the test (which honors an empty PATH component as CWD)
    // resolves a CWD buf and RUNS — a fail-open where a real stale-binding regression is tolerated.
    env: cargoEnv,
    runInstall: ({ pnpmPath }) => {
      // Resolve the platform-correct `pnpm install --frozen-lockfile` launch from the RESOLVED `pnpmPath`
      // the preflight proved on PATH (the single source of truth — never a bare `pnpm` token). On Windows
      // the resolved path is a `.cmd` shim that Node's `spawn(…, { shell:false })` cannot launch directly,
      // so `pnpmInstallCommand` routes the QUOTED resolved path through a VERIFIED ABSOLUTE command processor
      // (a case-insensitive absolute existing `ComSpec`, else `<SystemRoot>\System32\cmd.exe`) — a real
      // `.exe` that spawns directly and stays the reapable tree root for `runContainedStep`'s `taskkill /T`
      // teardown — with `windowsVerbatimArguments` so Node does not re-quote it. On POSIX it launches the
      // resolved path DIRECTLY (no PATH re-search). Either way the containment model is unchanged — we keep
      // the explicit command so the spawn remains `shell:false`.
      const launch = pnpmInstallCommand(pnpmPath);
      // On Windows `pnpmInstallCommand` SETUP-FAILS when no absolute command processor (a verified absolute
      // `ComSpec`, else `<SystemRoot>\System32\cmd.exe`) can be resolved — it returns `{ setupFail, detail }`
      // instead of a launch shape, refusing to spawn a bare/relative `cmd.exe`. Reuse the EXISTING setup-fail
      // rail: synthesize a `runContainedStep`-shaped result with `spawnError: true`, which
      // `preflightFreshnessTooling` already maps to action "setup-fail" ⇒ EXIT_USAGE (FAIL LOUD), rather than
      // inventing a new error protocol.
      if (launch.setupFail) {
        return {
          code: EXIT_USAGE,
          reason: "",
          durationMs: 0,
          stdout: "",
          stderr: launch.detail,
          spawnError: true,
        };
      }
      const { cmd, args, windowsVerbatimArguments } = launch;
      return runContainedStep({
        cmd,
        args,
        windowsVerbatimArguments,
        cwd: repoRealpath,
        env: cargoEnv,
        phase: "build", // install can be silent-ish while it links — allow artifact-growth as progress
        deadlineMs,
        stallMs,
        targetDir: runnerTarget,
      });
    },
  });
  if (preflight.action === "setup-fail") {
    err(`gate setup failed ensuring freshness tooling: ${preflight.detail}`);
    return EXIT_USAGE;
  }
  if (preflight.action === "watchdog" && preflight.installRes && preflight.installRes.reason) {
    err(
      `pnpm install ${preflight.installRes.reason} after ` +
        `${Math.round((preflight.installRes.durationMs || 0) / 1000)}s while ensuring freshness tooling`,
    );
    return mapStepReason(preflight.installRes);
  }
  const freshnessToleranceAllowed = preflight.freshnessToleranceAllowed;
  log(
    `freshness-tooling preflight: ${preflight.action} — tolerance ${freshnessToleranceAllowed ? "ALLOWED (pnpm not resolvable AND buf not resolvable; the Rust byte-pin would skip)" : "DISABLED (tools present/installed; a freshness FAIL is a HARD regression)"}`,
  );

  // This CLI has NO self-test seam and NO ambient-env divert: runGate ALWAYS issues the real archive
  // build + nextest run + direct libtest execution. (The reusable multi-step seam lives in
  // gate-internals.mjs and is driven ONLY by the self-test, in-process — never reachable from this CLI.)
  const out = await archiveAndList(ctx);
  if (out.error) {
    err(`gate setup failed at the ${out.where} step`);
    return out.error;
  }
  const { listJson, extractDir, archiveFile } = out;
  const buildMetaTargetDir =
    listJson["rust-build-meta"] && listJson["rust-build-meta"]["target-directory"];
  const allSuites = Object.values(listJson["rust-suites"] || {});
  log(
    `archive lists ${allSuites.length} suites; build-meta target-directory=${buildMetaTargetDir || "?"}`,
  );

  // Aggregate verdict accumulators.
  const failures = []; // { surface, name }
  let toleratedOccurred = false;
  let hardSetupFail = false;

  // ---------- SURFACE 1: nextest run from the archive (process isolation) ----------
  log("SURFACE 1: nextest run from the archive (process isolation) …");
  const runArgs = [
    "nextest",
    "run",
    "--archive-file",
    archiveFile,
    "--extract-to",
    extractDir,
    "--extract-overwrite",
    "--workspace-remap",
    repoRealpath,
  ];
  if (opts.noFailFast) runArgs.push("--no-fail-fast");
  const runRes = await runContainedStep({
    cmd: "cargo",
    args: runArgs,
    cwd: repoRealpath,
    env: cargoEnv,
    phase: "test", // TEST phase: byte-growth-only liveness (a silent test binary is a hang)
    deadlineMs,
    stallMs,
    targetDir: runnerTarget,
  });
  if (runRes.reason) {
    err(`nextest run ${runRes.reason} after ${Math.round(runRes.durationMs / 1000)}s`);
    return mapStepReason(runRes);
  }
  const nextestText = runRes.stdout + "\n" + runRes.stderr;
  // SURFACE-1 verdict via the shared analyzer (the same code the self-test drives in-process). It consults
  // the run exit code + the summary `failed` total, NOT just the `FAIL [` lines, so a crash
  // (SIGABRT/SIGSEGV/LEAK/TIMEOUT/…) or a setup/harness error in ANY crate fails the gate.
  const s1 = analyzeNextestSurface(nextestText, runRes.code, freshnessToleranceAllowed);
  for (const f of s1.failures) failures.push(f);
  if (s1.toleratedCount > 0) toleratedOccurred = true;
  log(
    `SURFACE 1 done in ${Math.round(runRes.durationMs / 1000)}s: ` +
      `${s1.summary.passed} passed, ${s1.summary.failed} failed ` +
      `(${s1.namedCount} named, ${s1.toleratedCount} tolerated), ${s1.summary.skipped} skipped; ` +
      `run exit ${runRes.code}`,
  );

  // ---------- SURFACE 2: direct verter_session libtest execution (in-process surface) ----------
  const sel = selectSessionSuites(allSuites);
  if (sel.error) {
    err(`SURFACE 2 SETUP FAILURE: ${sel.error}`);
    return EXIT_USAGE;
  }
  const sessionSuites = sel.suites;
  log(
    `SURFACE 2: directly executing ${sessionSuites.length} verter_session libtest binaries ` +
      `(lib=${sel.lib}, test=${sel.test}) in-process from the SAME archive …`,
  );
  let s2Passed = 0;
  let s2Failed = 0;
  let s2Tolerated = 0;
  // Package identity derived from the archive list JSON (NOT a separate `cargo metadata` subprocess that
  // would escape the watchdog). All session suites share one package, so derive once from the first.
  const sessionPkgInfo = deriveSuitePkgInfo(sessionSuites[0]);
  for (const s of sessionSuites) {
    const remaining = deadlineMs - nowMs();
    if (remaining <= 0) {
      warn(
        `whole-gate budget exhausted before verter_session suite '${s["binary-id"]}' => TIMEOUT`,
      );
      return EXIT_TIMEOUT;
    }
    const bin = resolveSuiteBinary(s["binary-path"], buildMetaTargetDir, extractDir);
    if (!bin || !existsSync(bin)) {
      err(
        `SURFACE 2: suite binary not found for ${s["binary-id"]} (path=${s["binary-path"]}) — setup failure`,
      );
      hardSetupFail = true;
      continue;
    }
    // cwd = the package manifest dir (what Cargo sets). nextest reports it as the suite's `cwd`; defend
    // against a missing/extract-relative value by falling back to <repo>/crates/verter_session.
    const cwd = s.cwd && existsSync(s.cwd) ? s.cwd : join(repoRealpath, "crates", "verter_session");
    // The directly-executed binary needs the runtime Cargo env these tests read — CARGO_MANIFEST_DIR
    // (tests resolve the repo root + read corpus fixtures through it) and CARGO_TARGET_DIR (already on the
    // base cargo env). cwd IS the manifest dir. See buildSuiteEnv for the verified-complete scope.
    const suiteEnv = buildSuiteEnv(
      cargoEnv,
      cwd,
      sessionPkgInfo,
      s["binary-name"] || "verter_session",
    );
    // Preserve the libtest DEFAULT threading (do NOT force --test-threads=1). Optionally pass an explicit
    // passthrough if the caller asked for it.
    const binArgs = [];
    if (opts.testThreads != null) binArgs.push(`--test-threads=${opts.testThreads}`);
    const res = await runContainedStep({
      cmd: bin,
      args: binArgs,
      cwd,
      env: suiteEnv, // the runtime Cargo env this suite reads (CARGO_MANIFEST_DIR + CARGO_TARGET_DIR)
      phase: "test", // TEST phase: byte-growth-only liveness
      deadlineMs,
      stallMs,
      targetDir: runnerTarget,
      captureStdoutSeparately: true, // keep libtest stdout parseable; still mirror stderr
    });
    if (res.reason) {
      err(
        `SURFACE 2: suite ${s["binary-id"]} ${res.reason} after ${Math.round(res.durationMs / 1000)}s`,
      );
      return mapStepReason(res);
    }
    const libText = res.stdout + "\n" + res.stderr;
    // SURFACE-2 verdict via the shared analyzer (the same code the self-test drives in-process). A
    // tolerated direct-libtest failure is admitted ONLY under NORMAL libtest failure semantics — exit 101
    // (not a signal/abort), a parsed `test result: FAILED` summary whose `failed` count EXACTLY equals the
    // parsed FAILED names, and every name allowlisted. A crash (signal), a missing summary, or any
    // unaccounted failure is a HARD FAILURE.
    const a2 = analyzeLibtestSurface(libText, res.code, s["binary-id"], freshnessToleranceAllowed);
    if (a2.verdict === "pass") {
      s2Passed++;
    } else if (a2.verdict === "tolerated") {
      s2Passed++;
      s2Tolerated += a2.toleratedNames.length;
      toleratedOccurred = true;
    } else {
      for (const f of a2.failures) failures.push(f);
      s2Failed++;
    }
  }
  log(
    `SURFACE 2 done: ${s2Passed} suites clean, ${s2Failed} suites with non-tolerated failures, ${s2Tolerated} tolerated test failures`,
  );

  // ---------- Aggregate verdict ----------
  if (hardSetupFail) {
    err(
      "VERDICT: FAIL (a verter_session suite binary was missing from the archive — setup integrity failure)",
    );
    return EXIT_FAIL;
  }
  if (failures.length > 0) {
    err(`VERDICT: FAIL — ${failures.length} non-tolerated failure(s):`);
    for (const f of failures.slice(0, 50)) err(`  [${f.surface}] ${f.name}`);
    return EXIT_FAIL;
  }
  if (toleratedOccurred) {
    log(
      "VERDICT: PASS-WITH-TOLERATED (only the env-only typeinfo_proto_ts_freshness pair produced an actual " +
        "FAIL line, by exact name, AND the freshness-tooling preflight proved pnpm is not resolvable AND buf " +
        "is not resolvable, so the pair is tolerated. This is the LATENT-net path: the normal buf-less runner " +
        "SKIPS the Rust byte-pin (no FAIL line) and reaches the ordinary PASS below — this branch fires only " +
        "when the pair somehow FAILED despite buf being absent. When the tools are present/installed this pair " +
        "is a HARD failure)",
    );
    return EXIT_PASS;
  }
  log("VERDICT: PASS (both surfaces green)");
  return EXIT_PASS;
}

main().catch((e) => {
  err(`fatal: ${e && e.stack ? e.stack : e}`);
  process.exit(EXIT_USAGE);
});
