#!/usr/bin/env node
// Canonical Rust gate CLI.
//
// SECURITY: this binary runs only the real gate (oracle → archive →
// nextest → direct libtest → verdict). `--prepare` is a warm utility,
// never a gate PASS. No test-seam, classifier hook, custom-command
// mode, or env var can make this CLI return the success contract without
// building and running the suite. Internals live in `gate-internals.mjs`;
// the self-test imports them in-process, never via a flag here.
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
//   Builds the whole workspace test universe via `cargo nextest archive` and runs THREE verification
//   surfaces. Surfaces 1 and 2 share ONE dev-profile archive; surface 3 has its own archive because it
//   requires a different Cargo profile (see SHIPPED-CFG SURFACE below):
//     1. nextest run (per-test PROCESS ISOLATION) — surfaces nothing that survives a fork, catches the
//        ordinary regression set.
//     2. the verter_session libtest binaries executed DIRECTLY (in-process / multi-test-per-process) —
//        surfaces shared-process state bugs the isolated path cannot.
//     3. nextest run over a SECOND archive built with `debug_assertions` OFF — surfaces behaviour that
//        differs between the debug test build and every shipped artifact.
//   Every build command issued is a `--workspace` archive build, so the gate NEVER issues the
//   package-scoped `cargo test -p verter_session` resolution and so structurally cannot incur the
//   recompile that resolution caused (see "Canonical feature set" below).
//
// SHIPPED-CFG SURFACE (surface 3) — WHAT IT COVERS AND WHAT IT DOES NOT
//   `debug_assert!` does NOT evaluate its argument when `debug_assertions` is off, and
//   `#[cfg(debug_assertions)]` items do not exist there. Every shipped artifact (the LSP binary, napi,
//   wasm) is built that way. So a side effect written inside a `debug_assert!` argument — the real,
//   shipped shape being `debug_assert!(session.commit_completed())`, where the call performs a state
//   transition — executes in every debug test and in NO shipped build.
//   NOTHING ELSE IN THIS REPO SEES THAT. Surfaces 1 and 2 are debug builds, so the effect happens and the
//   tests pass. `cargo check --workspace --release` compiles the shipped cfg but RUNS NOTHING, so it
//   cannot observe a runtime no-op. Only running tests with `debug_assertions` off makes it observable.
//   Surface 3 therefore builds the workspace test universe again under the `no-debug-assertions` profile
//   (declared in the workspace Cargo.toml: `debug_assertions` off + `overflow-checks` off, dev codegen
//   otherwise) and RUNS the `package(verter_session) + package(verter_scheduler)` filterset from it. The
//   same profile split also turns a cross-crate item whose availability was gated on `debug_assertions`
//   into a COMPILE error here, instead of a shipped-build surprise.
//   NOT COVERED, explicitly: this is not an optimised build. The profile inherits dev codegen (opt-level
//   0, no LTO, many codegen units), so optimisation-, inlining- and LTO-dependent behaviour is out of
//   scope. It runs under nextest process isolation only — there is no in-process shipped-cfg pass
//   equivalent to surface 2 — and it runs the filterset above, not the whole archive, so a
//   `debug_assertions`-dependent regression in a package outside that filterset is not covered.
//   COST: surface 3 adds a second whole-workspace compile (a different profile is a different unit hash,
//   so no artifact is shared with the dev archive) plus the filtered run.
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
//   5. Whole-gate hard timeout (default 80m, --timeout) — a deadline for the ENTIRE gate, not per-step. It
//      covers BOTH archive builds and all three surfaces. On
//      expiry the active step's tree is reaped + a sweep runs; exit 124.
//   6. Stall detector with SEPARATE build vs test phases:
//        BUILD phase (the archive build): progress = stdout/stderr byte growth OR runner-owned target-tree
//          artifact growth (file-count + newest-mtime, bounded scan). A long silent rustc is NOT a stall.
//        TEST phase (the nextest run + the direct libtest execs): progress = stdout/stderr byte growth
//          ONLY. Target-tree growth is NOT a valid test liveness signal; a silent test binary IS a hang.
//      Default stall 12m (--stall). On stall: reap + sweep; exit 125.
//   7. Spotlight marker (macOS): a <runnerTarget>/.metadata_never_index file is written so Spotlight does
//      not index the build tree (a harmless no-op file on Linux/Windows).
//   8. Resource ceiling: build jobs and test concurrency are finite (defaults: min(host CPUs, 4)); every
//      contained child tree is sampled for aggregate RSS once per second and is reaped when it reaches
//      the gate memory ceiling (default: 50% of physical RAM). A ceiling trip is the distinct, non-PASS
//      `ABORTED — memory ceiling` outcome (exit 123). Repeated sampler failure also aborts rather than
//      silently running unmonitored.
//   9. Terminal-outcome accounting: a test that did not PASS fails the gate and is NAMED, whatever its
//      outcome class. nextest reports several non-`FAIL` terminal outcomes — `N timed out`, `N exec
//      failed`, a crash status (SIGABRT/SIGSEGV/LEAK-FAIL/…) — and reports a cancelled or interrupted run
//      as `A/B tests run`, meaning B-A selected tests NEVER EXECUTED. None of those are in nextest's
//      `failed` count, so the verdict is derived from `runCount - passed` (label-independent: an outcome
//      class this gate has never heard of still lands there) plus the unrun count. A run whose only
//      problem is a timeout is a FAIL, never a PASS: a timed-out test has not passed, it has not even
//      finished.
//      TRUST MODEL. The verdict rests on nextest's own counts, not on the names: `runCount - passed`
//      cannot be lowered by anything a test prints, whereas the status lines share a stream with
//      captured test output and are forgeable. Naming is therefore advisory and the count is
//      authoritative; the one route from `failures exist in the log` to a green verdict is the
//      tolerance allowlist, so tolerance is refused outright whenever a failure was superseded by a
//      pass. Residual, named rather than claimed away: no text-level rule can fully separate runner
//      output from test output on a shared stream - see GI-19 in docs/arch/gate-integrity-ledger.md.
//      NAMING, and its honest limit. Failing tests are listed by name with their status, including the
//      compound (`FAIL + LEAK`) and retried (`TRY 3 FAIL`, `TRY 3 FL+LK`) status fields, with the LAST
//      status per test deciding — so a flaky test that failed attempt 1 and passed attempt 2 is not a
//      failure, and three attempts of one test are one failure. But naming is best-effort where the
//      VERDICT is not: a status spelling the parser does not recognise is not named, and surfaces through
//      the unaccounted tripwire as `<run exit N; unaccounted failure(s) …>` instead. That still FAILS the
//      gate — no silent pass — it just costs the operator the test's name. The recognised vocabulary is
//      pinned in `NEXTEST_FAILURE_STATUSES` + `classifyNextestStatusField` from nextest's own status
//      literals; widen it there, and only there, if a future nextest adds a spelling.
//
// BUILD-PREREQUISITE PREFLIGHT (gate mode only; runs FIRST, before everything below)
//   Parts of the Rust suite load artifacts CARGO DOES NOT BUILD. The real-provider suites spawn the pinned
//   `tsserver` with `--globalPlugins @verter/typescript-plugin --pluginProbeLocations
//   packages/vue-vscode/node_modules`; that probe dir is a pnpm symlink to `packages/typescript-plugin`,
//   whose `main` is `dist/index.js` — a `tsc -b` OUTPUT that `pnpm install` does NOT produce. With the
//   symlink present but the `dist` absent, tsserver loads no plugin, cannot resolve `.vue`/`.svelte`
//   carriers, and ~64 `*_tsserver` tests fail with `TS2307: Cannot find module './Comp.vue' or its
//   corresponding type declarations.` — sixty-four opaque failures that read exactly like a compiler
//   regression. CLAUDE.md's "Verification Must Prove Execution (MANDATORY)" requires a gate to prove
//   "required source, build, and fixture prerequisites matched the tested tree"; a gate that cannot tell
//   "the code is broken" from "an artifact was never built" fails that rule.
//   So as its FIRST step — before the freshness preflight, before cargo, before any test — the gate LOADS
//   that plugin entry in a child process (`require()` of the probe directory, exactly what tsserver
//   resolves) and, on any load failure, FAILS CLOSED with exit 127 naming the probe target, the load
//   error, the producing packages and the exact producer command (marker: `BUILD-PREREQUISITE MISSING`).
//   A REAL LOAD, not a list of files to stat: the entry eagerly requires its emitted helpers and
//   `@verter/language-shared`'s entry re-exports a dozen emitted siblings, so a stat list mirrors the emit
//   graph and drifts — a tree with both `index.js` files present and one helper missing satisfies every
//   stat and still throws inside tsserver. The load proves the transitive closure RESOLVES; it does NOT
//   prove freshness, and a stale-but-loadable dist is a separate, deliberately out-of-scope problem.
//   It does NOT build the artifacts (the verdict must not depend on a mutation the gate performed) and
//   does NOT skip the affected tests (with no install at all those tests SKIP, the silent-pass half of the
//   same rule). It precedes the freshness preflight because that preflight's `pnpm install` is precisely
//   what converts the silent-skip state into the 64-failure state. `--prepare` is exempt: it builds the
//   archive and runs no test.
//   Two workspace packages produce the closure: the plugin, and `@verter/language-shared`.
//   `@verter/native` is deliberately NOT among them (the plugin's `"files": ["src/index.ts"]` excludes
//   `src/tsc/`, its only consumer), so the gate never demands a `napi build --release`.
//
// ORACLE-CACHE PREREQUISITE PREFLIGHT (gate mode only; runs SECOND, right after the build-prerequisite
// preflight above)
//   The `verter_session/bf2-authoritative` feature (now ON for every archive — see `ARCHIVE_FEATURES` in
//   gate-internals.mjs) gates 45 tests, including the ENTIRE `svelte_official_conformance_gate` suite —
//   the tests that compare Verter's Svelte output against the pinned official `svelte@5.56.8` oracle.
//   Those tests realize their Vue/Svelte oracles OFFLINE from a gitignored local npm cache
//   (`.oracle-npm-cache`, warmed from the network ONLY by the explicit, never-automatic
//   `node packages/framework-conformance-harness/scripts/provision-oracle-npm-cache.mjs`). A fresh checkout has no cache, and the harness does
//   NOT fail loudly when it cannot realize an oracle: it records the affected axis as skipped and keeps
//   comparing every other axis — an environment absence that reads as a compiled-output divergence.
//   Same shape as the build-prerequisite preflight: a REAL LOAD, not a stat. The probe calls the SAME
//   `ensureOracleDomain` the suite's own `bin/check-candidate.mjs` calls, which validates the realized
//   `.oracle-installs` tree against the committed lockfile's closure — an absent OR an invalid (present
//   but unusable — corrupt, torn, or drifted) cache both FAIL SETUP loudly (exit 127, marker
//   `ORACLE-CACHE PREREQUISITE MISSING`), naming the exact provisioning command. This is REALIZATION
//   (offline, idempotent — the same automatic step every `bf2-authoritative` test already performs), never
//   PROVISIONING (the networked step, which stays an explicit human/CI action the gate never runs).
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
//             · watchdog (MEMORY/TIMEOUT/STALL)               → propagated, never tolerated.
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
//   node scripts/gate.mjs [--timeout 80m] [--stall 12m] [--target-dir <DIR>] [--no-fail-fast]
//                         [--build-jobs N] [--test-threads N] [--memory-limit 12GiB]
//                                                           # THE GATE — exit 0 = suite built + passed.
//   node scripts/gate.mjs --prepare [--target-dir <DIR>] [--timeout 80m] [--stall 12m]
//                         [--build-jobs N] [--memory-limit 12GiB]
//                                             # warm-pass: archive + list (+ a one-shot warm of the macOS
//                                             # first-launch assessment). Prints the `PREPARED_NOT_GATE`
//                                             # marker — this is a PRE-WARM (tests were NOT run), NOT a gate
//                                             # pass, and is not counted in a timed gate. --prepare combines
//                                             # ONLY with the flags shown; any other flag or a positional
//                                             # argument is a usage error (exit 127).
//   node scripts/gate.mjs --help              # prints this usage + exits 0. Accepts no other argument
//                                             # (a bare --help only); --help with any other token => 127.
//     durations: s/m/h suffix or bare seconds (e.g. 80m, 12m, 5s, 90).
//
//   This CLI accepts ONLY the flags above. There is intentionally NO test-seam / classifier-hook /
//   custom-command mode — every accepted invocation either runs the real gate, runs the `--prepare`
//   warm-pass, or prints help. An unknown flag is a USAGE error (exit 127), never a silent success.
//
// EXIT CODES (distinct, documented; exit 0 is OPERATION-scoped — see OPERATION-SCOPED EXIT SEMANTICS above)
//   0   PASS / PASS-WITH-TOLERATED  (the GATE: a real `node scripts/gate.mjs` run); OR a successful
//       --prepare warm-pass (PREPARED_NOT_GATE — NOT a gate pass); OR --help after printing usage
//   1   FAIL          (a build/test command failed / a non-tolerated test failed)
//   123 ABORTED       (active child tree reached the memory ceiling, or its RSS monitor became unavailable)
//   124 TIMEOUT       (whole-gate wallclock deadline tripped)
//   125 STALL         (no progress within the stall window)
//   126 LOCK-REFUSED  (another gate holds the single-flight mutex and is alive / lock uninspectable)
//   127 USAGE/SETUP   (bad arguments, repo root not found, a MISSING BUILD PREREQUISITE, a missing/invalid
//                      ORACLE-CACHE PREREQUISITE, archive/list setup failure)
//
// ENV VARS HONORED
//   VERTER_GATE_LOCK / MOM_GATE_LOCK   lockdir path (default: OS temp dir keyed by repo realpath)
//   VERTER_GATE_TARGET_DIR             runner-owned target dir (default <repo>/target/gate-runner)
//   CARGO_TARGET_DIR / CARGO_BUILD_TARGET_DIR / CARGO_BUILD_BUILD_DIR are SCRUBBED and forced to the
//     runner-owned dir.
//   CARGO_BUILD_JOBS is SCRUBBED and forced to --build-jobs (default min(host CPUs, 4)).
//   (No environment variable can divert this CLI to a non-gate success path.)

