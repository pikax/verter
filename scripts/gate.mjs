#!/usr/bin/env node
// Canonical Rust gate CLI.
//
// SECURITY: this binary runs only the real gate (prerequisites → real harness smokes → archive/list →
// Surface 1 → verdict). The shipped-cfg lane stays implemented but is currently SKIPPED
// (`SHIPPED_CFG_LANE_ENABLED` in gate-internals.mjs). When that flag is true, Surface + shipped-cfg
// lanes run concurrently when the ceiling allows it, serial otherwise.
// `--prepare` is a warm utility,
// never a gate PASS. No test-seam, classifier hook, custom-command
// mode, or env var can make this CLI return the success contract without
// building and running the suite. Internals live in `gate-internals.mjs`;
// the self-test imports them in-process, never via a flag here.
//
// OPERATION-SCOPED EXIT SEMANTICS (read this before trusting an exit 0)
//   `exit 0` means "the requested OPERATION succeeded" — it is scoped to the mode you ran, NOT a blanket
//   gate pass. Concretely:
//     * `node scripts/gate.mjs` and `node scripts/gate.mjs --exhaustive` are THE GATE. Their exit 0 means
//       Surface 1 built AND passed (except the env-only typeinfo freshness PAIR, by exact name, AND only
//       when the freshness-tooling preflight below proves `pnpm` is not resolvable AND `buf` is not
//       resolvable — the condition under which the Rust byte-pin test skips; see FRESHNESS-TOOLING
//       PREFLIGHT). The shipped-cfg guard is currently SKIPPED and is not part of this contract — a PASS
//       is Surface 1 only. That, and only that, is the gate-pass contract.
//     * `--prepare` is a WARM-PASS only. Its exit 0 means PREPARED (the archive built + the first-launch
//       assessment was warmed) — tests were NOT run, so it is NEVER a gate pass. Its success output carries
//       the `PREPARED_NOT_GATE` marker and contains no `PASS` token precisely so a CI `grep PASS` cannot
//       mistake it for a verdict.
//     * `--help` exits 0 after printing this usage — also not a gate pass.
//   `--help` and `--prepare` are both MUTUALLY EXCLUSIVE and argv-strict (a stray flag/positional alongside
//   either is a usage error, exit 127), so neither exit-0 mode can be reached with junk arguments.
//
// PURPOSE
//   Builds the whole workspace test universe ONCE via `cargo nextest archive` (dev profile) and runs it
//   with `cargo nextest run` (per-test PROCESS ISOLATION) — ONE full build, ONE full run, per the
//   maintainer's SINGLE-TEST-UNIVERSE directive
//   (docs/arch/refactor/rev11/rulings/MAINTAINER-DIRECTIVE-SINGLE-TEST-UNIVERSE.md, refining
//   ONE-BUILD-ONE-RUN). That is SURFACE 1. Deliberate shared-process coverage — the class the former
//   Surface 2 existed for — now lives INSIDE that one universe as
//   `verter_session/tests/cases/shared_process_contract.rs`: ordinary `#[test]` functions that perform
//   many operations sequentially (create/use/drop/recreate; multiple hosts alive at once; repeated edits;
//   scheduler shutdown+restart; `OnceLock` lifecycle; failure then recovery) inside the ONE process
//   nextest gives that test — not a second archive, not a second run.
//   After archive/list and every post-list precondition, Surface 1 is the gate verdict. The shipped-cfg
//   lane described below stays implemented but is currently SKIPPED (temporary; flip
//   `SHIPPED_CFG_LANE_ENABLED` to restore it). When restored, Surface 1 overlaps that small serial
//   SHIPPED-CFG GUARD — NOT a second whole-workspace archive/run — covering behaviour that can differ
//   only because `debug_assertions` / `overflow-checks` are off. Every build command issued is a
//   `--workspace` archive build (for surface 1) or, when the lane is enabled, a tiny package-scoped
//   build (for the guard), so the gate NEVER issues the package-scoped `cargo test -p verter_session`
//   resolution and so structurally cannot incur the recompile that resolution caused (see "Canonical
//   feature set" below).
//
// SHIPPED-CFG GUARD — WHAT IT COVERS AND WHAT IT DOES NOT
//   `debug_assert!` does NOT evaluate its argument when `debug_assertions` is off, and
//   `#[cfg(debug_assertions)]` items do not exist there. Every shipped artifact (the LSP binary, napi,
//   wasm) is built that way. So a side effect written inside a `debug_assert!` argument — the real,
//   shipped shape being `debug_assert!(session.commit_completed())`, where the call performs a state
//   transition — executes in every debug test and in NO shipped build. Nothing in Surface 1 sees that (it
//   is a debug build), and `cargo check --workspace --release` compiles the shipped cfg but RUNS NOTHING,
//   so it cannot observe a runtime no-op.
//   Per the SINGLE-TEST-UNIVERSE directive, this is deliberately NOT the former Surface 3 (a second
//   15,454-test whole-workspace archive+run). It is TWO small, targeted mechanisms:
//     (a) `cargo check --workspace --all-targets --profile no-debug-assertions` — a COMPILE-ONLY check.
//         Catches items wrongly hidden behind `cfg(debug_assertions)` and cross-crate APIs that vanish
//         under the shipped configuration, without running anything.
//     (b) `cargo nextest run -p verter_shipped_cfg_contract --cargo-profile no-debug-assertions` — a
//         SMALL, PACKAGE-SCOPED build+run (not `--workspace`) of a dedicated crate
//         (crates/verter_shipped_cfg_contract) whose tests are the ONLY things in this repo that RUN with
//         `debug_assertions` off. Two profile-sanity canaries prove the alternate profile actually took
//         effect (they fail loudly under `dev`); the rest are real `VerterHost` scenarios exercising the
//         production code paths this block's audit found relying on `#[cfg(debug_assertions)]`
//         non-breaking oracle cross-checks.
//   Retained ONLY until semantic dependence on `debug_assertions`/overflow-checks is structurally
//   eliminated (no semantic work inside `debug_assert!`, no semantic production code behind
//   `cfg(debug_assertions)`, explicit `checked_*`/`wrapping_*`/`saturating_*` arithmetic) — once that
//   holds, `no-debug-assertions` becomes the canonical full-test profile and this guard is removed
//   entirely. As of this block: an audit of every `debug_assert!`/`debug_assert_eq!`/`debug_assert_ne!`
//   call in `crates/*/src` found none performing semantic work (134 call-bearing sites, all pure
//   observation — `is_none`/`len`/`matches!`/etc.) and every production `#[cfg(debug_assertions)]` block
//   is a non-breaking diagnostic cross-check whose result never changes the returned value; explicit
//   overflow-safe arithmetic was NOT audited workspace-wide and is out of scope for this block — so the
//   guard stays.
//   NOT COVERED, explicitly: this is not an optimised build. The profile inherits dev codegen (opt-level
//   0, no LTO, many codegen units), so optimisation-, inlining- and LTO-dependent behaviour is out of
//   scope. `verter_shipped_cfg_contract` covers only the code paths its own tests exercise, not the whole
//   workspace — a `debug_assertions`-dependent regression outside that crate's reach is not covered by
//   the guard (though `cargo check --profile no-debug-assertions --all-targets` still compiles it).
//
// SHIPPED-CFG LANE — TEMPORARY SKIP
//   The lane above stays implemented and re-enablable (`SHIPPED_CFG_LANE_ENABLED` in gate-internals.mjs).
//   It is currently SKIPPED: a PASS means Surface 1 passed. Every run discloses the skip in the verdict
//   line and in the summary.
//   TODO: re-enable. Until then the gate does not execute tests with debug_assertions / overflow-checks
//   off, so a state mutation written inside a debug_assert! argument is a silent no-op in every shipped
//   build and is uncovered. cargo check --workspace --release compiles the shipped cfg but runs nothing.
//
// CANONICAL WORKSPACE RESOLUTION (why no `-p verter_session`)
//   Surface 1 consumes the one `cargo nextest archive --workspace` universe. The retired package-scoped
//   `cargo test -p verter_session --tests` command redundantly replayed tests Surface 1 already owned and
//   could force another Cargo resolution/build. Deliberate shared-process contracts now live inside the one
//   archive-backed Surface 1 run, so the production gate never issues that package-scoped blanket replay.
//   It does NOT use `--all-features` (the repo has slow/external feature gates) and does NOT mutate Cargo.toml.
//
// EQUIVALENCE TO THE TWO-COMMAND GATE
//   The legacy gate was: `cargo nextest run --workspace` then `cargo test -p verter_session --tests`. The
//   SINGLE-TEST-UNIVERSE directive retired the second command as a blanket rerun (it executed the SAME
//   archive the first already built and its shared-process rationale — nextest isolates every test in its
//   own process — was real but its blanket-replay implementation was weak: detection depended on
//   incidental ordering, a leak could be masked by another test resetting state, and unrelated tests
//   turned flaky merely by sharing a process). Deliberate shared-process coverage now lives INSIDE surface
//   1's one archive/run as `verter_session/tests/cases/shared_process_contract.rs` — see PURPOSE above.
//
// SAFETY MODEL (pure Node + OS-native tools; ZERO new compiled binaries)
//   1. Runner-owned targets: the archive/list front half uses <runnerTarget>; post-list Surface and shipped
//      lanes use pairwise-disjoint <runnerTarget>/lanes/<lane>/target plus separate gate work/extract/output
//      roots. CARGO_TARGET_DIR is forced per lane (override controls only the runner parent), so no lane
//      shares a Cargo lock or timing source with another and cleanup never targets developer target/debug.
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
//      covers the archive build and surface 1 (the shipped-cfg guard is currently skipped). When the lane
//      is restored it is also absolute across both post-list lanes (concurrent or serial, per
//      deriveGateLaneResourceSplit's `concurrent` flag) and the shipped check→contract transition. On
//      expiry every registered forest is reaped; exit 124.
//   6. Aggregate stall detector with SEPARATE build vs test phases:
//        BUILD phase (the archive build): progress = stdout/stderr byte growth OR runner-owned target-tree
//          artifact growth (file-count + newest-mtime, bounded scan). A long silent rustc is NOT a stall.
//        TEST phase (the Surface and shipped-contract nextest runs): progress = stdout/stderr byte growth
//          ONLY. Target-tree growth is NOT a valid test liveness signal; a silent test binary IS a hang.
//      Any live lane's progress advances one aggregate vector; completed lanes cannot keep a survivor alive.
//      Default stall 12m (--stall). On stall: reap all registered forests + sweep; exit 125.
//   7. Spotlight marker (macOS): a <runnerTarget>/.metadata_never_index file is written so Spotlight does
//      not index the build tree (a harmless no-op file on Linux/Windows).
//   8. Resource ceiling: build jobs and test concurrency are independently finite; omitted build jobs use
//      the measured CPU/memory tier and omitted test threads default to min(host CPUs, 12). Every
//      live process forests are sampled from one OS process-table snapshot once per second and their
//      disjoint RSS is summed against one gate ceiling (default: 50% of physical RAM). A ceiling trip reaps
//      every lane and is the distinct, non-PASS
//      `ABORTED — memory ceiling` outcome (exit 123). Repeated sampler failure also aborts rather than
//      silently running unmonitored. Raw per-lane output (concurrent or serial) is buffered and replayed
//      exactly once in Surface/check/contract order so parseable status rows never interleave.
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
//   the tests that compare Verter's Svelte output against the pinned official `svelte@5.56.10` oracle.
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
// REPORT-ONLY TELEMETRY
//   After mutex acquisition all startup probes share a separate hard aggregate reporting deadline. The
//   canonical build/test deadline begins only after that collection settles, so telemetry spends none of
//   the verdict-bearing timeout. The gate then emits a bounded environment fingerprint, stable phase
//   durations, and the maximum monitored contained-child-tree RSS with the phase/process count from that
//   SAME observation. A partial/aborted run is explicitly `partial`, never `complete`. Final text and schema-v1
//   summaries land together under `gate-work/`; Cargo HTML timings are capability-gated and snapshotted
//   immediately after the dev archive, shipped check, and shipped contract. Missing probes/reports/copies
//   warn and retain the historical argv/verdict. Surface 1 remains archive-backed and gets no `--timings`.
//   Per-test reports count final process identities separately from parseably timed identities; legacy
//   `count` remains the timed-count alias. No telemetry observation is consulted by a verdict branch.
//
// EXECUTION POLICY
//   A bare gate is local fail-fast: nextest stops scheduling after its first failure. A PASS means
//   Surface 1 passed; the shipped-cfg guard is currently SKIPPED and is disclosed on every run. When
//   the lane is restored, a hard Surface-1 receipt cancels a live shipped step or prevents its remaining
//   contract, and the final verdict records that guard as incomplete and is always FAIL.
//   `--exhaustive` changes fail-fast policy only: Surface 1 receives `--no-fail-fast`. When the shipped-cfg
//   lane is restored, the small shipped contract also receives `--no-fail-fast` and both isolated post-list
//   lanes are awaited after ordinary test failures.
//
// USAGE
//   node scripts/gate.mjs [--timeout 80m] [--stall 12m] [--target-dir <DIR>] [--exhaustive]
//                         [--build-jobs N] [--test-threads N] [--memory-limit 12GiB]
//                                                           # THE GATE — exit 0 = Surface 1 built + passed
//                                                           # (shipped-cfg guard currently SKIPPED).
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
//   0   PASS / PASS-WITH-TOLERATED  (the GATE: a real bare or `--exhaustive` run); OR a successful
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
//   CARGO_BUILD_JOBS is SCRUBBED and forced to --build-jobs (default CPU/memory tier: at most 12).
//   (No environment variable can divert this CLI to a non-gate success path.)