import { readdirSync, realpathSync, statSync } from "node:fs";
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
  deriveGateResourceLimits,
  parseMemorySize,
  formatMemorySize,
  // mutex + teardown
  Mutex,
  reapActiveStep,
  provenanceSweep,
  // contained step + analysis
  runContainedStep,
  mapStepReason,
  analyzeNextestSurface,
  analyzeLibtestSurface,
  // gate telemetry (report-only; see the "GATE TELEMETRY" section of gate-internals.mjs)
  collectNextestTestTimings,
  summarizeNextestTimings,
  parseLibtestSummary,
  // build-prerequisite preflight (the non-cargo artifacts the suite loads from disk)
  checkBuildPrerequisites,
  probeBudgetMs,
  // oracle-cache prerequisite preflight (the offline Svelte/Vue oracle npm cache the bf2-authoritative
  // conformance suites realize from)
  checkOracleCachePrerequisite,
  oracleCacheProbeBudgetMs,
  // shipped-cfg archive/filter builders — feature parity across every archive, and the package presence
  // guard extended to every package the filterset names
  buildNextestArchiveArgs,
  buildShippedCfgFilter,
  SHIPPED_CFG_EXTRA_PACKAGES,
  checkPackagesPresentInArchive,
  // trybuild exclusion (interim, pending maintainer disposition) — filter builder + per-surface coverage
  // guard, shared by every surface so a stale row fails loud instead of silently under/over-excluding
  TRYBUILD_EXCLUDED_SUITES,
  buildTrybuildExclusionFilterExpr,
  trybuildSkipArgsForPackage,
  countTrybuildExclusionMatches,
  // freshness-tooling preflight (verdict-gating authority)
  preflightFreshnessTooling,
  pnpmInstallCommand,
  vueMacroOracleGateCommands,
  selectSessionSuites,
  ensureRequiredWindowsDebugSidecars,
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

const OVERSIZE_SOURCE_LINE_LIMIT = 1500;
const OVERSIZE_SOURCE_EXEMPTIONS = new Set([
  "crates/verter_session/src/typeinfo/typeinfo_tests/oracle_query_specs.rs",
  "crates/verter_session/src/host_manage/prepared_decl.rs",
  "crates/verter_compiler/src/compile/template_data.rs",
  "crates/verter_compiler/src/svelte/runtime/entity_table.rs",
  "crates/verter_compiler/src/svelte/runtime/diff_oracle_divergences.rs",
  "crates/verter_compiler/src/svelte/runtime/expr.rs",
  "crates/verter_compiler/src/svelte/parser/tokenizer.rs",
  "crates/verter_compiler/src/ide/template/mod.rs",
  "crates/verter_compiler/src/template/code_gen/ssr/mod.rs",
  "crates/verter_compiler/src/template/code_gen/vapor/mod.rs",
  "crates/verter_compiler/src/template/code_gen/vdom/element.rs",
  "crates/verter_compiler/src/template/code_gen/vdom/slots.rs",
  "crates/verter_compiler/src/tsc/script.rs",
  "crates/verter_ffi/src/convert.rs",
  "crates/verter_lsp/src/config.rs",
  "crates/verter_lsp/src/features/completion.rs",
  "crates/verter_lsp/src/server/sync_orchestration.rs",
  "crates/verter_lsp/src/workspace_scanner.rs",
  "crates/verter_mcp/src/server.rs",
  "crates/verter_napi/src/lib.rs",
  "crates/verter_parser/src/parser/mod.rs",
  "crates/verter_parser/src/tokenizer/byte.rs",
  "crates/verter_parser/src/utils/oxc/bindings/helpers.rs",
  "crates/verter_parser/src/utils/oxc/vue/script/setup.rs",
  "crates/verter_parser/src/utils/oxc/vue/script/usage.rs",
  "crates/verter_protocol/src/component_meta.rs",
  "crates/verter_scheduler/src/scheduler.rs",
  "crates/verter_scheduler/src/dag.rs",
  "crates/verter_semantic/src/analysis/build.rs",
  "crates/verter_semantic/src/analysis/component_meta.rs",
  "crates/verter_semantic/src/analysis/html_intrinsics_data.rs",
  "crates/verter_semantic/src/analysis/macros.rs",
  "crates/verter_semantic/src/analysis/style.rs",
  "crates/verter_semantic/src/analysis/template.rs",
  "crates/verter_semantic/src/analysis/type_eval_build.rs",
  "crates/verter_semantic/src/analysis/type_solver/prepared.rs",
  "crates/verter_semantic/src/analysis/types.rs",
  "crates/verter_session/src/component_meta_audit/mod.rs",
  "crates/verter_session/src/component_meta_caches.rs",
  "crates/verter_session/src/component_meta_materialize.rs",
  "crates/verter_session/src/file_artifact_store.rs",
  "crates/verter_session/src/host_manage.rs",
  "crates/verter_session/src/host_manage/analysis_io.rs",
  "crates/verter_session/src/host_manage/component_meta_extract.rs",
  "crates/verter_session/src/host_manage/component_meta_methods.rs",
  "crates/verter_session/src/host_resolve.rs",
  "crates/verter_session/src/host_resolve/virtual_file_pipeline.rs",
  "crates/verter_session/src/resolver_core/mod.rs",
  "crates/verter_session/src/resolver_store.rs",
  "crates/verter_session/src/meta_resolve/materialize/field_types.rs",
  "crates/verter_session/src/meta_resolve/materialize/macro_shapes.rs",
  "crates/verter_session/src/meta_resolve/projectors/mod.rs",
  "crates/verter_session/src/parse.rs",
  "crates/verter_session/src/request_context.rs",
  "crates/verter_session/src/project_semantic_dispatch/build.rs",
  "crates/verter_session/src/project_semantic_dispatch/lower.rs",
  "crates/verter_session/src/project_semantic_dispatch/mod.rs",
  "crates/verter_session/src/project_semantic_dispatch/raise.rs",
  "crates/verter_session/src/project_type_store.rs",
  "crates/verter_session/src/decl_body_memo.rs",
  "crates/verter_session/src/host_manage/eval_env.rs",
  "crates/verter_session/src/meta_resolve/slot_binding_graph.rs",
  "crates/verter_type_expr/src/facts.rs",
  "crates/verter_session/src/resolver_core/component_meta.rs",
  "crates/verter_session/src/resolver_core/component_meta_registry.rs",
  "crates/verter_session/src/resolver_core/external_type_frontier.rs",
  "crates/verter_session/src/resolver_core/fallthrough.rs",
  "crates/verter_session/src/resolver_core/shallow_file_state.rs",
  "crates/verter_session/src/semantic_query.rs",
  "crates/verter_session/src/semantic_query_memo/mod.rs",
  "crates/verter_session/src/semantic_query_memo/arena.rs",
  "crates/verter_session/src/semantic_query_memo/derivation.rs",
  "crates/verter_session/src/semantic_query_memo/family.rs",
  "crates/verter_session/src/semantic_query_memo/inflight.rs",
  "crates/verter_session/src/semantic_query_memo/interner.rs",
  "crates/verter_session/src/semantic_query_memo/stats.rs",
  "crates/verter_session/src/semantic_query_memo/tests.rs",
  "crates/verter_session/src/types.rs",
  "crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs",
  "crates/verter_tsc/src/checker.rs",
  "crates/verter_type_runtime/src/tsgo/ipc.rs",
  "crates/verter_type_runtime/src/tsserver/ipc.rs",
  "crates/verter_workspace/src/resolver.rs",
  "crates/verter_session/src/project_semantic_dispatch/walk.rs",
  "crates/verter_wasm/src/lib.rs",
]);

function countSourceLines(source) {
  if (source.length === 0) return 0;
  let lines = source.endsWith("\n") ? 0 : 1;
  for (let i = 0; i < source.length; i++) {
    if (source.charCodeAt(i) === 10) lines++;
  }
  return lines;
}

function directoryIdentity(abs, entry) {
  if (entry && !entry.isDirectory() && !entry.isSymbolicLink()) return null;
  try {
    if (!statSync(abs).isDirectory()) return null;
    return realpathSync(abs);
  } catch {
    return null;
  }
}