import { readdirSync, realpathSync, statSync } from "node:fs";
import { fileURLToPath } from "node:url";
import {
  // exit-code constants (EXIT_STALL is mapped inside mapStepReason, not referenced directly here)
  EXIT_PASS,
  EXIT_FAIL,
  EXIT_LOCK_REFUSED,
  EXIT_USAGE,
  deriveGateLaneLayout,
  buildGateLaneCommandPlan,
  orchestrateGateLanes,
  reduceGateLaneReceipts,
  canonicalGateLaneTranscriptSegments,
  SHIPPED_CFG_LANE_ENABLED,
  SHIPPED_CFG_SKIP_SUMMARY,
  SHIPPED_CFG_SKIP_VERDICT_NOTE,
  // logging + time
  log,
  warn,
  err,
  nowMs,
  parseDuration,
  // --prepare success output (warm-pass marker — never a gate PASS token)
  preparedSuccessLines,
  buildPrepareWarmSpawnEnv,
  classifyPrepareWarmResult,
  // setup
  resolveRepoRoot,
  defaultLockDir,
  buildCargoEnv,
  deriveGateResourceLimits,
  deriveGateLaneResourceSplit,
  parseMemorySize,
  formatMemorySize,
  // mutex + teardown
  Mutex,
  // contained step + analysis
  createGateRunSupervisor,
  mapStepReason,
  analyzeNextestSurface,
  countTestAttributesInDir,
  decideShippedCfgGuardExpectedCountMatch,
  // gate telemetry (report-only; see the "GATE TELEMETRY" section of gate-internals.mjs)
  classifyGateTargetState,
  collectNextestTestTimings,
  createGateTelemetry,
  createGateTelemetryReporter,
  GATE_TELEMETRY_STARTUP_MAX_MS,
  formatGateTelemetryText,
  gatePhaseStatusFromStep,
  recordGateAggregateForestPeak,
  summarizeGateTelemetry,
  summarizeNextestTimings,
  // build-prerequisite preflight (the non-cargo artifacts the suite loads from disk)
  checkBuildPrerequisites,
  probeBudgetMs,
  // oracle-cache prerequisite preflight (the offline Svelte/Vue oracle npm cache the bf2-authoritative
  // conformance suites realize from)
  checkOracleCachePrerequisite,
  oracleCacheProbeBudgetMs,
  // real conformance-harness preflight
  HARNESS_SMOKE_MARKER,
  HARNESS_SMOKE_MODES,
  harnessSmokeCommand,
  decideHarnessSmokeResult,
  formatHarnessSmokeFailure,
  // archive builder — feature parity for the one workspace archive surface 1 builds
  buildNextestArchiveArgs,
  // trybuild exclusion (interim, pending maintainer disposition) — filter builder + coverage guard
  TRYBUILD_EXCLUDED_SUITES,
  buildTrybuildExclusionFilterExpr,
  buildCanonicalSurface1FilterExpr,
  countTrybuildExclusionMatches,
  // freshness-tooling preflight (verdict-gating authority)
  preflightFreshnessTooling,
  pnpmInstallCommand,
  vueMacroOracleGateCommands,
  ensureRequiredWindowsDebugSidecars,
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

// Runs the (synchronous, fs-only) oversize-source scan and returns a print-ready result instead of
// printing directly — callers decide WHEN to surface it. Never throws: a scan failure becomes part of
// the returned result so a caller can warn without the advisory ever being able to abort the gate.
function collectOversizeProductionSourcesSafe(repoRoot) {
  try {
    return { violations: collectOversizeProductionSources(repoRoot) };
  } catch (error) {
    return { scanError: error.message };
  }
}

function printOversizeProductionSourcesResult(result) {
  if (result.scanError) {
    warn(`oversize-source advisory could not scan the production tree: ${result.scanError}`);
    return;
  }
  if (result.violations.length === 0) return;

  const rows = result.violations.map(([rel, lines]) => `${rel} (${lines} lines)`).join("\n  ");
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
function logNextestTimingReport(label, text, allSuites, telemetry = null, telemetryKey = null) {
  const timings = collectNextestTestTimings(text);
  const report = summarizeNextestTimings(timings, allSuites, 50);
  if (telemetry && telemetryKey) telemetry.nextest[telemetryKey] = report;
  log(
    `${label} TIMING: ${report.timedCount}/${report.totalTests} terminal test(s) carried a parseable ` +
      `duration, summing to ${report.totalSec.toFixed(1)}s of reported per-test time (tests run process- ` +
      "isolated and concurrently, so this sum is NOT the surface's wall-clock).",
  );
  log(
    `${label} TIMING: ${report.processCount} final terminal process identity/-ies, ${report.timedCount} timed.`,
  );
  log(`${label} TIMING — cumulative duration by package (${report.perPackage.length} package(s)):`);
  for (const p of report.perPackage) {
    log(
      `  ${p.key}: ${p.count} test(s), ${p.totalSec.toFixed(1)}s; ` +
        `${p.processCount} process(es), ${p.timedCount} timed`,
    );
  }
  log(`${label} TIMING — cumulative duration by binary (${report.perBinary.length} binary/-ies):`);
  for (const b of report.perBinary) {
    log(
      `  ${b.key}: ${b.count} test(s), ${b.totalSec.toFixed(1)}s; ` +
        `${b.processCount} process(es), ${b.timedCount} timed`,
    );
  }
  log(`${label} TIMING — top ${report.topFamilies.length} highest cumulative-time test families:`);
  for (const f of report.topFamilies) {
    log(
      `  ${f.totalSec.toFixed(2)}s (${f.count} test(s)) ${f.key}; ` +
        `${f.processCount} process(es), ${f.timedCount} timed`,
    );
  }
  return report;
}

function recordTelemetryPhaseSafe(ctx, phaseId, observation) {
  if (!ctx?.telemetryReporter) return;
  ctx.telemetryReporter.recordPhase(phaseId, observation);
}

function recordContainedStepTelemetry(ctx, phaseId, result) {
  recordTelemetryPhaseSafe(ctx, phaseId, {
    status: gatePhaseStatusFromStep(result),
    durationMs: result?.durationMs ?? null,
    peakRssBytes: result?.peakRssBytes || 0,
    peakRssProcessCount: result?.peakRssProcessCount || 0,
    detail: result?.cancellationReason || result?.reason || null,
  });
}

function cargoTimingEnabled(ctx, phaseId) {
  return Boolean(ctx.telemetryReporter?.cargoTimingEnabled(phaseId));
}

function beginCargoTimingCapture(ctx, phaseId, sourceTargetDir = ctx.runnerTarget) {
  return ctx.telemetryReporter?.beginCargoTiming(phaseId, sourceTargetDir) || null;
}

function finishCargoTimingCapture(ctx, phaseId, capture) {
  ctx.telemetryReporter?.finishCargoTiming(phaseId, capture);
}

// ----------------------------------------------------------------------------------------------------
// Argument parsing. The production CLI accepts ONLY the real-gate flags + --prepare + --help. There is NO
// `-- <cmd>` custom-command path, NO `--selftest-*` hook, and NO `--internal-selftest-seam` seam: those
// would be modes that can return success without running the gate, which this binary must never expose.
// An unknown argument is a USAGE error (exit 127), never a silent success.
//
// OPERATION-SCOPED EXIT SEMANTICS — the two NON-gate modes (`--help`, `--prepare`) each legitimately exit 0
// on success, while bare and `--exhaustive` gate runs carry the gate-pass contract. To keep
// those two modes from being confusable with a gate pass, BOTH are MUTUALLY EXCLUSIVE and argv-strict:
//   --help / -h : accepts NO other argv token whatsoever. `gate.mjs --help --anything` (a flag OR a
//     positional) is a USAGE error (exit 127) — only a bare `gate.mjs --help` prints usage and exits 0, so
//     a stray flag can never be silently swallowed under the exit-0 help mode.
//   --prepare   : accepts ONLY the companion flags the prepare warm-pass actually uses (--target-dir,
//     --timeout, --stall, --build-jobs, --memory-limit, each with its value); ANY other flag (e.g.
//     --exhaustive / --test-threads — gate-only) or ANY positional token is a USAGE error (exit 127).
//     `gate.mjs --prepare junk` /
//     `--prepare --selftest-x` exit 127, so prepare's exit-0 cannot be reached with junk argv.
// The gate mode (no mode flag) accepts the full real-gate flag set.
// ----------------------------------------------------------------------------------------------------

// Flags --prepare is allowed to combine with (the warm-pass front half — archiveAndList — reads exactly
// these). Each takes a value argument. Gate-only flags (--exhaustive / --test-threads) are NOT here, so
// `--prepare --exhaustive` is a usage error rather than a silently-ignored flag.
const PREPARE_ALLOWED_VALUE_FLAGS = new Set([
  "--target-dir",
  "--timeout",
  "--stall",
  "--build-jobs",
  "--memory-limit",
]);
const GATE_ALLOWED_VALUE_FLAGS = new Set([...PREPARE_ALLOWED_VALUE_FLAGS, "--test-threads"]);
const ARGUMENT_VALUE_ERROR_MARKER = "ARGUMENT VALUE ERROR";

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

function readRequiredOptionValue(argv, valueIndex, flag, operation) {
  const value = argv[valueIndex];
  const operationPrefix = operation === "prepare" ? "--prepare: " : "";
  if (typeof value !== "string" || value.length === 0) {
    throw usageError(
      `${ARGUMENT_VALUE_ERROR_MARKER}: ${operationPrefix}'${flag}' requires a non-empty value`,
    );
  }
  if (value.startsWith("-")) {
    throw usageError(
      `${ARGUMENT_VALUE_ERROR_MARKER}: ${operationPrefix}'${flag}' requires a value; ` +
        `option-looking token '${value}' cannot be consumed as that value`,
    );
  }
  return value;
}

function defaultOptions(mode) {
  const resources = deriveGateResourceLimits();
  return {
    mode,
    timeoutSecs: parseDuration("80m"),
    stallSecs: parseDuration("12m"),
    targetDir: process.env.VERTER_GATE_TARGET_DIR || "",
    exhaustive: false,
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
    let explicitBuildJobs;
    let explicitMemoryLimitBytes;
    let i = 0;
    while (i < argv.length) {
      const a = argv[i];
      if (a === "--prepare") {
        // the mode selector itself; already handled.
      } else if (PREPARE_ALLOWED_VALUE_FLAGS.has(a)) {
        const v = readRequiredOptionValue(argv, i + 1, a, "prepare");
        i++;
        if (a === "--target-dir") opts.targetDir = v;
        else if (a === "--timeout") opts.timeoutSecs = parseDuration(v);
        else if (a === "--stall") opts.stallSecs = parseDuration(v);
        else if (a === "--build-jobs") explicitBuildJobs = parsePositiveInteger(v, a);
        else if (a === "--memory-limit") explicitMemoryLimitBytes = parseMemorySize(v);
      } else {
        throw usageError(
          `--prepare accepts only --target-dir/--timeout/--stall/--build-jobs/--memory-limit ` +
            `(and no positional argument); got ` +
            `'${a}'. --prepare is the warm-pass, NOT the gate — gate-only flags and stray tokens are rejected.`,
        );
      }
      i++;
    }
    Object.assign(
      opts,
      deriveGateResourceLimits({
        buildJobs: explicitBuildJobs,
        memoryLimitBytes: explicitMemoryLimitBytes,
      }),
    );
    return opts;
  }

  // Gate mode — the real-gate flag set. Bare local and explicit exhaustive runs share the gate-pass contract.
  const opts = defaultOptions("gate");
  let explicitBuildJobs;
  let explicitTestThreads;
  let explicitMemoryLimitBytes;
  let i = 0;
  while (i < argv.length) {
    const a = argv[i];
    if (a === "--exhaustive") {
      opts.exhaustive = true;
    } else if (GATE_ALLOWED_VALUE_FLAGS.has(a)) {
      const v = readRequiredOptionValue(argv, i + 1, a, "gate");
      i++;
      if (a === "--target-dir") opts.targetDir = v;
      else if (a === "--timeout") opts.timeoutSecs = parseDuration(v);
      else if (a === "--stall") opts.stallSecs = parseDuration(v);
      else if (a === "--build-jobs") explicitBuildJobs = parsePositiveInteger(v, a);
      else if (a === "--test-threads") explicitTestThreads = parsePositiveInteger(v, a);
      else if (a === "--memory-limit") explicitMemoryLimitBytes = parseMemorySize(v);
    } else {
      throw usageError(
        `unknown argument: '${a}'. This gate accepts only --timeout/--stall/--target-dir/` +
          `--exhaustive/--build-jobs/--test-threads/--memory-limit/--prepare/--help; ` +
          `it has no test-seam or custom-command mode.`,
      );
    }
    i++;
  }
  Object.assign(
    opts,
    deriveGateResourceLimits({
      buildJobs: explicitBuildJobs,
      testThreads: explicitTestThreads,
      memoryLimitBytes: explicitMemoryLimitBytes,
    }),
  );
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

  // Off the critical path: this is a synchronous fs-only scan (no build, no test) with zero dependency
  // on anything the mutex/preflights/archive build produce, so it has no reason to run serially BEFORE
  // them and add its own wall-clock ahead of a multi-hundred-second archive build. Deferred one event-loop
  // turn (setImmediate) so it runs interleaved with — not ahead of — the real work; joined and printed
  // in this function's `finally` below, by which point it has always long since finished. That `finally`
  // is not reached by paths that exit before the try (mkdirSync setup, mutex error, LOCK-REFUSED), and
  // the SIGINT/SIGTERM handlers exit without cancelling main(), so whether the advisory prints on a
  // signalled run is genuinely indeterminate. No invariant is claimed here beyond: at most once.
  const oversizeScanPromise = new Promise((resolve) => {
    setImmediate(() => {
      const startedAtMs = nowMs();
      resolve({
        result: collectOversizeProductionSourcesSafe(repoRealpath),
        durationMs: nowMs() - startedAtMs,
      });
    });
  });

  const runnerTarget = opts.targetDir
    ? isAbsolute(opts.targetDir)
      ? opts.targetDir
      : join(repoRealpath, opts.targetDir)
    : join(repoRealpath, "target", "gate-runner");
  const targetInitialState = classifyGateTargetState(runnerTarget);

  // Gate work dir (archive, list JSON, extract) lives under the runner target dir.
  const gateDir = join(runnerTarget, "gate-work");
  const laneLayout = deriveGateLaneLayout(runnerTarget, gateDir);

  const lockdir =
    process.env.VERTER_GATE_LOCK || process.env.MOM_GATE_LOCK || defaultLockDir(repoRealpath);

  const token = `${process.pid}.${nowMs()}.${Math.floor(Math.random() * 1e9)}`;
  const cargoEnv = buildCargoEnv(process.env, runnerTarget, undefined, opts.buildJobs);
  // When the shipped-cfg lane is enabled, Surface 1 and that lane run CONCURRENTLY once past archive/list
  // ONLY when the ceiling splits on both resource axes (`deriveGateLaneResourceSplit(...).concurrent ===
  // true`; see runGate below); otherwise they run serially, shipped after Surface settles. Sizing both
  // lanes to the SAME opts.buildJobs/opts.testThreads ceiling would request 2x that ceiling from the host
  // for the whole overlap window, so `deriveGateLaneResourceSplit` partitions the ONE ceiling across the
  // two lanes so their COMBINED demand never exceeds it. While the lane is skipped, Surface 1 keeps the
  // full ceiling — there is no second cargo to share with. The front archive/list phase is sequential
  // (no lane overlap yet) and keeps the full `opts.buildJobs` ceiling via `cargoEnv`.
  const laneResourceSplit =
    opts.mode === "gate"
      ? SHIPPED_CFG_LANE_ENABLED
        ? deriveGateLaneResourceSplit({ buildJobs: opts.buildJobs, testThreads: opts.testThreads })
        : {
            surface: { buildJobs: opts.buildJobs, testThreads: opts.testThreads },
            shippedCfg: { buildJobs: opts.buildJobs, testThreads: opts.testThreads },
            concurrent: false,
          }
      : null;
  const surfaceCargoEnv =
    opts.mode === "gate"
      ? buildCargoEnv(
          process.env,
          laneLayout.surface1.targetDir,
          undefined,
          laneResourceSplit.surface.buildJobs,
        )
      : null;
  const shippedCargoEnv =
    opts.mode === "gate"
      ? buildCargoEnv(
          process.env,
          laneLayout.shippedCfg.targetDir,
          undefined,
          laneResourceSplit.shippedCfg.buildJobs,
        )
      : null;
  let telemetry = null;
  let telemetryReporter = null;
  let telemetryFinalized = false;

  const finalizeTelemetry = (finalExitCode) => {
    if (!telemetry || telemetryFinalized) return;
    telemetryFinalized = true;
    let summary;
    try {
      if (supervisor) recordGateAggregateForestPeak(telemetry, supervisor.snapshotTelemetry());
      summary = summarizeGateTelemetry(telemetry, {
        terminalReached: true,
        exitCode: finalExitCode,
      });
      const textSummary = formatGateTelemetryText(summary);
      for (const line of textSummary.split("\n")) log(line);

      // The gate already owns gateDir. Keep one concise text summary and its additive schema-v1 JSON
      // beside each other; write failures are warnings only and cannot replace the existing gate verdict.
      try {
        mkdirSync(gateDir, { recursive: true });
        writeFileSync(join(gateDir, "gate-telemetry-v1.log"), textSummary + "\n", "utf8");
        writeFileSync(
          join(gateDir, "gate-telemetry-v1.json"),
          JSON.stringify(summary, null, 2) + "\n",
          "utf8",
        );
        log(
          "GATE TELEMETRY ARTIFACTS: gate-work/gate-telemetry-v1.log + " +
            "gate-work/gate-telemetry-v1.json",
        );
      } catch {
        warn(
          "GATE TELEMETRY WARNING: schema-v1 text/JSON artifacts could not be written; gate verdict unchanged",
        );
      }
    } catch {
      warn("GATE TELEMETRY WARNING: terminal summary unavailable; gate verdict unchanged");
    }
  };

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
  //   1. Fence new admissions and close/reap EVERY exact process-forest registration through the one
  //      gate-owned supervisor. The close waits for each child and its descendants to be confirmed dead.
  //   2. Release the mutex (token-checked) only after supervisor close settles, so a second gate can never
  //      start while any old registered test process still runs.
  // Did THIS gate acquire the lock? Declared before teardown so the closure reads the live value. The
  // sweep + reap below are gated on it: a gate that REFUSED the lock (another gate holds it) shares the
  // SAME default runner target dir as the holder, so an UNCONDITIONAL provenanceSweep(runnerTarget) would
  // TERM/KILL the HOLDER's cargo/nextest/rustc tree — a non-acquiring gate killing the very build it
  // refused to contend with. ONLY a gate that acquired the lock (and thus ran cargo in its own runner
  // target) may reap/sweep that target; a LOCK-REFUSED / errored-before-acquire gate must touch NO other
  // process. (release() below is always safe: the mutex is token-checked and releases nothing it does not
  // own, so a non-acquiring gate's release is a no-op on the holder's lock.)
  let acquired = false;
  // Created once after the canonical deadline is established, then threaded through every sequential
  // front step and both post-list lanes (concurrent or serial). Signal/finally teardown closes this same
  // authority.
  let supervisor = null;

  // Memoized so EVERY caller awaits the SAME completion. The signal handlers AND the main-flow `finally`
  // both invoke teardown; without memoization the second caller's short-circuit would let it race ahead to
  // `process.exit` while the FIRST caller's async reap/sweep/release was still in flight, cutting off the
  // lock release (an external SIGTERM then leaves the lockdir held). The shared promise makes both paths
  // block on the full teardown before any exit, so the mutex is ALWAYS released before exit.
  let teardownPromise = null;
  const teardown = () => {
    if (teardownPromise) return teardownPromise;
    teardownPromise = (async () => {
      const telemetryStartedAtMs = nowMs();
      let telemetryStatus = "ok";
      // Only an ACQUIRING gate owns this runner target; a non-acquiring gate skips reap+sweep so it can
      // never touch the holder's (or any other) process tree.
      if (acquired && supervisor) {
        try {
          const reap = await supervisor.closeAndReapAll("GATE_TEARDOWN");
          if (reap && reap.reaped && !reap.confirmedDead) {
            telemetryStatus = "failed";
            warn(
              "teardown could not CONFIRM the active step's process tree was reaped within the kill " +
                "budget — releasing the lock anyway to avoid a permanent hang, but the tree's death is " +
                "UNVERIFIED (a descendant may still be live). This is recorded, not claimed clean.",
            );
          }
        } catch {
          telemetryStatus = "failed";
          /* best-effort reap */
        }
      }
      mutex.release();
      if (telemetryReporter) {
        recordTelemetryPhaseSafe({ telemetry, telemetryReporter }, "teardown", {
          status: telemetryStatus,
          startedAtMs: telemetryStartedAtMs,
        });
      }
    })();
    return teardownPromise;
  };
  const installSignalTraps = () => {
    process.on("SIGINT", async () => {
      await teardown();
      finalizeTelemetry(130);
      process.exit(130);
    });
    process.on("SIGTERM", async () => {
      await teardown();
      finalizeTelemetry(143);
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
  // Whole-gate telemetry begins only after the mutex is successfully acquired. Everything below remains
  // report-only: all startup probes share their own hard aggregate deadline, may be unavailable, and no
  // probe result enters a verdict branch or spends the canonical build/test timeout budget.
  const telemetryStartupDeadlineMs = nowMs() + GATE_TELEMETRY_STARTUP_MAX_MS;
  telemetry = createGateTelemetry({ mode: opts.mode });
  telemetry.lanes =
    opts.mode === "gate"
      ? {
          overlapBoundary: "post-list",
          executionPolicy: opts.exhaustive ? "exhaustive" : "local-fail-fast",
          aggregateAuthority: {
            deadline: "whole-gate-absolute",
            stall: "aggregate-live-vector",
            rssCeiling: "one-supervisor-same-snapshot-sum",
          },
          surface1: { ...laneLayout.surface1, ...laneResourceSplit.surface },
          shippedCfg: {
            ...laneLayout.shippedCfg,
            ...laneResourceSplit.shippedCfg,
            coldTarget: true,
            serial: ["check", "contract"],
          },
          replayOrder: ["surface-1", "shipped-check", "shipped-contract"],
          resourceSplit: laneResourceSplit,
        }
      : null;
  telemetryReporter = createGateTelemetryReporter({
    telemetry,
    deadlineMs: telemetryStartupDeadlineMs,
    targetState: targetInitialState,
    resources: {
      buildJobs: opts.buildJobs,
      testThreads: opts.mode === "prepare" ? null : opts.testThreads,
      memoryLimitBytes: opts.memoryLimitBytes,
      profiles: opts.mode === "prepare" ? ["dev"] : ["dev", "no-debug-assertions"],
    },
    env: cargoEnv,
    runnerTarget,
    gateDir,
  });
  telemetryReporter.collectStartup();
  // Establish the canonical deadline only after report-only startup collection settles. Reordering this
  // above collectStartup would let slow/unavailable telemetry shorten the build/test budget and change the
  // gate verdict despite telemetry's report-only contract.
  const deadlineMs = nowMs() + opts.timeoutSecs * 1000;
  log(`mutex acquired (token=${token} lockdir=${lockdir})`);
  log(`runner target dir: ${runnerTarget}`);
  log(
    `resource ceiling: cargo build jobs=${opts.buildJobs}, ` +
      `test threads=${opts.mode === "prepare" ? "n/a (prepare runs no tests)" : opts.testThreads}, ` +
      `active child-tree RSS=${formatMemorySize(opts.memoryLimitBytes)}`,
  );
  if (opts.mode === "gate") {
    if (!SHIPPED_CFG_LANE_ENABLED) {
      log(
        `lane resource partition: Surface 1 only (${SHIPPED_CFG_SKIP_VERDICT_NOTE}) — ` +
          `surface-1 build-jobs=${laneResourceSplit.surface.buildJobs} ` +
          `test-threads=${laneResourceSplit.surface.testThreads}`,
      );
    } else {
      const laneScheduling = laneResourceSplit.concurrent
        ? "the two post-list lanes run concurrently"
        : "the ceiling is too small to run both lanes at once without oversubscribing it — the two " +
          "post-list lanes run SERIALLY instead (shipped-cfg starts only after Surface 1 settles)";
      log(
        `lane resource partition (combined demand bounded to the ceiling above — ${laneScheduling}): ` +
          `surface-1 build-jobs=${laneResourceSplit.surface.buildJobs} ` +
          `test-threads=${laneResourceSplit.surface.testThreads}; ` +
          `shipped-cfg build-jobs=${laneResourceSplit.shippedCfg.buildJobs} ` +
          `test-threads=${laneResourceSplit.shippedCfg.testThreads}`,
      );
    }
    log(`execution policy: ${opts.exhaustive ? "exhaustive" : "local fail-fast"}`);
  }

  const stallMs = opts.stallSecs * 1000;
  supervisor = createGateRunSupervisor({
    deadlineMs,
    stallMs,
    memoryLimitBytes: opts.memoryLimitBytes,
    killGraceMs: mutex.KILL_GRACE_MS,
    ownershipRoots: [runnerTarget],
  });

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
        supervisor,
        telemetry,
        telemetryReporter,
        laneLayout,
        surfaceCargoEnv,
        shippedCargoEnv,
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
        supervisor,
        telemetry,
        telemetryReporter,
        laneLayout,
        surfaceCargoEnv,
        shippedCargoEnv,
        laneResourceSplit,
      });
    }
  } catch (e) {
    err(`gate error: ${e && e.stack ? e.stack : e}`);
    exitCode = EXIT_USAGE;
  } finally {
    // Printed here (not inside runGate/runPrepare) so both modes share one call site. Reached only by
    // paths that get this far: see the note at the scan's construction for what is NOT covered.
    const advisory = await oversizeScanPromise;
    printOversizeProductionSourcesResult(advisory.result);
    recordTelemetryPhaseSafe({ telemetry, telemetryReporter }, "advisory", {
      status: advisory.result.scanError ? "failed" : "ok",
      durationMs: advisory.durationMs,
    });
    await teardown();
    finalizeTelemetry(exitCode);
  }
  process.exit(exitCode);
}

// ----------------------------------------------------------------------------------------------------
// The ONE build variant. There is only one whole-workspace test universe now (SINGLE-TEST-UNIVERSE): the
// dev profile, built by `cargo nextest archive --workspace` and enumerated by `cargo nextest list`.
// Surface 1 runs from it. The shipped-cfg lane (see runShippedCfgLane) does NOT archive the workspace —
// it is a `cargo check` compile-only step plus a tiny package-scoped `cargo nextest run`, neither of which
// goes through this archive/variant machinery at all.
// ----------------------------------------------------------------------------------------------------
const VARIANT_DEBUG = {
  key: "debug",
  cargoProfile: null, // cargo's default (dev)
  archiveName: "nextest.tar.zst",
  extractName: "extract",
  label: "workspace test universe (dev profile)",
};

// TRYBUILD EXCLUSION — interim, pending maintainer disposition (see TRYBUILD_EXCLUDED_SUITES in
// gate-internals.mjs for why). Applied to surface 1's `--workspace` selection.
const TRYBUILD_EXCLUSION_FILTER = buildTrybuildExclusionFilterExpr();

// SHIPPED-CFG CONTRACT EXCLUSION. `verter_shipped_cfg_contract`'s tests are DELIBERATELY meaningless
// under surface 1's dev-profile `--workspace` archive: its two profile-sanity canaries assert
// `debug_assertions` and overflow-checks are OFF, which is true only under the alternate
// `no-debug-assertions` profile the shipped-cfg guard runs it under (`runShippedCfgLane`). Excluded from
// surface 1's selection here so the SAME package still runs — deliberately, under the right profile — a
// few steps later, rather than failing surface 1 for behaving exactly as designed.
const SURFACE_1_FILTER = buildCanonicalSurface1FilterExpr();

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
// Archive + list — the shared front half of the gate and --prepare (surface 1's ONE archive; the
// shipped-cfg guard does not archive at all — see runShippedCfgLane). Returns the parsed list JSON + the
// extract dir, or an `{ error }` on setup/build failure. `variant` selects the Cargo profile and the
// archive/extract paths; there is currently one variant (VARIANT_DEBUG, the default).
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

  // --- BUILD the whole workspace test universe ONCE (ARCHIVE_FEATURES =>
  // verter_session/bf2-authoritative ON, so the 45 oracle-backed conformance tests are
  // PRESENT in the archive surface 1 runs from — see buildNextestArchiveArgs) ---
  log(`archiving ${variant.label} (cargo nextest archive --workspace) …`);
  const cargoTimingCapture = beginCargoTimingCapture(ctx, "dev-archive");
  const archiveRes = await ctx.supervisor.runStep("front", {
    cmd: "cargo",
    args: buildNextestArchiveArgs({
      buildJobs,
      cargoProfile: variant.cargoProfile,
      archiveFile,
      runnerTarget,
      timingsEnabled: cargoTimingEnabled(ctx, "dev-archive"),
    }),
    cwd: repoRealpath,
    env: cargoEnv,
    phase: "build",
    deadlineMs,
    stallMs,
    targetDir: runnerTarget,
    memoryLimitBytes,
  });
  finishCargoTimingCapture(ctx, "dev-archive", cargoTimingCapture);
  recordContainedStepTelemetry(ctx, "dev-archive", archiveRes);
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
  // TELEMETRY (report-only): the archive step's successful peak RSS is measured internally by the
  // watchdog and retained in the phase/whole summary; the archive's on-disk size costs one stat().
  let archiveSizeBytes = 0;
  try {
    archiveSizeBytes = statSync(archiveFile).size;
  } catch {
    /* best-effort */
  }
  log(
    `archive [${variant.key}] TELEMETRY: size ${formatMemorySize(archiveSizeBytes)}, peak RSS ` +
      `${formatMemorySize(archiveRes.peakRssBytes)} across ${archiveRes.peakRssProcessCount} process(es)`,
  );

  // --- LIST the suites from the archive (NO rebuild); JSON to a dedicated stdout capture ---
  log(
    `listing suites from the [${variant.key}] archive (cargo nextest list --message-format json) …`,
  );
  const listRes = await ctx.supervisor.runStep("front", {
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
  recordContainedStepTelemetry(ctx, "dev-list", listRes);
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
  // TELEMETRY (report-only): the list/extract step's successful peak RSS retained in the phase/whole
  // summary + the total size of every suite binary this archive extracted.
  const extractedSizes = computeExtractedBinarySizes(listJson, extractDir);
  log(
    `list [${variant.key}] TELEMETRY: peak RSS ${formatMemorySize(listRes.peakRssBytes)} across ` +
      `${listRes.peakRssProcessCount} process(es); extracted binaries ` +
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
  const rustBuildMeta = listJson["rust-build-meta"];
  const buildMetaTargetDir = rustBuildMeta && rustBuildMeta["target-directory"];
  const suites = Object.values(listJson["rust-suites"] || {});
  // One-shot warm: launch each suite binary with --list (no test execution) so the OS first-launch
  // assessment for that binary is performed now via the legitimate path. STRICT: a successful warm is
  // EXACTLY `status === 0`; libtest's `--list` exits 0 on success, so 0 is the only success code here. A
  // non-zero status, a signal, or a missing/unresolvable binary is a warm FAILURE — reported, never
  // counted as warmed, and it makes the whole prepare a fail-setup.
  let warmed = 0;
  let warmFailures = 0;
  let missing = 0;
  const telemetryStartedAtMs = nowMs();
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
    const warmEnvResult = buildPrepareWarmSpawnEnv({
      suite: s,
      rustBuildMeta,
      baseEnv: ctx.cargoEnv,
    });
    if (!warmEnvResult.ok) {
      warmFailures++;
      warn(`${warmEnvResult.detail} — NOT counted as warmed`);
      continue;
    }
    const warmEnv = warmEnvResult.env;
    const r = spawnSync(bin, ["--list"], {
      encoding: "utf8",
      windowsHide: true,
      timeout: 30000,
      env: warmEnv,
    });
    const warmResult = classifyPrepareWarmResult(r);
    if (warmResult.ok) {
      warmed++;
    } else {
      warmFailures++;
      warn(
        `prepare: warm '--list' of ${s["binary-id"] || bin} did NOT exit 0 (${warmResult.detail}) — NOT counted as ` +
          "warmed (a warm-list failure is reported, never swallowed as success)",
      );
    }
  }
  recordTelemetryPhaseSafe(ctx, "prepare-warm", {
    status: warmFailures > 0 || missing > 0 ? "failed" : "ok",
    startedAtMs: telemetryStartedAtMs,
  });
  if (warmFailures > 0 || missing > 0) {
    // STRICT warm counting: a warm-list failure / missing binary is NEVER swallowed as success — it is a
    // fail-setup (exit 127). This is NOT a gate verdict; it means
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
  for (const [index, invocation] of vueMacroOracleGateCommands(process.execPath).entries()) {
    const phaseId = index === 0 ? "vue-macro-oracle-check" : "vue-macro-oracle-tests";
    log(`Vue macro oracle: ${invocation.name} …`);
    const result = await ctx.supervisor.runStep("front", {
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
    recordContainedStepTelemetry(ctx, phaseId, result);
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
// SHIPPED-CFG GUARD — the SINGLE-TEST-UNIVERSE directive's replacement for the former Surface 3 (a
// 15,454-test second whole-workspace archive+run). Per the directive, this is deliberately NOT another
// archive: it is (a) a compile-only check under the `no-debug-assertions` profile, then (b) a small
// package-scoped nextest run of the dedicated `verter_shipped_cfg_contract` crate — the ONLY tests in
// this repo that execute with `debug_assertions` off.
//
// WHAT ONLY THIS GUARD CAN SEE. `debug_assert!` does not evaluate its argument when `debug_assertions` is
// off. A side effect written inside that argument — `debug_assert!(session.commit_completed())`, where the
// call performs a state transition — runs in every debug test (surface 1) and in NO shipped build. `cargo
// check --workspace --release` compiles the shipped cfg but runs nothing, so it cannot see it either. Only
// executing tests with `debug_assertions` off makes the no-op observable. The compile-check half also
// makes a cross-crate item gated on `debug_assertions` a COMPILE error here rather than a shipped-build
// surprise, because a dependent's code is compiled against the same profile as the dependency.
//
// WHAT IT DOES NOT COVER, stated plainly: it is not an optimised build (dev codegen, no LTO — optimisation-
// and LTO-dependent behaviour is out of scope). `verter_shipped_cfg_contract` is deliberately small
// ("dozens of tests at most") — it covers the production code paths ITS OWN tests exercise, not the whole
// workspace. It is retained only until semantic dependence on `debug_assertions`/overflow-checks is
// structurally eliminated repo-wide (see the top-of-file SHIPPED-CFG GUARD comment for this block's audit
// result); once that holds, `no-debug-assertions` becomes the canonical profile and this guard is removed.
// ----------------------------------------------------------------------------------------------------
function stepOutput(result) {
  return `${result?.stdout || ""}\n${result?.stderr || ""}`;
}

function persistLaneOutput(lane, receipt) {
  try {
    const output =
      receipt.laneId === "surface-1"
        ? receipt.output
        : `${receipt.check?.output || ""}\n${receipt.contract?.output || ""}`;
    writeFileSync(lane.outputFile, output, "utf8");
  } catch (error) {
    receipt.exitCode = EXIT_USAGE;
    receipt.messages.push({
      phaseId: receipt.laneId === "surface-1" ? "surface-1" : "shipped-check",
      level: "error",
      text: `gate setup failed writing ${receipt.laneId} buffered output: ${error?.message || error}`,
    });
  }
  return receipt;
}

async function runSurface1Lane(opts, ctx, { allSuites, commandPlan, freshnessToleranceAllowed }) {
  const { repoRealpath, deadlineMs, stallMs, memoryLimitBytes, laneLayout, surfaceCargoEnv } = ctx;
  const lane = laneLayout.surface1;
  const runRes = await ctx.supervisor.runStep("surface-1", {
    cmd: "cargo",
    args: commandPlan.surface1.args,
    cwd: repoRealpath,
    env: surfaceCargoEnv,
    phase: "test",
    deadlineMs,
    stallMs,
    targetDir: lane.targetDir,
    memoryLimitBytes,
    mirrorOutput: false,
  });
  recordContainedStepTelemetry(ctx, "surface-1", runRes);
  const output = stepOutput(runRes);
  const messages = [];
  if (runRes.reason) {
    messages.push({
      phaseId: "surface-1",
      level: "error",
      text: `nextest run ${runRes.reason} after ${Math.round(runRes.durationMs / 1000)}s`,
    });
    return persistLaneOutput(lane, {
      laneId: "surface-1",
      hardFailure: false,
      exitCode: runRes.reason === "CANCELLED" ? EXIT_USAGE : mapStepReason(runRes),
      failures: [],
      toleratedOccurred: false,
      coverage: { parseable: false, complete: false },
      output,
      result: runRes,
      analysis: null,
      allSuites,
      messages,
    });
  }
  if (runRes.spawnError) {
    messages.push({
      phaseId: "surface-1",
      level: "error",
      text: "could not launch 'cargo' for Surface 1 (command not found / not executable)",
    });
    return persistLaneOutput(lane, {
      laneId: "surface-1",
      hardFailure: false,
      exitCode: EXIT_USAGE,
      failures: [],
      toleratedOccurred: false,
      coverage: { parseable: false, complete: false },
      output,
      result: runRes,
      analysis: null,
      allSuites,
      messages,
    });
  }

  const analysis = analyzeNextestSurface(output, runRes.code, freshnessToleranceAllowed);
  const complete =
    analysis.summary.runCountFound && analysis.summary.count === 1 && analysis.summary.unrun === 0;
  messages.push({
    phaseId: "surface-1",
    level: "log",
    text:
      `SURFACE 1 done in ${Math.round(runRes.durationMs / 1000)}s: ` +
      `${analysis.summary.runCount}${analysis.summary.unrun > 0 ? `/${analysis.summary.initialCount}` : ""} run, ` +
      `${analysis.summary.passed} passed, ${analysis.summary.nonPassed} did not pass ` +
      `(${analysis.summary.failed} failed, ${analysis.summary.timedOut} timed out, ` +
      `${analysis.summary.execFailed} exec failed` +
      `${analysis.summary.unrun > 0 ? `, ${analysis.summary.unrun} NEVER RAN` : ""}) ` +
      `(${analysis.namedCount} named, ${analysis.toleratedCount} tolerated), ` +
      `${analysis.summary.skipped} skipped; run exit ${runRes.code}`,
  });
  messages.push({
    phaseId: "surface-1",
    level: "log",
    text:
      `SURFACE 1 TELEMETRY: peak RSS ${formatMemorySize(runRes.peakRssBytes)} across ` +
      `${runRes.peakRssProcessCount} process(es)`,
  });
  return persistLaneOutput(lane, {
    laneId: "surface-1",
    hardFailure: analysis.failures.length > 0,
    exitCode: null,
    failures: analysis.failures.map((failure) => ({ ...failure })),
    toleratedOccurred: analysis.toleratedCount > 0,
    coverage: { parseable: analysis.summary.runCountFound, complete },
    output,
    result: runRes,
    analysis,
    allSuites,
    messages,
  });
}

async function runShippedCfgLane(opts, ctx, { allSuites, commandPlan }) {
  const { repoRealpath, deadlineMs, stallMs, memoryLimitBytes, laneLayout, shippedCargoEnv } = ctx;
  const lane = laneLayout.shippedCfg;
  const receipt = {
    laneId: "shipped-cfg",
    hardFailure: false,
    exitCode: null,
    failures: [],
    check: { status: "not-run", output: "", result: null },
    contract: {
      status: "not-run",
      parseable: false,
      complete: false,
      output: "",
      result: null,
      analysis: null,
    },
    parity: { complete: false, matches: false, expectedTestCount: null },
    allSuites,
    messages: [],
  };

  const checkTimingCapture = beginCargoTimingCapture(ctx, "shipped-check", lane.targetDir);
  const checkRes = await ctx.supervisor.runStep("shipped-cfg", {
    cmd: "cargo",
    args: commandPlan.shippedCfg.checkArgs,
    cwd: repoRealpath,
    env: shippedCargoEnv,
    phase: "build",
    deadlineMs,
    stallMs,
    targetDir: lane.targetDir,
    memoryLimitBytes,
    mirrorOutput: false,
  });
  finishCargoTimingCapture(ctx, "shipped-check", checkTimingCapture);
  recordContainedStepTelemetry(ctx, "shipped-check", checkRes);
  receipt.check = {
    status: checkRes.reason === "CANCELLED" ? "cancelled" : checkRes.code === 0 ? "ok" : "failed",
    output: stepOutput(checkRes),
    result: checkRes,
  };
  if (checkRes.reason) {
    receipt.messages.push({
      phaseId: "shipped-check",
      level: checkRes.reason === "CANCELLED" ? "warn" : "error",
      text:
        `SHIPPED-CFG GUARD: cargo check ${checkRes.reason} after ` +
        `${Math.round(checkRes.durationMs / 1000)}s` +
        (checkRes.cancellationReason ? ` (${checkRes.cancellationReason})` : ""),
    });
    if (checkRes.reason !== "CANCELLED" || checkRes.cancellationReason !== "SURFACE_1_FAIL_FAST") {
      receipt.exitCode = checkRes.reason === "CANCELLED" ? EXIT_USAGE : mapStepReason(checkRes);
    }
    return persistLaneOutput(lane, receipt);
  }
  if (checkRes.spawnError) {
    receipt.exitCode = EXIT_USAGE;
    receipt.messages.push({
      phaseId: "shipped-check",
      level: "error",
      text: "could not launch 'cargo' for the shipped-cfg compile check (command not found / not executable)",
    });
    return persistLaneOutput(lane, receipt);
  }
  if (checkRes.code !== 0) {
    const name = checkRes.signalName
      ? `cargo check child terminated by signal ${checkRes.signalName}; shipped configuration did not finish compiling`
      : `cargo check --profile no-debug-assertions failed (exit ${checkRes.code}); an item may be hidden behind cfg(debug_assertions) or otherwise fail under shipped configuration`;
    receipt.failures.push({ surface: "check", name });
    receipt.hardFailure = true;
    receipt.messages.push({ phaseId: "shipped-check", level: "error", text: name });
    return persistLaneOutput(lane, receipt);
  }
  receipt.messages.push({
    phaseId: "shipped-check",
    level: "log",
    text: `SHIPPED-CFG GUARD: compile check clean in ${Math.round(checkRes.durationMs / 1000)}s`,
  });

  const contractTimingCapture = beginCargoTimingCapture(ctx, "shipped-contract", lane.targetDir);
  const runRes = await ctx.supervisor.runStep("shipped-cfg", {
    cmd: "cargo",
    args: commandPlan.shippedCfg.contractArgs,
    cwd: repoRealpath,
    env: shippedCargoEnv,
    phase: "build",
    deadlineMs,
    stallMs,
    targetDir: lane.targetDir,
    memoryLimitBytes,
    mirrorOutput: false,
  });
  finishCargoTimingCapture(ctx, "shipped-contract", contractTimingCapture);
  recordContainedStepTelemetry(ctx, "shipped-contract", runRes);
  receipt.contract = {
    status: runRes.reason === "CANCELLED" ? "cancelled" : "failed",
    parseable: false,
    complete: false,
    output: stepOutput(runRes),
    result: runRes,
    analysis: null,
  };
  if (runRes.reason) {
    receipt.messages.push({
      phaseId: "shipped-contract",
      level: runRes.reason === "CANCELLED" ? "warn" : "error",
      text:
        `SHIPPED-CFG GUARD: verter_shipped_cfg_contract nextest run ${runRes.reason} after ` +
        `${Math.round(runRes.durationMs / 1000)}s` +
        (runRes.cancellationReason ? ` (${runRes.cancellationReason})` : ""),
    });
    if (runRes.reason !== "CANCELLED" || runRes.cancellationReason !== "SURFACE_1_FAIL_FAST") {
      receipt.exitCode = runRes.reason === "CANCELLED" ? EXIT_USAGE : mapStepReason(runRes);
    }
    return persistLaneOutput(lane, receipt);
  }
  if (runRes.spawnError) {
    receipt.exitCode = EXIT_USAGE;
    receipt.messages.push({
      phaseId: "shipped-contract",
      level: "error",
      text: "could not launch 'cargo' for the shipped-cfg contract (command not found / not executable)",
    });
    return persistLaneOutput(lane, receipt);
  }

  const guard = analyzeNextestSurface(receipt.contract.output, runRes.code, false);
  const complete =
    guard.summary.runCountFound && guard.summary.count === 1 && guard.summary.unrun === 0;
  receipt.contract = {
    ...receipt.contract,
    status: "ok",
    parseable: guard.summary.runCountFound,
    complete,
    analysis: guard,
  };
  receipt.failures.push(...guard.failures.map((failure) => ({ ...failure })));
  receipt.hardFailure = receipt.failures.length > 0;
  const expectedTestCount = countTestAttributesInDir(
    join(repoRealpath, "crates", "verter_shipped_cfg_contract", "src"),
  );
  const parityFailure = decideShippedCfgGuardExpectedCountMatch(
    guard.summary.initialCount,
    expectedTestCount,
  );
  receipt.parity = {
    complete: !parityFailure,
    matches: !parityFailure,
    expectedTestCount,
    selectedTestCount: guard.summary.initialCount,
  };
  if (parityFailure) {
    receipt.exitCode = parityFailure.exit;
    receipt.messages.push({
      phaseId: "shipped-contract",
      level: "error",
      text: parityFailure.message,
    });
    return persistLaneOutput(lane, receipt);
  }
  receipt.messages.push({
    phaseId: "shipped-contract",
    level: "log",
    text:
      `SHIPPED-CFG GUARD done in ${Math.round(runRes.durationMs / 1000)}s: ` +
      `${guard.summary.runCount}${guard.summary.unrun > 0 ? `/${guard.summary.initialCount}` : ""} run, ` +
      `${guard.summary.passed} passed, ${guard.summary.nonPassed} did not pass ` +
      `(${guard.summary.failed} failed, ${guard.summary.timedOut} timed out, ` +
      `${guard.summary.execFailed} exec failed` +
      `${guard.summary.unrun > 0 ? `, ${guard.summary.unrun} NEVER RAN` : ""}) ` +
      `(${guard.namedCount} named), ${guard.summary.skipped} skipped; run exit ${runRes.code}`,
  });
  receipt.messages.push({
    phaseId: "shipped-contract",
    level: "log",
    text:
      `SHIPPED-CFG GUARD TELEMETRY: peak RSS ${formatMemorySize(runRes.peakRssBytes)} across ` +
      `${runRes.peakRssProcessCount} process(es) (compile check: peak RSS ` +
      `${formatMemorySize(checkRes.peakRssBytes)} across ${checkRes.peakRssProcessCount} process(es))`,
  });
  return persistLaneOutput(lane, receipt);
}

function replayGateLaneTranscript(receipts, ctx, allSuites) {
  const segments = SHIPPED_CFG_LANE_ENABLED
    ? canonicalGateLaneTranscriptSegments(receipts)
    : canonicalGateLaneTranscriptSegments(receipts).filter((segment) => segment.phaseId === "surface-1");
  for (const segment of segments) {
    log(segment.header);
    if (segment.output) {
      process.stderr.write(segment.output.endsWith("\n") ? segment.output : `${segment.output}\n`);
    }
    const owner = segment.phaseId === "surface-1" ? receipts.surface : receipts.shipped;
    for (const message of owner?.messages || []) {
      if (message.phaseId !== segment.phaseId) continue;
      if (message.level === "error") err(message.text);
      else if (message.level === "warn") warn(message.text);
      else log(message.text);
    }
    if (
      segment.phaseId === "shipped-contract" &&
      receipts.shipped?.contract?.status === "not-run"
    ) {
      warn(
        "SHIPPED-CFG GUARD: contract NOT ADMITTED because the compile check did not complete successfully",
      );
    }
    if (segment.phaseId === "surface-1" && receipts.surface?.analysis) {
      logNextestTimingReport(
        "SURFACE 1",
        receipts.surface.output,
        allSuites,
        ctx.telemetry,
        "surface1",
      );
    }
    if (segment.phaseId === "shipped-contract" && receipts.shipped?.contract?.analysis) {
      logNextestTimingReport(
        "SHIPPED-CFG CONTRACT",
        receipts.shipped.contract.output,
        allSuites,
        ctx.telemetry,
        "shippedContract",
      );
    }
  }
}

// Run the harness-owned preflights sequentially so each duration/RSS line is
// independently attributable. Returning false always maps to setup exit 127:
// no Cargo universe has been built, and no gate verdict has been produced.
async function runHarnessSmokeChecks(ctx) {
  const { cargoEnv, repoRealpath, runnerTarget, deadlineMs, stallMs, memoryLimitBytes } = ctx;
  for (const mode of HARNESS_SMOKE_MODES) {
    const command = harnessSmokeCommand(repoRealpath, mode);
    log(`HARNESS-SMOKE [${mode}]: running the canonical conformance harness …`);
    const result = await ctx.supervisor.runStep("front", {
      ...command,
      env: cargoEnv,
      phase: "test",
      deadlineMs,
      stallMs,
      targetDir: runnerTarget,
      captureStdoutSeparately: true,
      memoryLimitBytes,
    });
    recordContainedStepTelemetry(ctx, `harness-smoke-${mode}`, result);
    log(
      `HARNESS-SMOKE [${mode}] TELEMETRY: duration ${((result.durationMs || 0) / 1000).toFixed(
        3,
      )}s, peak RSS ${formatMemorySize(result.peakRssBytes || 0)} across ${
        result.peakRssProcessCount || 0
      } process(es)`,
    );
    const decision = decideHarnessSmokeResult(mode, result);
    if (!decision.ok) {
      err(formatHarnessSmokeFailure(mode, decision));
      return false;
    }
    log(`HARNESS-SMOKE [${mode}]: SATISFIED`);
  }
  return true;
}

// ----------------------------------------------------------------------------------------------------
// runGate: the full canonical gate.
//   0. Verify the non-cargo BUILD PREREQUISITES the suite loads from disk.
//   1. Verify the pinned Vue macro oracle and its extractor.
//   2. archive (build ONCE, dev profile) + list (parse rust-suites).
//   3. POST-LIST LANES — Surface 1 nextest from the archive. When SHIPPED_CFG_LANE_ENABLED is true it
//      overlaps the serial shipped check and contract when the build-jobs/test-threads ceiling can be
//      split across both without oversubscribing it; otherwise the two lanes run serially instead (see
//      deriveGateLaneResourceSplit's `concurrent` flag). Currently that lane is skipped. Surface includes
//      deliberate shared-process coverage (verter_session/tests/cases/shared_process_contract.rs) as
//      ordinary tests in this ONE run — see SINGLE-TEST-UNIVERSE at the top of this file.
//      The shipped lane (when enabled) is a compile-only check under `no-debug-assertions` plus, only
//      after check success, a small package-scoped nextest run of `verter_shipped_cfg_contract`. NOT a
//      second workspace archive.
//   4. Reduce fixed receipt slots; tolerated-only complete Surface 1 coverage => PASS-WITH-TOLERATED.
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
  const prerequisiteStartedAtMs = nowMs();
  const prerequisites = checkBuildPrerequisites({
    repoRoot: repoRealpath,
    timeoutMs: probeBudgetMs(deadlineMs, nowMs()),
  });
  recordTelemetryPhaseSafe(ctx, "build-prerequisite", {
    status: prerequisites.ok ? "ok" : "failed",
    startedAtMs: prerequisiteStartedAtMs,
  });
  if (!prerequisites.ok) {
    for (const line of prerequisites.lines) err(line);
    return EXIT_USAGE;
  }
  log(`build-prerequisite preflight: SATISFIED — ${prerequisites.target} loaded`);

  // ---------- ORACLE-CACHE PREREQUISITE PREFLIGHT (the gate's SECOND step) ----------
  // `verter_session/bf2-authoritative` (now ON for every archive — see `buildNextestArchiveArgs`) gates 45
  // tests, including the ENTIRE `svelte_official_conformance_gate` suite: the tests that actually compare
  // Verter's Svelte output against the pinned official `svelte@5.56.10` oracle. Those tests realize their
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
  const oracleStartedAtMs = nowMs();
  const oraclePrereq = checkOracleCachePrerequisite({
    repoRoot: repoRealpath,
    env: cargoEnv,
    timeoutMs: oracleCacheProbeBudgetMs(deadlineMs, nowMs()),
  });
  recordTelemetryPhaseSafe(ctx, "oracle-cache", {
    status: oraclePrereq.ok ? "ok" : "failed",
    startedAtMs: oracleStartedAtMs,
  });
  if (!oraclePrereq.ok) {
    for (const line of oraclePrereq.lines) err(line);
    return EXIT_USAGE;
  }
  log(
    `oracle-cache prerequisite preflight: SATISFIED — realized ${JSON.stringify(oraclePrereq.realized)}`,
  );

  // ---------- REAL CONFORMANCE-HARNESS SMOKES (the gate's THIRD step) ----------
  // Oracle realization proves the pinned installs are structurally usable. These smokes now exercise the
  // two broader runtime boundaries before Cargo pays to build the Rust universe: the real Vapor DOM/runtime
  // preload and the real workspace-domain TypeScript virtual host. Both run through the same contained-step
  // deadline/stall/RSS machinery as every other external gate phase and require an exact structured receipt.
  const harnessSmokesOk = await runHarnessSmokeChecks(ctx);
  if (!harnessSmokesOk) return EXIT_USAGE;

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
  const freshnessStartedAtMs = nowMs();
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
      return ctx.supervisor.runStep("front", {
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
  recordTelemetryPhaseSafe(ctx, "freshness-tooling", {
    status:
      preflight.action === "watchdog"
        ? "aborted"
        : preflight.action === "setup-fail"
          ? "failed"
          : "ok",
    startedAtMs: freshnessStartedAtMs,
    peakRssBytes: preflight.installRes?.peakRssBytes || 0,
    peakRssProcessCount: preflight.installRes?.peakRssProcessCount || 0,
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

  // This CLI has NO self-test seam and NO ambient-env divert: runGate ALWAYS issues the real archive/list
  // and archive-backed Surface 1. The isolated serialized shipped check/contract lane stays implemented
  // but is currently skipped (`SHIPPED_CFG_LANE_ENABLED`). Reusable seams live in gate-internals.mjs and
  // are driven ONLY by selftests in-process — never reachable from this CLI.
  const out = await archiveAndList(ctx);
  if (out.error) {
    err(`gate setup failed at the ${out.where} step`);
    return out.error;
  }
  const { listJson, extractDir, archiveFile } = out;
  const buildMetaTargetDir =
    listJson["rust-build-meta"] && listJson["rust-build-meta"]["target-directory"];
  const allSuites = Object.values(listJson["rust-suites"] || {});
  for (const path of [
    ctx.laneLayout.surface1.targetDir,
    ctx.laneLayout.surface1.workDir,
    ctx.laneLayout.surface1.extractDir,
    dirname(ctx.laneLayout.surface1.outputFile),
    ctx.laneLayout.shippedCfg.targetDir,
    ctx.laneLayout.shippedCfg.workDir,
    dirname(ctx.laneLayout.shippedCfg.outputFile),
  ]) {
    mkdirSync(path, { recursive: true });
  }
  const sidecars = ensureRequiredWindowsDebugSidecars({
    allSuites,
    runnerTarget: ctx.runnerTarget,
    extractDir,
    destinationExtractDir: ctx.laneLayout.surface1.extractDir,
  });
  if (sidecars.error) {
    err(`Windows archive debug-sidecar setup failed: ${sidecars.error}`);
    return EXIT_USAGE;
  }
  if (sidecars.copied > 0) {
    log(
      `restored ${sidecars.copied} runtime-required Windows PDB sidecar(s) beside Surface 1 extracted tests`,
    );
  }
  log(
    `archive lists ${allSuites.length} suites; build-meta target-directory=${buildMetaTargetDir || "?"}`,
  );

  const trybuildCov1 = verifyTrybuildExclusionCoverage(allSuites, "SURFACE 1 (dev archive)");
  if (trybuildCov1.error) {
    err(trybuildCov1.error);
    return EXIT_USAGE;
  }

  const commandPlan = buildGateLaneCommandPlan({
    archiveFile,
    surfaceExtractDir: ctx.laneLayout.surface1.extractDir,
    repoRealpath,
    filterExpr: SURFACE_1_FILTER,
    exhaustive: opts.exhaustive,
    testThreads: ctx.laneResourceSplit.surface.testThreads,
    shippedTestThreads: ctx.laneResourceSplit.shippedCfg.testThreads,
    shippedCheckTimingsEnabled: cargoTimingEnabled(ctx, "shipped-check"),
    shippedContractTimingsEnabled: cargoTimingEnabled(ctx, "shipped-contract"),
  });

  // ---------- POST-LIST LANES ----------
  // When SHIPPED_CFG_LANE_ENABLED is true and ctx.laneResourceSplit.concurrent is true, both promises
  // are admitted together before either receipt is observed. When the ceiling is too small to split
  // (either axis below 2), orchestrateGateLanes runs them serially instead — see its `concurrent` option.
  // Shipped remains internally serial in either case (check -> contract), while Surface consumes the
  // immutable front archive through its own extract root. While the lane is skipped, only Surface 1 runs.
  const runSurfaceLane = () =>
    runSurface1Lane(opts, ctx, {
      allSuites,
      commandPlan,
      freshnessToleranceAllowed,
    });
  let receipts;
  if (SHIPPED_CFG_LANE_ENABLED) {
    receipts = await orchestrateGateLanes({
      exhaustive: opts.exhaustive,
      concurrent: ctx.laneResourceSplit.concurrent,
      runSurfaceLane,
      runShippedLane: () => runShippedCfgLane(opts, ctx, { allSuites, commandPlan }),
      cancelLane: (laneId, reason) => ctx.supervisor.cancelLane(laneId, reason),
    });
  } else {
    log(SHIPPED_CFG_SKIP_SUMMARY);
    recordTelemetryPhaseSafe(ctx, "shipped-check", {
      status: "skipped",
      durationMs: 0,
      detail: "SHIPPED_CFG_LANE_ENABLED=false",
    });
    recordTelemetryPhaseSafe(ctx, "shipped-contract", {
      status: "skipped",
      durationMs: 0,
      detail: "SHIPPED_CFG_LANE_ENABLED=false",
    });
    receipts = { surface: await runSurfaceLane(), shipped: null };
  }
  replayGateLaneTranscript(receipts, ctx, allSuites);

  // Always stated at the tail of the run, regardless of verdict, so a reader who only reads the last few
  // lines cannot mistake a green gate for full coverage: this run excluded a named, counted test class from
  // surface 1 — see the earlier "TRYBUILD EXCLUSION" lines for the exact count and filter.
  log(
    "NOTE: this gate run excluded the trybuild compile-fail harness class (INTERIM, pending maintainer " +
      "disposition — not deleted, not feature-gated, still runnable directly) from surface 1; " +
      `see the "TRYBUILD EXCLUSION" lines above for exact counts.`,
  );
  if (!SHIPPED_CFG_LANE_ENABLED) {
    log(SHIPPED_CFG_SKIP_SUMMARY);
  }

  // ---------- Aggregate verdict ----------
  receipts.shippedCfgLaneEnabled = SHIPPED_CFG_LANE_ENABLED;
  const decision = reduceGateLaneReceipts(receipts);
  const skipVerdictSuffix = SHIPPED_CFG_LANE_ENABLED ? "" : `; ${SHIPPED_CFG_SKIP_VERDICT_NOTE}`;
  if (decision.exitCode !== null) return decision.exitCode;
  if (decision.verdict === "FAIL") {
    err(
      `VERDICT: FAIL — ${decision.failures.length} non-tolerated failure(s)` +
        (SHIPPED_CFG_LANE_ENABLED ? "" : ` (${SHIPPED_CFG_SKIP_VERDICT_NOTE})`) +
        ":",
    );
    for (const f of decision.failures.slice(0, 50)) err(`  [${f.surface}] ${f.name}`);
    // Do NOT re-run the gate on any other branch to decide whether this pre-existed — the working branch's
    // gate is green by invariant, never by comparison. Triage each named failure in isolation instead:
    err(
      "next step: triage each named failure in isolation (never re-run the full gate to compare) — " +
        "node scripts/triage-gate-failure.mjs --log <this captured output>",
    );
    return EXIT_FAIL;
  }
  if (decision.verdict === "PASS-WITH-TOLERATED") {
    log(
      "VERDICT: PASS-WITH-TOLERATED (only the env-only typeinfo_proto_ts_freshness pair produced an actual " +
        "FAIL line, by exact name, AND the freshness-tooling preflight proved pnpm is not resolvable AND buf " +
        "is not resolvable, so the pair is tolerated. This is the LATENT-net path: the normal buf-less runner " +
        "SKIPS the Rust byte-pin (no FAIL line) and reaches the ordinary PASS below — this branch fires only " +
        "when the pair somehow FAILED despite buf being absent. When the tools are present/installed this pair " +
        "is a HARD failure" +
        `${skipVerdictSuffix})`,
    );
    return EXIT_PASS;
  }
  log(
    SHIPPED_CFG_LANE_ENABLED
      ? "VERDICT: PASS (surface 1 + the shipped-cfg guard both green)"
      : `VERDICT: PASS (surface 1 green; ${SHIPPED_CFG_SKIP_VERDICT_NOTE})`,
  );
  return EXIT_PASS;
}

main().catch((e) => {
  err(`fatal: ${e && e.stack ? e.stack : e}`);
  process.exit(EXIT_USAGE);
});