function collectOversizeProductionSources(repoRoot) {
  const violations = [];
  const stack = [];
  const cratesRoot = join(repoRoot, "crates");

  for (const crateEntry of readdirSync(cratesRoot, { withFileTypes: true })) {
    const crateAbs = join(cratesRoot, crateEntry.name);
    if (directoryIdentity(crateAbs, crateEntry) === null) continue;
    const rel = `crates/${crateEntry.name}/src`;
    const abs = join(crateAbs, "src");
    if (!existsSync(abs)) continue;
    const identity = directoryIdentity(abs);
    if (identity !== null) stack.push({ abs, rel, ancestors: new Set([identity]) });
  }

  while (stack.length > 0) {
    const current = stack.pop();
    let entries;
    try {
      entries = readdirSync(current.abs, { withFileTypes: true });
    } catch {
      continue;
    }
    for (const entry of entries) {
      const abs = join(current.abs, entry.name);
      const rel = `${current.rel}/${entry.name}`;
      const identity = directoryIdentity(abs, entry);
      if (identity !== null) {
        if (["tests", "benches", "examples", "target"].includes(entry.name)) continue;
        // Branch-local identities stop symlink cycles without suppressing a distinct alias path.
        if (current.ancestors.has(identity)) continue;
        const ancestors = new Set(current.ancestors);
        ancestors.add(identity);
        stack.push({ abs, rel, ancestors });
        continue;
      }
      if (!entry.name.endsWith(".rs")) continue;
      if (entry.name === "tests.rs" || entry.name.endsWith("_tests.rs")) continue;
      let source;
      try {
        source = readFileSync(abs, "utf8");
      } catch {
        continue;
      }
      const lines = countSourceLines(source);
      if (lines > OVERSIZE_SOURCE_LINE_LIMIT && !OVERSIZE_SOURCE_EXEMPTIONS.has(rel)) {
        violations.push([rel, lines]);
      }
    }
  }

  violations.sort(([left], [right]) => (left < right ? -1 : left > right ? 1 : 0));
  return violations;
}

function reportOversizeProductionSources(repoRoot) {
  let violations;
  try {
    violations = collectOversizeProductionSources(repoRoot);
  } catch (error) {
    warn(`oversize-source advisory could not scan the production tree: ${error.message}`);
    return;
  }
  if (violations.length === 0) return;

  const rows = violations.map(([rel, lines]) => `${rel} (${lines} lines)`).join("\n  ");
  warn(
    `Oversize source advisory: production source files exceed ${OVERSIZE_SOURCE_LINE_LIMIT} lines\n` +
      `without an explicit exemption:\n  ${rows}\n\n` +
      "File size is advisory and does not affect the gate verdict.",
  );
}

// ----------------------------------------------------------------------------------------------------
// GATE TELEMETRY (report-only). Every function here only LOGS; none of it is consulted by any verdict
// path. See the "GATE TELEMETRY" section in gate-internals.mjs for where the underlying data comes from.
// ----------------------------------------------------------------------------------------------------

// Sums the on-disk size of every suite binary a nextest archive listing names, resolved the SAME way the
// gate resolves them to run them (`resolveSuiteBinary`). Best-effort: a binary that cannot be resolved or
// stat'd is counted as `missing`, never thrown.
function computeExtractedBinarySizes(listJson, extractDir) {
  const buildMetaTargetDir =
    listJson["rust-build-meta"] && listJson["rust-build-meta"]["target-directory"];
  const suites = Object.values(listJson["rust-suites"] || {});
  let totalBytes = 0;
  let resolved = 0;
  let missing = 0;
  for (const s of suites) {
    const bin = resolveSuiteBinary(s["binary-path"], buildMetaTargetDir, extractDir);
    if (bin && existsSync(bin)) {
      try {
        totalBytes += statSync(bin).size;
        resolved++;
        continue;
      } catch {
        /* fall through to missing */
      }
    }
    missing++;
  }
  return { totalBytes, resolved, missing };
}

// Reports per-package / per-binary cumulative test duration + the 50 heaviest test families for a nextest
// surface, derived from the SAME captured stdout+stderr the gate already parses for pass/fail (see the
// "GATE TELEMETRY" section in gate-internals.mjs). Grouped into one block so the numbers that answer
// "where did the time go" sit together rather than interleaved with the pass/fail log lines.
function logNextestTimingReport(label, text, allSuites) {
  const timings = collectNextestTestTimings(text);
  const report = summarizeNextestTimings(timings, allSuites, 50);
  log(
    `${label} TIMING: ${report.timedCount}/${report.totalTests} terminal test(s) carried a parseable ` +
      `duration, summing to ${report.totalSec.toFixed(1)}s of reported per-test time (tests run process- ` +
      "isolated and concurrently, so this sum is NOT the surface's wall-clock).",
  );
  log(`${label} TIMING — cumulative duration by package (${report.perPackage.length} package(s)):`);
  for (const p of report.perPackage) {
    log(`  ${p.key}: ${p.count} test(s), ${p.totalSec.toFixed(1)}s`);
  }
  log(`${label} TIMING — cumulative duration by binary (${report.perBinary.length} binary/-ies):`);
  for (const b of report.perBinary) {
    log(`  ${b.key}: ${b.count} test(s), ${b.totalSec.toFixed(1)}s`);
  }
  log(`${label} TIMING — top ${report.topFamilies.length} highest cumulative-time test families:`);
  for (const f of report.topFamilies) {
    log(`  ${f.totalSec.toFixed(2)}s (${f.count} test(s)) ${f.key}`);
  }
}

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
//     --timeout, --stall, --build-jobs, --memory-limit, each with its value); ANY other flag (e.g.
//     --no-fail-fast / --test-threads — gate-only) or ANY positional token is a USAGE error (exit 127).
//     `gate.mjs --prepare junk` /
//     `--prepare --selftest-x` exit 127, so prepare's exit-0 cannot be reached with junk argv.
// The gate mode (no mode flag) accepts the full real-gate flag set.
// ----------------------------------------------------------------------------------------------------

// Flags --prepare is allowed to combine with (the warm-pass front half — archiveAndList — reads exactly
// these). Each takes a value argument. Gate-only flags (--no-fail-fast / --test-threads) are NOT here, so
// `--prepare --no-fail-fast` is a usage error rather than a silently-ignored flag.
const PREPARE_ALLOWED_VALUE_FLAGS = new Set([
  "--target-dir",
  "--timeout",
  "--stall",
  "--build-jobs",
  "--memory-limit",
]);

function usageError(msg) {
  return new Error(msg);
}

function parsePositiveInteger(value, flag) {
  const parsed = Number(value);
  if (!/^\d+$/.test(String(value || "")) || !Number.isSafeInteger(parsed) || parsed < 1) {
    throw usageError(`${flag} requires a positive integer`);
  }
  return parsed;
}

function defaultOptions(mode) {
  const resources = deriveGateResourceLimits();
  return {
    mode,
    timeoutSecs: parseDuration("80m"),
    stallSecs: parseDuration("12m"),
    targetDir: process.env.VERTER_GATE_TARGET_DIR || "",
    noFailFast: true,
    buildJobs: resources.buildJobs,
    testThreads: resources.testThreads,
    memoryLimitBytes: resources.memoryLimitBytes,
  };
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
    return defaultOptions("help");
  }

  // --prepare is mutually exclusive: it combines ONLY with its warm-pass companion flags
  // (--target-dir/--timeout/--stall/--build-jobs/--memory-limit) and accepts NO positional argument and
  // NO gate-only flag. This is the
  // non-gate warm-pass; rejecting stray argv keeps its exit-0 unreachable with junk arguments.
  if (argv.includes("--prepare")) {
    const opts = defaultOptions("prepare");
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
        else if (a === "--build-jobs") opts.buildJobs = parsePositiveInteger(v, a);
        else if (a === "--memory-limit") opts.memoryLimitBytes = parseMemorySize(v);
      } else {
        throw usageError(
          `--prepare accepts only --target-dir/--timeout/--stall/--build-jobs/--memory-limit ` +
            `(and no positional argument); got ` +
            `'${a}'. --prepare is the warm-pass, NOT the gate — gate-only flags and stray tokens are rejected.`,
        );
      }
      i++;
    }
    return opts;
  }

  // Gate mode — the real-gate flag set. No mode flag, so the gate-pass contract applies.
  const opts = defaultOptions("gate");
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
    } else if (a === "--build-jobs") {
      opts.buildJobs = parsePositiveInteger(argv[++i], a);
    } else if (a === "--test-threads") {
      opts.testThreads = parsePositiveInteger(argv[++i], a);
    } else if (a === "--memory-limit") {
      opts.memoryLimitBytes = parseMemorySize(argv[++i]);
    } else {
      throw usageError(
        `unknown argument: '${a}'. This gate accepts only --timeout/--stall/--target-dir/` +
          `--no-fail-fast/--build-jobs/--test-threads/--memory-limit/--prepare/--help; ` +
          `it has no test-seam or custom-command mode.`,
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

  reportOversizeProductionSources(repoRealpath);

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
  const cargoEnv = buildCargoEnv(process.env, runnerTarget, undefined, opts.buildJobs);

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
  log(
    `resource ceiling: cargo build jobs=${opts.buildJobs}, ` +
      `test threads=${opts.mode === "prepare" ? "n/a (prepare runs no tests)" : opts.testThreads}, ` +
      `active child-tree RSS=${formatMemorySize(opts.memoryLimitBytes)}`,
  );

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
        buildJobs: opts.buildJobs,
        memoryLimitBytes: opts.memoryLimitBytes,
      });
    } else {
      exitCode = await runGate(opts, {
        cargoEnv,
        repoRealpath,
        runnerTarget,
        gateDir,
        deadlineMs,
        stallMs,
        buildJobs: opts.buildJobs,
        memoryLimitBytes: opts.memoryLimitBytes,
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
// Build VARIANTS. Each is one whole-workspace test universe, built by the SAME `cargo nextest archive
// --workspace` command and enumerated by the SAME `cargo nextest list` — one archive file and one extract
// dir per variant, so the two never share artifacts or a listing. There is no second discovery mechanism:
// a variant only chooses the Cargo profile.
//
//   DEBUG          — the default dev profile; surfaces 1 and 2 run from it.
//   SHIPPED_CFG    — the `no-debug-assertions` profile (see the workspace Cargo.toml): `debug_assertions`
//                    off and `overflow-checks` off, i.e. the conditional-compilation and runtime-check
//                    state of every shipped artifact, at dev codegen cost. Surface 3 runs from it.
// ----------------------------------------------------------------------------------------------------
const VARIANT_DEBUG = {
  key: "debug",
  cargoProfile: null, // cargo's default (dev)
  archiveName: "nextest.tar.zst",
  extractName: "extract",
  label: "workspace test universe (dev profile)",
};
const VARIANT_SHIPPED_CFG = {
  key: "shipped-cfg",
  cargoProfile: "no-debug-assertions",
  archiveName: "nextest-no-debug-assertions.tar.zst",
  extractName: "extract-no-debug-assertions",
  label: "workspace test universe (no-debug-assertions profile: debug_assertions OFF)",
};

// SURFACE 3 selection. `debug_assertions` decides what a `debug_assert!` argument does and which
// `cfg(debug_assertions)` items exist, so the surface is only meaningful where the tree actually uses
// those constructs to guard state the tests observe. `verter_session`, `verter_scheduler`, and the
// compiled-output conformance crates + `verter_compiler` own that state, and running them is what makes
// surface 3 a RUN rather than a compile check. The expression is a nextest filterset evaluated against the
// release-cfg archive's own listing; built from `SHIPPED_CFG_EXTRA_PACKAGES` (gate-internals.mjs) so the
// filter string and the presence guard below can never drift apart.
const SHIPPED_CFG_FILTER = buildShippedCfgFilter();

// TRYBUILD EXCLUSION — interim, pending maintainer disposition (see TRYBUILD_EXCLUDED_SUITES in
// gate-internals.mjs for why). Applied to every surface: SURFACE 1 ANDs this bare filter onto its
// otherwise-unfiltered `--workspace` selection; SURFACE 3 ANDs it onto SHIPPED_CFG_FILTER; SURFACE 2 uses
// the per-package `--skip` form directly (it runs a libtest binary, not nextest, so it has no `-E`).
const TRYBUILD_EXCLUSION_FILTER = buildTrybuildExclusionFilterExpr();
const SHIPPED_CFG_FILTER_NO_TRYBUILD = `(${SHIPPED_CFG_FILTER}) and (${TRYBUILD_EXCLUSION_FILTER})`;

// Shared coverage guard: verify every registered trybuild row matches real work in THIS archive's own
// listing, log the exclusion LOUDLY (count + reason + filter string, never a silent skip), and return the
// verified counts. Returns `{ error }` when a row went stale (zero matches) — the caller must fail closed.
function verifyTrybuildExclusionCoverage(allSuites, surfaceLabel) {
  const trybuild = countTrybuildExclusionMatches(allSuites);
  if (trybuild.missing.length > 0) {
    return {
      error:
        `TRYBUILD EXCLUSION SETUP FAILURE (${surfaceLabel}): the following registered row(s) matched ZERO ` +
        "tests in this archive's own listing — a trybuild file was renamed, moved, or removed without " +
        "updating TRYBUILD_EXCLUDED_SUITES in scripts/gate-internals.mjs: " +
        trybuild.missing.map((m) => `package(${m.package}) test(/^${m.modulePrefix}/)`).join(", ") +
        ". Refusing to run an exclusion filter that cannot prove it still excludes real tests.",
    };
  }
  log(
    `TRYBUILD EXCLUSION (${surfaceLabel}, INTERIM — pending maintainer disposition, not deletion): ` +
      `excluding ${trybuild.total} trybuild compile-fail harness test(s) across ${TRYBUILD_EXCLUDED_SUITES.length} ` +
      "registered file(s) in 6 crates (one trybuild::TestCases::new() invocation spawns a cargo build of " +
      "the crate's full dependency closure — 98s cold / 0.8s warm measured, not a unit test). Still runnable " +
      "directly; not deleted, not feature-gated. filter: '" +
      TRYBUILD_EXCLUSION_FILTER +
      "'",
  );
  return { trybuild };
}

// ----------------------------------------------------------------------------------------------------
// Archive + list — the shared front half of the gate, --prepare, and surface 3. Returns the parsed list
// JSON + the extract dir, or an `{ error }` on setup/build failure. `variant` selects the Cargo profile
// and the per-variant archive/extract paths; everything else (the command, the flags, the parsing, the
// watchdog model) is identical across variants.
// ----------------------------------------------------------------------------------------------------
async function archiveAndList(ctx, variant = VARIANT_DEBUG) {
  const {
    cargoEnv,
    repoRealpath,
    runnerTarget,
    gateDir,
    deadlineMs,
    stallMs,
    buildJobs,
    memoryLimitBytes,
  } = ctx;
  const archiveFile = join(gateDir, variant.archiveName);
  const extractDir = join(gateDir, variant.extractName);
  mkdirSync(gateDir, { recursive: true });
  // nextest's --extract-to canonicalizes the destination BEFORE extracting, so it must already exist.
  mkdirSync(extractDir, { recursive: true });

  // --- BUILD the whole workspace test universe ONCE (workspace unification => session_metrics ON;
  // ARCHIVE_FEATURES => verter_session/bf2-authoritative ON, so the 45 oracle-backed conformance tests are
  // PRESENT in the archive both debug surfaces and surface 3 build from — see buildNextestArchiveArgs) ---
  log(`archiving ${variant.label} (cargo nextest archive --workspace) …`);
  const archiveRes = await runContainedStep({
    cmd: "cargo",
    args: buildNextestArchiveArgs({
      buildJobs,
      cargoProfile: variant.cargoProfile,
      archiveFile,
      runnerTarget,
    }),
    cwd: repoRealpath,
    env: cargoEnv,
    phase: "build",
    deadlineMs,
    stallMs,
    targetDir: runnerTarget,
    memoryLimitBytes,
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
        ? `cargo nextest archive build [${variant.key}] child terminated by signal ` +
            `${archiveRes.signalName} (not a compile exit code) — workspace build did not complete`
        : `cargo nextest archive build [${variant.key}] failed (exit ${archiveRes.code}) — ` +
            `workspace did not compile under ${variant.cargoProfile || "the dev profile"}`,
    );
    return { error: EXIT_FAIL, where: "archive", res: archiveRes };
  }
  log(
    `archive [${variant.key}] built in ${Math.round(archiveRes.durationMs / 1000)}s -> ${archiveFile}`,
  );
  // TELEMETRY (report-only): the archive step's own successful peak RSS is measured internally by the
  // watchdog and normally discarded once the step succeeds; the archive's on-disk size costs one stat().
  let archiveSizeBytes = 0;
  try {
    archiveSizeBytes = statSync(archiveFile).size;
  } catch {
    /* best-effort */
  }
  log(
    `archive [${variant.key}] TELEMETRY: size ${formatMemorySize(archiveSizeBytes)}, peak RSS ` +
      `${formatMemorySize(archiveRes.peakRssBytes)} across ${archiveRes.memoryProcessCount} process(es)`,
  );

  // --- LIST the suites from the archive (NO rebuild); JSON to a dedicated stdout capture ---
  log(
    `listing suites from the [${variant.key}] archive (cargo nextest list --message-format json) …`,
  );
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
    memoryLimitBytes,
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
  // TELEMETRY (report-only): the list/extract step's own successful peak RSS (also discarded on success
  // today) + the total size of every suite binary this archive extracted.
  const extractedSizes = computeExtractedBinarySizes(listJson, extractDir);
  log(
    `list [${variant.key}] TELEMETRY: peak RSS ${formatMemorySize(listRes.peakRssBytes)} across ` +
      `${listRes.memoryProcessCount} process(es); extracted binaries ` +
      `${formatMemorySize(extractedSizes.totalBytes)} total across ${extractedSizes.resolved} binary/-ies` +
      `${extractedSizes.missing > 0 ? ` (${extractedSizes.missing} unresolved)` : ""}`,
  );
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

async function runVueMacroOracleChecks(ctx) {
  const { cargoEnv, repoRealpath, runnerTarget, deadlineMs, stallMs, memoryLimitBytes } = ctx;
  for (const invocation of vueMacroOracleGateCommands(process.execPath)) {
    log(`Vue macro oracle: ${invocation.name} …`);
    const result = await runContainedStep({
      cmd: invocation.cmd,
      args: invocation.args,
      cwd: repoRealpath,
      env: cargoEnv,
      phase: "test",
      deadlineMs,
      stallMs,
      targetDir: runnerTarget,
      memoryLimitBytes,
    });
    if (result.reason) {
      err(`${invocation.name} ${result.reason} after ${Math.round(result.durationMs / 1000)}s`);
      return mapStepReason(result);
    }
    if (result.code !== 0) {
      err(`${invocation.name} failed (exit ${result.code})`);
      return EXIT_FAIL;
    }
    log(`${invocation.name} passed in ${Math.round(result.durationMs / 1000)}s`);
  }
  return EXIT_PASS;
}

// ----------------------------------------------------------------------------------------------------
// SURFACE 3 — the shipped-cfg surface. Builds the whole workspace test universe a SECOND time under the
// `no-debug-assertions` profile (`debug_assertions` off, `overflow-checks` off — the conditional-
// compilation and runtime-check state of every shipped artifact) and RUNS the selected tests from it.
//
// WHAT ONLY THIS SURFACE CAN SEE. `debug_assert!` does not evaluate its argument when `debug_assertions`
// is off. A side effect written inside that argument — `debug_assert!(session.commit_completed())`, where
// the call performs a state transition — runs in every debug test and in NO shipped build. Surfaces 1 and
// 2 are debug builds, so the effect happens there and every test passes. `cargo check --workspace
// --release` compiles the shipped cfg but runs nothing, so it cannot see it either. Only executing tests
// with `debug_assertions` off makes the no-op observable. The same profile split also makes a cross-crate
// item gated on `debug_assertions` a COMPILE error here rather than a shipped-build surprise, because a
// dependent's test code is compiled against the same profile as the dependency.
//
// WHAT IT DOES NOT COVER, stated plainly: it is not an optimised build. The profile inherits `dev`
// codegen (opt-level 0, no LTO, many codegen units), so optimisation-, inlining-, and LTO-dependent
// behaviour is out of scope, as is anything specific to release's `panic`/codegen settings. It also runs
// under nextest process isolation only — there is no in-process shipped-cfg pass equivalent to surface 2 —
// and it runs the filterset below, not the whole archive.
// ----------------------------------------------------------------------------------------------------
async function runShippedCfgSurface(opts, ctx, freshnessToleranceAllowed) {
  const { cargoEnv, repoRealpath, runnerTarget, deadlineMs, stallMs, memoryLimitBytes } = ctx;

  log(
    "SURFACE 3: building the shipped-cfg test universe " +
      "(cargo nextest archive --workspace --cargo-profile no-debug-assertions) …",
  );
  const out = await archiveAndList(ctx, VARIANT_SHIPPED_CFG);
  if (out.error) {
    err(`SURFACE 3 setup failed at the ${out.where} step`);
    return { exit: out.error };
  }
  const { listJson, extractDir, archiveFile } = out;
  const allSuites = Object.values(listJson["rust-suites"] || {});

  // SELECTION INTEGRITY. The filterset must match real work in THIS archive's own listing. A filter that
  // silently selects nothing would let surface 3 report a green run having executed zero tests, which is
  // the "selectors matched non-zero work" failure the gate contract forbids. Assert the packages the
  // filterset names are present with the target kinds BEFORE the run, and assert a non-zero run count
  // AFTER it.
  const sel = selectSessionSuites(allSuites);
  if (sel.error) {
    err(`SURFACE 3 SETUP FAILURE: ${sel.error}`);
    return { exit: EXIT_USAGE };
  }
  // Every OTHER package the filterset names (verter_scheduler + the conformance crates + verter_compiler,
  // ruling §10) gets the same generic non-zero-suites guard — a typo or a renamed/removed crate fails
  // loud instead of the filterset silently matching nothing for that package.
  const extra = checkPackagesPresentInArchive(allSuites, SHIPPED_CFG_EXTRA_PACKAGES);
  if (extra.missing.length > 0) {
    err(
      `SURFACE 3 SETUP FAILURE: zero lib/test suites in the shipped-cfg archive listing for: ` +
        `${extra.missing.join(", ")} — but the filterset names ` +
        `${extra.missing.map((pkg) => `package(${pkg})`).join(" + ")} (full filterset: '${SHIPPED_CFG_FILTER}'). ` +
        "Refusing to run a filterset that cannot match.",
    );
    return { exit: EXIT_USAGE };
  }
  log(
    `SURFACE 3: shipped-cfg archive lists ${allSuites.length} suites; filterset '${SHIPPED_CFG_FILTER}' ` +
      `covers verter_session (lib=${sel.lib}, test=${sel.test}) + ` +
      `${SHIPPED_CFG_EXTRA_PACKAGES.map((pkg) => `${pkg} (${extra.counts[pkg]} suites)`).join(" + ")}`,
  );

  const trybuildCov = verifyTrybuildExclusionCoverage(allSuites, "SURFACE 3");
  if (trybuildCov.error) {
    err(trybuildCov.error);
    return { exit: EXIT_USAGE };
  }

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
    "-E",
    SHIPPED_CFG_FILTER_NO_TRYBUILD,
  ];
  if (opts.noFailFast) runArgs.push("--no-fail-fast");
  runArgs.push("--test-threads", String(opts.testThreads));
  const runRes = await runContainedStep({
    cmd: "cargo",
    args: runArgs,
    cwd: repoRealpath,
    env: cargoEnv,
    phase: "test", // TEST phase: byte-growth-only liveness (a silent test binary is a hang)
    deadlineMs,
    stallMs,
    targetDir: runnerTarget,
    memoryLimitBytes,
  });
  if (runRes.reason) {
    err(`SURFACE 3 nextest run ${runRes.reason} after ${Math.round(runRes.durationMs / 1000)}s`);
    return { exit: mapStepReason(runRes) };
  }
  const s3 = analyzeNextestSurface(
    runRes.stdout + "\n" + runRes.stderr,
    runRes.code,
    freshnessToleranceAllowed,
  );
  if (s3.summary.runCount === 0) {
    err(
      `SURFACE 3 SETUP FAILURE: the filterset '${SHIPPED_CFG_FILTER_NO_TRYBUILD}' selected ZERO tests to ` +
        "run in the shipped-cfg archive. A surface that executes nothing proves nothing; refusing to pass it.",
    );
    return { exit: EXIT_USAGE };
  }
  log(
    `SURFACE 3 done in ${Math.round(runRes.durationMs / 1000)}s: ` +
      `${s3.summary.runCount}${s3.summary.unrun > 0 ? `/${s3.summary.initialCount}` : ""} run, ` +
      `${s3.summary.passed} passed, ${s3.summary.nonPassed} did not pass ` +
      `(${s3.summary.failed} failed, ${s3.summary.timedOut} timed out, ${s3.summary.execFailed} exec failed` +
      `${s3.summary.unrun > 0 ? `, ${s3.summary.unrun} NEVER RAN` : ""}) ` +
      `(${s3.namedCount} named, ${s3.toleratedCount} tolerated), ${s3.summary.skipped} skipped; ` +
      `run exit ${runRes.code}`,
  );
  log(
    `SURFACE 3 TELEMETRY: peak RSS ${formatMemorySize(runRes.peakRssBytes)} across ` +
      `${runRes.memoryProcessCount} process(es)`,
  );
  logNextestTimingReport("SURFACE 3", runRes.stdout + "\n" + runRes.stderr, allSuites);
  return { failures: s3.failures, tolerated: s3.toleratedCount > 0 };
}

// ----------------------------------------------------------------------------------------------------
// runGate: the full canonical gate.
//   0. Verify the non-cargo BUILD PREREQUISITES the suite loads from disk.
//   1. Verify the pinned Vue macro oracle and its extractor.
//   2. archive (build ONCE, dev profile) + list (parse rust-suites).
//   3. SURFACE 1 — nextest run from the archive (process isolation).
//   4. SURFACE 2 — directly exec every verter_session suite (kind ∈ {lib,test}) with cwd = its package
//      manifest dir (the in-process / libtest surface). ZERO recompile (reads the archived artifacts).
//   5. SURFACE 3 — archive the workspace AGAIN under the `no-debug-assertions` profile (shipped
//      `cfg(debug_assertions)` state) and run the selected tests from it.
//   6. Aggregate failures across all three surfaces; tolerated-only => PASS-WITH-TOLERATED.
// ----------------------------------------------------------------------------------------------------
async function runGate(opts, ctx) {
  const { cargoEnv, repoRealpath, runnerTarget, deadlineMs, stallMs, memoryLimitBytes } = ctx;

  // ---------- BUILD-PREREQUISITE PREFLIGHT (the FIRST step of the gate) ----------
  // Parts of the suite load artifacts cargo does not build: the real-provider suites spawn the pinned
  // tsserver with `--globalPlugins @verter/typescript-plugin`, whose entry is a `tsc -b` output that
  // `pnpm install` does NOT produce. Without it tsserver resolves no carrier and ~64 `*_tsserver` tests
  // fail with `TS2307: Cannot find module './Comp.vue'` — indistinguishable, from the gate's output, from
  // a real compiler regression.
  //
  // The oracle is a REAL LOAD of that plugin entry in a child process, NOT a list of files to stat: the
  // entry eagerly requires its emitted helpers and `@verter/language-shared`'s entry re-exports a dozen
  // emitted siblings, so a stat list is a mirror of the emit graph that drifts (both `index.js` present +
  // one helper missing passes every stat and still throws inside tsserver). It proves resolvability, NOT
  // freshness — a stale-but-loadable dist is a separate, deliberately out-of-scope problem.
  //
  // Ordering: this is the first step of the gate proper. It runs BEFORE `preflightFreshnessTooling` ON
  // PURPOSE — that preflight may `pnpm install`, and the install is exactly what converts the SILENT-SKIP
  // state (no node_modules ⇒ no tsserver ⇒ the affected tests skip ⇒ a green gate that proved nothing)
  // into the LOUD-FAILURE state. Checking first catches both with one actionable message. (The mutex,
  // the runner target dir and the whole-gate deadline are established by `main` before runGate is
  // entered; this precedes every install, every cargo step and every test, not every statement.)
  // See `checkBuildPrerequisites` for why the gate refuses to build or to skip.
  // The probe is bounded by the GATE's remaining wallclock, not by its own constant: it runs with the
  // single-flight mutex held, so a probe that could outlive `--timeout` would hold the lock past the
  // deadline that is supposed to release it.
  const prerequisites = checkBuildPrerequisites({
    repoRoot: repoRealpath,
    timeoutMs: probeBudgetMs(deadlineMs, nowMs()),
  });
  if (!prerequisites.ok) {
    for (const line of prerequisites.lines) err(line);
    return EXIT_USAGE;
  }
  log(`build-prerequisite preflight: SATISFIED — ${prerequisites.target} loaded`);

  // ---------- ORACLE-CACHE PREREQUISITE PREFLIGHT (the gate's SECOND step) ----------
  // `verter_session/bf2-authoritative` (now ON for every archive — see `buildNextestArchiveArgs`) gates 45
  // tests, including the ENTIRE `svelte_official_conformance_gate` suite: the tests that actually compare
  // Verter's Svelte output against the pinned official `svelte@5.56.8` oracle. Those tests realize their
  // Vue/Svelte oracles OFFLINE from a gitignored local npm cache (`.oracle-npm-cache`, warmed from the
  // network ONLY by the explicit `node packages/framework-conformance-harness/scripts/provision-oracle-npm-cache.mjs`, never by this gate). An
  // absent or unusable cache does not make those tests fail loudly on their own: the harness records the
  // affected axis as skipped and keeps comparing every other axis, so a missing cache reads as compiled-
  // output DIVERGENCES, not as a setup problem.
  // Like the build-prerequisite preflight above, the oracle is a REAL LOAD, not a stat: this calls the
  // SAME `ensureOracleDomain` the suite's own `bin/check-candidate.mjs` calls on every request, which
  // validates the realized closure against the committed lockfile (paths, names, versions, edges, per-
  // package content digests). It runs BEFORE the archive build, same bounded-by-the-gate's-own-deadline
  // model as the build-prerequisite probe (still holding the single-flight mutex).
  const oraclePrereq = checkOracleCachePrerequisite({
    repoRoot: repoRealpath,
    env: cargoEnv,
    timeoutMs: oracleCacheProbeBudgetMs(deadlineMs, nowMs()),
  });
  if (!oraclePrereq.ok) {
    for (const line of oraclePrereq.lines) err(line);
    return EXIT_USAGE;
  }
  log(
    `oracle-cache prerequisite preflight: SATISFIED — realized ${JSON.stringify(oraclePrereq.realized)}`,
  );

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
        memoryLimitBytes,
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

  const vueMacroOracleResult = await runVueMacroOracleChecks(ctx);
  if (vueMacroOracleResult !== EXIT_PASS) return vueMacroOracleResult;

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
  const sidecars = ensureRequiredWindowsDebugSidecars({
    allSuites,
    runnerTarget: ctx.runnerTarget,
    extractDir,
  });
  if (sidecars.error) {
    err(`Windows archive debug-sidecar setup failed: ${sidecars.error}`);
    return EXIT_USAGE;
  }
  if (sidecars.copied > 0) {
    log(
      `restored ${sidecars.copied} runtime-required Windows PDB sidecar(s) beside archived tests`,
    );
  }
  log(
    `archive lists ${allSuites.length} suites; build-meta target-directory=${buildMetaTargetDir || "?"}`,
  );

  const trybuildCov1 = verifyTrybuildExclusionCoverage(allSuites, "SURFACE 1+2 (dev archive)");
  if (trybuildCov1.error) {
    err(trybuildCov1.error);
    return EXIT_USAGE;
  }

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
    "-E",
    TRYBUILD_EXCLUSION_FILTER,
  ];
  if (opts.noFailFast) runArgs.push("--no-fail-fast");
  runArgs.push("--test-threads", String(opts.testThreads));
  const runRes = await runContainedStep({
    cmd: "cargo",
    args: runArgs,
    cwd: repoRealpath,
    env: cargoEnv,
    phase: "test", // TEST phase: byte-growth-only liveness (a silent test binary is a hang)
    deadlineMs,
    stallMs,
    targetDir: runnerTarget,
    memoryLimitBytes,
  });
  if (runRes.reason) {
    err(`nextest run ${runRes.reason} after ${Math.round(runRes.durationMs / 1000)}s`);
    return mapStepReason(runRes);
  }
  const nextestText = runRes.stdout + "\n" + runRes.stderr;
  // SURFACE-1 verdict via the shared analyzer (the same code the self-test drives in-process). It consults
  // the run exit code + the summary's run-but-did-not-pass total (`runCount - passed`), NOT just the
  // `FAIL [` lines, so a crash (SIGABRT/SIGSEGV/LEAK-FAIL/…), a TIMEOUT, an `exec failed`, a cancelled run
  // that left tests unexecuted, or a setup/harness error in ANY crate fails the gate — and each such test
  // is NAMED in the verdict, not folded into an opaque "unaccounted" line.
  const s1 = analyzeNextestSurface(nextestText, runRes.code, freshnessToleranceAllowed);
  for (const f of s1.failures) failures.push(f);
  if (s1.toleratedCount > 0) toleratedOccurred = true;
  log(
    `SURFACE 1 done in ${Math.round(runRes.durationMs / 1000)}s: ` +
      `${s1.summary.runCount}${s1.summary.unrun > 0 ? `/${s1.summary.initialCount}` : ""} run, ` +
      `${s1.summary.passed} passed, ${s1.summary.nonPassed} did not pass ` +
      `(${s1.summary.failed} failed, ${s1.summary.timedOut} timed out, ${s1.summary.execFailed} exec failed` +
      `${s1.summary.unrun > 0 ? `, ${s1.summary.unrun} NEVER RAN` : ""}) ` +
      `(${s1.namedCount} named, ${s1.toleratedCount} tolerated), ${s1.summary.skipped} skipped; ` +
      `run exit ${runRes.code}`,
  );
  log(
    `SURFACE 1 TELEMETRY: peak RSS ${formatMemorySize(runRes.peakRssBytes)} across ` +
      `${runRes.memoryProcessCount} process(es)`,
  );
  logNextestTimingReport("SURFACE 1", nextestText, allSuites);

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
  // TELEMETRY (report-only) accumulators: Surface 2 is the one surface whose duration and executed-test
  // count were previously never reported at all — the runner only counted how many suite binaries passed.
  const s2Details = [];
  let s2TotalDurationMs = 0;
  let s2TotalTests = 0;
  let s2PeakRssBytes = 0;
  let s2PeakProcessCount = 0;
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
    // Use the same explicit finite concurrency as the nextest surfaces. The suites still run sequentially;
    // this caps the worker threads within the currently active shared-process libtest binary.
    // `--skip <prefix>` (NOT `--exact`, verified: `--exact` also makes `--skip` require exact equality and
    // stops it matching a module-path prefix) removes the trybuild exclusion rows for this package from a
    // DIRECT libtest run — this binary is invoked without nextest, so it never sees `-E`.
    const binArgs = [
      `--test-threads=${opts.testThreads}`,
      ...trybuildSkipArgsForPackage("verter_session"),
    ];
    const res = await runContainedStep({
      cmd: bin,
      args: binArgs,
      cwd,
      env: suiteEnv, // the runtime Cargo env this suite reads (CARGO_MANIFEST_DIR + CARGO_TARGET_DIR)
      phase: "test", // TEST phase: byte-growth-only liveness
      deadlineMs,
      stallMs,
      targetDir: runnerTarget,
      memoryLimitBytes,
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
    // TELEMETRY (report-only): this suite's own duration + peak RSS are already returned by
    // runContainedStep and were previously discarded; the executed-test count is libtest's own trailing
    // `test result: … N passed; M failed` line (parseLibtestSummary — the SAME parser analyzeLibtestSurface
    // already calls, invoked again here rather than widening that function's return shape).
    const libSummary = parseLibtestSummary(libText);
    const executedTests = libSummary.found ? libSummary.passed + libSummary.failed : 0;
    s2TotalDurationMs += res.durationMs;
    s2TotalTests += executedTests;
    if (res.peakRssBytes > s2PeakRssBytes) {
      s2PeakRssBytes = res.peakRssBytes;
      s2PeakProcessCount = res.memoryProcessCount;
    }
    s2Details.push({
      binaryId: s["binary-id"],
      durationMs: res.durationMs,
      executedTests,
      peakRssBytes: res.peakRssBytes,
      memoryProcessCount: res.memoryProcessCount,
    });
  }
  log(
    `SURFACE 2 done: ${s2Passed} suites clean, ${s2Failed} suites with non-tolerated failures, ${s2Tolerated} tolerated test failures`,
  );
  log(
    `SURFACE 2 TELEMETRY: ${s2TotalTests} executed test(s) across ${s2Details.length} suite(s), total ` +
      `duration ${(s2TotalDurationMs / 1000).toFixed(1)}s (suites run SEQUENTIALLY here, so — unlike ` +
      "surface 1/3's process-isolated per-test sum above — this total IS the surface's wall-clock), peak " +
      `RSS ${formatMemorySize(s2PeakRssBytes)} across ${s2PeakProcessCount} process(es); per-suite:`,
  );
  for (const d of s2Details) {
    log(
      `  ${d.binaryId}: ${(d.durationMs / 1000).toFixed(1)}s, ${d.executedTests} test(s), peak RSS ` +
        `${formatMemorySize(d.peakRssBytes)} (${d.memoryProcessCount} process(es))`,
    );
  }

  // ---------- SURFACE 3: shipped-cfg (debug_assertions OFF) build + run ----------
  // Runs LAST: it needs its own whole-workspace compile, so the two cheap debug surfaces report their
  // (far more common) regressions first. See runShippedCfgSurface for what this surface covers and, just
  // as importantly, what it does not.
  const s3 = await runShippedCfgSurface(opts, ctx, freshnessToleranceAllowed);
  if (s3.exit !== undefined) return s3.exit;
  for (const f of s3.failures) failures.push({ surface: `shipped-cfg/${f.surface}`, name: f.name });
  if (s3.tolerated) toleratedOccurred = true;

  // Always stated at the tail of the run, regardless of verdict, so a reader who only reads the last few
  // lines cannot mistake a green gate for full coverage: this run excluded a named, counted test class from
  // every surface — see the earlier "TRYBUILD EXCLUSION" lines for the per-surface counts and filter.
  log(
    "NOTE: this gate run excluded the trybuild compile-fail harness class (INTERIM, pending maintainer " +
      "disposition — not deleted, not feature-gated, still runnable directly) from all three surfaces; " +
      `see the "TRYBUILD EXCLUSION" lines above for exact counts.`,
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
    // Do NOT re-run the gate on any other branch to decide whether this pre-existed — the working branch's
    // gate is green by invariant, never by comparison. Triage each named failure in isolation instead:
    err(
      "next step: triage each named failure in isolation (never re-run the full gate to compare) — " +
        "node scripts/triage-gate-failure.mjs --log <this captured output>",
    );
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
  log("VERDICT: PASS (all three surfaces green)");
  return EXIT_PASS;
}

main().catch((e) => {
  err(`fatal: ${e && e.stack ? e.stack : e}`);
  process.exit(EXIT_USAGE);
});
