---
name: testing
description: "Testing patterns, TDD workflow, TypeScript and Rust test conventions, sourcemap testing, and test execution hygiene for Verter"
---

# Testing Patterns & Conventions

For VS Code extension E2E fixtures, helpers API, and warm-session rules, see `/e2e-vscode-testing`.

## Server Cleanup

Always kill dev/preview servers or other long-running test processes when done — stale servers interfere with subsequent runs (e.g., Playwright's `reuseExistingServer: true` uses old builds).

```bash
# After finishing with a server, kill it
# If started in background, use the process ID or port:
kill $(lsof -t -i:4173)   # Unix
taskkill //F //PID <pid>   # Windows

# Or if using pnpm/npm scripts, Ctrl+C the process
```

## Test Output Best Practices

Redirect output to a temp file, then grep — avoids re-running expensive builds:

```bash
# Good: capture once, search multiple times
pnpm exec playwright test --project=preview 2>&1 | tee /tmp/e2e-output.log
# Then search as needed:
grep -i "fail\|error" /tmp/e2e-output.log

# Bad: re-running the full test suite each time you need different output
pnpm exec playwright test --project=preview 2>&1 | grep "fail"
pnpm exec playwright test --project=preview 2>&1 | grep "error"  # wasteful re-run
```

## TypeScript Test Patterns

**Test locations**: Unit tests co-located as `*.spec.ts` next to source. Type tests in `packages/types/` use `vitest --typecheck`.

**AI-generated tests**: Add comments indicating AI assistance:

```typescript
// For new test files, add a JSDoc at the top:
/**
 * @ai-generated - This test file was generated with AI assistance.
 * Brief description of what the tests cover.
 */

// For individual tests in existing files:
// @ai-generated - Tests X functionality with Y scenarios
it("does something", () => {
  /* ... */
});
```

**Sourcemap testing** (see `macros.map.spec.ts`):

```typescript
const { s, source, result } = processMacrosForSourcemap(code);
const map = s.generateMap({ source: "test.vue" });
```

**Type testing best practices** (`packages/types/`):

- Use a positive assertion for the changed inference contract. Add a `@ts-expect-error` negative assertion when it discriminates a plausible widening to `any`/`unknown`/`never` or another public type-boundary regression that existing coverage does not already catch.

```typescript
it("type is correctly inferred", () => {
  type Result = SomeTypeHelper<Input>;

  // Positive assertion - type matches expected
  assertType<Result>({} as ExpectedType);
  assertType<ExpectedType>({} as Result);

  // @ts-expect-error - Result is not any/unknown/never
  assertType<{ unrelated: true }>({} as Result);
});
```

## Rust Test Patterns

### Test File Organization

When a Rust source file's inline `#[cfg(test)] mod tests` block exceeds ~400 lines, extract tests to a separate sibling file. Two patterns:

**For standalone files** (e.g., `analysis.rs`):

```rust
// In analysis.rs — replace the inline #[cfg(test)] mod tests { ... } block:
#[cfg(test)]
#[path = "analysis_tests.rs"]
mod analysis_tests;
```

**For `mod.rs` files** (e.g., `ide/template/mod.rs`):

```rust
// In mod.rs — loads tests.rs from the same directory:
#[cfg(test)]
mod tests;
```

Extracted file contains module contents directly — `use super::*;`, helpers, and `#[test]` fns. No wrapping `mod tests { }` block.

### TDD Workflow

Behavioral code changes use TDD. Documentation, generated projections, formatting, and mechanical metadata changes use their owning freshness/validation evidence unless they also change executable behavior.

1. Name the plausible regression or contract boundary and confirm existing coverage does not already discriminate it
2. Write or extend the smallest test and observe it fail before the production change
3. Implement minimum code to pass
4. Run relevant tests, verify pass
5. Refactor while keeping tests green

### Durable test vocabulary

Test file/module/test names, comments, fixtures, snapshots, assertion messages, and guard diagnostics describe the lasting behavior or regression boundary. They never name an architecture program/revision, roadmap/DAG, node/block/train identifier, plan phase/stage, implementation sequence, cutover stage, or deletion history. A test for work coordinated by `CCA1`, for example, names the capability or failing behavior and contains no `CCA1` reference.

A comment may supplement that durable explanation with a GitHub issue only when the issue records a specific independently reported product defect and is outside the DAG-controlled `[[github_issue]]` mappings. Never cite a DAG-managed issue, PR, node, charter, or ledger row in code or tests; the DAG coordinates delivery and is not the defect contract.

### Test Economy

Tests are evidence, not a quota. At preflight, map each changed contract to the smallest sufficient proof before proposing new tests:

- Prefer an existing test, then extending or table-driving one existing test, before creating a new test or file.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated by the current suite.
- Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid evidence when appropriate; record a terse rationale.
- Do not add prose or formatting assertions unless those exact bytes are a public contract. Do not add implementation mirrors, duplicate permutations, or tests that merely restate the implementation.
- Negative and mutation tests are reserved for plausible critical fail-closed/correctness boundaries or reproduced defects. They are not universal companions to positive tests.
- Incremental, cancellation, stale-publication, counter, allocation, soak, and performance evidence applies only when the change touches the corresponding authority or hot path. Otherwise record it as not applicable with a terse boundary-based rationale.
- New features and bug fixes still require adequate regression evidence. Refactors must keep applicable existing coverage green; they do not earn new tests merely by changing structure.

### Pinned Vue Macro Runtime Oracle

The official Vue macro baseline is generated only from the repository-pinned
local `@vue/compiler-sfc@3.6.0-rc.5`; tests must not fetch compiler output from the
network. `scripts/vue-macro-runtime-oracle/oracle-lib.mjs` parses compiled
JavaScript and compares normalized runtime facts instead of carrier formatting.
Schema v2 records compiler/profile provenance, constructor order, `skipCheck`,
literal-safe defaults/`defaultKind`, and `typePresent` so an omitted type cannot
collapse with explicit `type: null`. Profile fixtures cover development,
production, and production custom-element output. A
`verter-complete-extension` row records the official
Unknown baseline and may be refined only by a canonical `Complete` Verter
result.

Regenerate and verify with:

```bash
node scripts/gen-vue-macro-runtime-oracle.mjs
node scripts/gen-vue-macro-runtime-oracle.mjs --check
node --test scripts/vue-macro-runtime-oracle/oracle.test.mjs
```

Never hand-edit
`crates/verter_session/tests/fixtures/vue_macro_runtime_oracle.json`; the
generator owns it. The canonical gate runs both drift verification and the
oracle unit tests.

### End-of-change Checks

Outside the orchestration landing-train lifecycle — a local change NOT driven as a train — run the canonical Rust pair after the change, per the repo End-of-change Checks. The full workspace suite is the canonical completeness gate.

Any change driven THROUGH the landing-train lifecycle — including a single substantial train (even a one-slice train) — uses the tiered gating: during slice implementation and fix cycles, targeted runs (changed tests + affected crates + a conservative reverse-dependency closure) are ITERATION EVIDENCE ONLY, and a selector that cannot prove the affected closure MUST fall back to full-workspace coverage for that run (still iteration evidence, never landing evidence); the canonical pair runs at exactly the two lifecycle points — after the final content change on the rebased, landing-frozen train tree, and again independently at post-land confirm. Targeted success is never landing evidence, and the standalone clause above never lets a train-driven change skip the frozen-tree final gate.

The canonical gate's omitted resource defaults are independently measured.
Test threads are `min(12, available CPUs)`. Build jobs are additionally clamped
by the effective memory ceiling: 12 jobs from 16 GiB, 8 jobs from 12 GiB, and 4
jobs below that, always capped by available CPUs. This keeps the documented
24-GiB host at 8 build jobs under its unchanged 12-GiB default ceiling; the
measured 12-job peak was 11.60 GiB and is too close to call portable there.
Explicit positive overrides remain exact. Raising global test
concurrency does not widen `.config/nextest.toml`'s `shared-provider-live` or
`lsp-server-unit` groups: both remain `max-threads = 1` under the default and CI
profiles, with their existing selectors. The cargo-free gate self-test pins
both group assignments as well as the default/override matrix.

On Windows, `--prepare` still warm-lists every archived suite and accepts only
an exact status 0. Proc-macro suites are real test suites, not filterable
compiler artifacts; their direct first launch prepends the suite's listed
`rust-build-meta.platforms[build-platform].libdir` to the already-sanitized
child PATH so the Rust host DLLs resolve. Missing, unavailable, or non-absolute
libdir metadata fails setup; no suite is skipped or tolerated.

The canonical full verification pass:

1. `node scripts/gate.mjs` — CANONICAL local Rust gate. It builds/lists the SINGLE TEST UNIVERSE once, settles every post-list precondition, then runs archive-backed Surface 1. The shipped-cfg lane is currently SKIPPED (temporary; `SHIPPED_CFG_LANE_ENABLED` in `scripts/gate-internals.mjs`) — a PASS is Surface 1 only and is disclosed on every run. The lane stays implemented in pairwise-disjoint runner-owned target/work/extract roots; flip the flag to restore concurrent/serial overlap, local fail-fast cancellation of a live shipped step, and a PASS that also requires successful shipped check, complete contract analysis, and expected-count parity. Use `node scripts/gate.mjs --exhaustive` for CI, release, complete failure diagnostics, and comparable benchmarks: Surface 1 receives `--no-fail-fast` (the skipped lane is skipped in this mode too). Surface 1 remains the one archive-backed workspace run with per-test process isolation, including `verter_session/tests/cases/shared_process_contract.rs`. The shipped lane remains the exact `cargo check --workspace --all-targets --profile no-debug-assertions` followed only on success by package-scoped `cargo nextest run -p verter_shipped_cfg_contract --cargo-profile no-debug-assertions`, with its independent expected-test-count check; it is never a second whole-workspace archive. One supervisor owns the absolute deadline, aggregate stall vector, same-snapshot aggregate RSS ceiling, cancellation, and teardown for both lanes. Raw output is buffered and replayed once in Surface/check/contract order. The freshness-tooling preflight and its verdict-gated `cases::typeinfo_proto_ts_freshness::*` tolerance are unchanged: present/installed buf+oxfmt makes freshness failures hard, neither pnpm nor buf resolving makes the Rust pair skip, oxfmt absence alone never grants tolerance, and deterministic install failure is a loud setup failure. Run with `node_modules` present. See `docs/contributing/gate-performance.md`.
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo check --workspace --release` — the only thing in the loop that compiles the REAL release profile (opt-level 3 + fat LTO); surface 1 is debug, and the shipped-cfg lane (cheap `no-debug-assertions` profile) is currently skipped. `debug_assert!` gates on `cfg!`, a RUNTIME constant, so its body still name-resolves in release: a `#[cfg(debug_assertions)]` helper called inside one is an E0425 in every release build (napi and wasm artifacts included) while compiling clean in debug. It is a CHECK and RUNS NO TESTS, so it cannot observe the runtime half of that class — a state mutation written inside a `debug_assert!` argument compiles fine and silently never executes in a shipped build. `verter_shipped_cfg_contract` under `no-debug-assertions` is what covers that half — the ONLY tests in the repo that execute with `debug_assertions` off — and the gate currently skips that lane. Mirrored in CI by the `rust-build-configs` job.
4. `cargo clippy --target wasm32-unknown-unknown -p verter_wasm -- -D warnings` — host clippy cannot see target-gated code, and the `wasm32-wasip1`/`wasip2` clippy jobs cover the SEPARATE `extensions/lapce` + `extensions/zed` manifests, not this one. Same `rust-build-configs` job in CI.
5. `cargo fmt --all --check`
6. `pnpm test` for TypeScript changes

Confirm `cargo clippy --version` reports the `rust-toolchain.toml`-pinned version before trusting
2–4. Clippy output from a different toolchain is not evidence about the one CI runs.

The gate runner also emits an advisory warning for each non-exempt production Rust source above 1,500
lines, formatted as `path (N lines)`. This scan is informational only: its findings do not enter either
surface analyzer, the failure accumulator, or the final gate verdict.

**Gate telemetry is report-only.** From immediately after mutex acquisition through advisory and teardown,
the runner records stable lane-local phase durations/peaks plus the supervisor's highest same-snapshot
aggregate live-forest RSS, total process count, and per-lane contributions. It emits a bounded host/tool fingerprint and
paired `gate-work/gate-telemetry-v1.{log,json}` artifacts. `complete` means every applicable phase was
measured and terminal handling ran; partial or watchdog-aborted runs stay `partial`, while a fully executed
red test run may still be measurement-complete. Bounded version/help probes share a separate hard aggregate
startup-reporting deadline and hard-terminate their direct child; the canonical build/test deadline starts
only after startup collection settles. Failed reporting warns and makes telemetry partial.
Cargo timing snapshots require either proven pre-launch absence or a changed pre/post content identity when
exact-file deletion fails. These paths never select tests, add a retry/run, alter the failure accumulator,
or affect exit status.

A local fail-fast cancellation marks a live shipped phase `aborted` and any unadmitted remainder `not-run`,
so telemetry is `partial`; the coverage-aware receipt reducer independently emits FAIL and never consults
telemetry. Exhaustive benchmark runs
must pass `--exhaustive` explicitly or their wall time is deliberately truncated and not comparable.

Cargo stable HTML timings are capability-gated on the dev archive, shipped compile check, and shipped
contract only, then copied immediately from the producing target's overwrite source to three distinct
`gate-work/cargo-timings/` files. Archive-backed Surface 1 gets no Cargo timings flag. Nextest reports count
final process identities separately from parseable timing identities at total/package(crate)/binary/family
levels; legacy `count` remains the timed-count alias. See `docs/contributing/gate-performance.md` for phase IDs,
fingerprint fields, schema, and artifact names.

**Build-prerequisite preflight — fail-closed, and the FIRST step of gate mode.** Parts of the Rust suite
load artifacts cargo does not build. The real-provider suites spawn the pinned tsserver with
`--globalPlugins @verter/typescript-plugin --pluginProbeLocations packages/vue-vscode/node_modules`; that
probe dir is a pnpm symlink to `packages/typescript-plugin`, whose `main` is `dist/index.js` — a `tsc -b`
OUTPUT that `pnpm install` does NOT produce. With the symlink present and the dist absent, tsserver loads
no plugin, cannot resolve `.vue`/`.svelte` carriers, and ~64 `*_tsserver` tests fail with `TS2307: Cannot
find module './Comp.vue' or its corresponding type declarations.` — sixty-four opaque failures that read
exactly like a compiler regression. So before the freshness preflight, before cargo, and before any test,
the gate **loads** that plugin entry in a child process and, on any load failure, exits 127 with the marker
`BUILD-PREREQUISITE MISSING`, naming the probe target, the load error, the producing packages, and the
producer command.

- **The oracle is a real load, not a file list.** `require()` of the probe directory — the same resolution
  tsserver performs — in a child process (`runBuildPrerequisiteLoadProbe`), fail-closed on every non-zero
  shape (structured load error, spawn error, signal, timeout, unparseable output). A stat list would be a
  mirror of the emit graph and would drift: the plugin entry eagerly requires its emitted helpers
  (`dist/helpers/carrierStore.js` among them) and `@verter/language-shared`'s entry re-exports a dozen
  emitted siblings, so a tree with **both `index.js` present and one helper missing** satisfies every stat
  and still throws inside tsserver. The load also covers the half-built case the plugin's tsconfig allows:
  `noEmitOnError` is off, so `tsc -b` on the plugin alone emits a dist while failing with `TS2307: Cannot
  find module '@verter/language-shared'`.
- **The probe runs under tsserver's environment, not the gate's.** `TsserverTypeProvider::spawn` strips
  `CHILD_PROCESS_ENV_DENYLIST` (`crates/verter_type_runtime/src/tsserver/ipc.rs` — `NODE_OPTIONS`,
  `VSCODE_INSPECTOR_OPTIONS`, `ELECTRON_RUN_AS_NODE`) before launching node, so an inheriting probe would
  have strictly more influence than the process it speaks for. That gap is exploitable, not theoretical:
  measured, `NODE_OPTIONS=--require=<preload>` with a preload patching `Module._load` to return a dummy for
  `process.argv[1]` made the probe exit 0 and report loaded while tsserver still failed on a missing
  helper. The probe reads that denylist **out of the Rust call site** (`parseTsserverEnvDenylist`) rather
  than restating it, so the two cannot drift; a generated mirror was rejected because its freshness test
  lives in the Rust suite the probe runs *before*. If the const cannot be found or parsed the probe fails
  closed as `environment-unknown`. It strips exactly that denylist and nothing more — equivalence, not
  maximal hardening: a var tsserver also inherits influences the real load identically.
- **The timeout is a HARD bound, sized by the gate's deadline.** `spawnSync`'s default `killSignal` is
  SIGTERM, which a child can trap — measured, a child trapping SIGTERM with an open handle left the parent
  blocked for its full 25s lifetime and then returned status 0, i.e. a hang *and* a false positive. The
  probe kills with `SIGKILL` (no graceful phase: the child's whole job is one `require()`, so there is
  nothing to flush — unlike `runContainedStep`, which escalates because it reaps whole build trees), and its
  budget is `probeBudgetMs(deadline, now)` — the smaller of a 60s cap and the gate's own remaining
  wallclock, so it cannot outlive the `--timeout` it sits inside while holding the single-flight mutex.
  Honest limit: this kills the direct child only; a module that spawns a detached grandchild on require
  would leak it, the same limitation the contained-step runner carries.
- **Failure classes are typed, not string-matched.** The probe returns a `reason`
  (`module-not-found` / `load-error` / `timeout` / `spawn-error` / `signalled` / `unknown-exit` /
  `environment-unknown`). Only `module-not-found` means "this tree was never built"; every other class means
  the probe could not *answer*. Callers that gate behavior on the prerequisite must branch on `reason` —
  see `(xix)` below, where skipping on any failure would green-skip a scenario whose artifacts are present.
- **What it does not prove: freshness.** A dist that loads but was emitted from an older commit passes.
  That is a separate problem and is deliberately out of scope — not an oversight.
- **Two packages produce the closure**, and they are the producer command's scope: the plugin, plus
  `@verter/language-shared`. `@verter/native` is deliberately excluded — the plugin's
  `"files": ["src/index.ts"]` leaves out `src/tsc/`, its only consumer, and no Rust test loads a `.node`.
- **Produce them** with `pnpm --filter @verter/language-shared --filter @verter/typescript-plugin build`
  (pnpm orders multi-filter recursive scripts topologically). NOT `pnpm build` (native + LSP + wasm + every
  TS package) and NOT `--filter @verter/typescript-plugin...` — the trailing ellipsis selects the package
  AND ITS DEPENDENCIES, dragging in `@verter/native`'s `napi build --release`. This is also the step
  `ci.yml`'s `rust-test` and `release.yml`'s `test` job run before the gate.
- **It never builds for you and never skips.** Building implicitly would make the verdict depend on a
  mutation the gate performed; skipping would restore the silent pass — with no install at all the affected
  tests SKIP and the gate goes green having proven nothing, the "unexpected prerequisite skips" half of
  Verification Must Prove Execution. `--prepare` is exempt: it builds the archive and runs no test.
- **Discrimination** is proven by `(GB9)` in `scripts/gate-selftest.mjs`, which drives the REAL production
  CLI (a byte-copy rooted in a synthetic git repo holding a miniature of the package graph, so the gate
  keeps its zero test seams and no developer tree is mutated) in six directions: nothing built / plugin
  entry missing / language-shared missing / **a transitively-required helper missing while both entries are
  present** ⇒ 127 before the freshness preflight and before cargo, with the refusal reporting
  `MODULE_NOT_FOUND` rather than a probe that could not answer; everything built ⇒ SATISFIED and the run
  proceeds. Every plant is stat-proven applied and re-stated after the run. Three properties are exercised
  with real subprocesses rather than injected result shapes, because they are the ways a bad tree could
  still pass: `(GB9.1b)` runs a **real SIGTERM-trapping child** and asserts the bound holds, the reason is
  `timeout`, and no process survives; `(GB9.1c)` runs a **real forged `NODE_OPTIONS` preload** and asserts
  it cannot fake a load, with a helper-present control proving the env was sanitized rather than broken, and
  a launcher-deleted leg proving `environment-unknown` fails closed; `(GB9.1d)`/`(GB9.1e)` pin the denylist
  parser's fail-closed shapes and the deadline clamp.
- **`(xix)` declares its precondition, narrowly.** That scenario drives the real gate against the real repo
  expecting it to reach cargo, so on a tree without the prerequisites it would hit this 127 and report a
  meaningless failure — and its verdict would depend on the very state under test. It now measures the state
  and emits a TRUE skip (counted in SKIP, never PASS) **only** when `reason === "module-not-found"`. Any
  other class — EPERM, timeout, an unrelated plugin throw, an unreadable launcher — FAILS, because
  `finish()` exits 0 whenever FAIL is zero, so skipping on an infrastructure failure would silently retire a
  scenario whose artifacts are present.

**Conformance-harness preflight — real work, before Cargo.** After the build-prerequisite load and pinned
oracle-cache realization are confirmed, but before freshness tooling or the archive build, the gate runs
the harness-owned `packages/framework-conformance-harness/bin/gate-smoke.mjs` in `vapor` and `typescript`
modes. Vapor calls the real exported `ensureVaporRuntimePreloaded()` path; TypeScript calls the real exported
`observeTypeScript()` with a multi-file in-memory observation in the `workspace` domain and asserts its
export plus zero relevant diagnostics. No DOM or virtual-host logic is copied into the gate. Each mode runs
separately through `runContainedStep`, emits separately-attributed duration/RSS telemetry, and succeeds only
with the exact-key, mode-bound `verter-harness-smoke/v1` object receipt emitted after the work completes.
Non-zero exit, timeout/stall/memory ceiling or monitor abort, signal, spawn failure, or a missing/invalid/
mismatched/extra-key receipt is setup failure 127 with exact `HARNESS-SMOKE FAILED [<mode>]` attribution;
there is no skip, warning, or tolerance. GB15's real-production-CLI self-test leg makes `pnpm` unresolvable,
provides temporary executable `buf`/`oxfmt` shims, refuses to launch without that proof, and requires the
production freshness preflight to report a non-installing outcome, so it cannot install or lock the developer
checkout. Its smoke-omission mutation must continue past strict constructor refusal and prove the mutated real
CLI omitted the mode while improperly reaching Cargo. A successful oracle-cache load therefore does not claim
that DOM bootstrap or virtual TypeScript observation works.

**Shipped-cfg guard — currently skipped, what it covers, and what it does not.**

The lane is implemented and re-enablable (`SHIPPED_CFG_LANE_ENABLED` in `scripts/gate-internals.mjs`) but is
currently SKIPPED. A gate PASS is Surface 1 only. Until re-enabled, the defect class below is uncovered by
the gate.

`std::debug_assert!` does not evaluate its argument when `debug_assertions` is off, and
`#[cfg(debug_assertions)]` items do not exist there. Every shipped artifact (the LSP binary, napi, wasm) is
built that way. So a state mutation written inside a raw `std::debug_assert!` argument would run in every
debug test and in NO shipped build. Two structural layers close this for the macro-argument-evaluation
class: `verter_debug_assert`'s clippy `disallowed-macros` ban on the raw `std::debug_assert!`/`_eq!`/`_ne!`
macros (enforced by `cargo clippy --workspace --all-targets -- -D warnings`) routes every call site through
`verter_debug_assert!`/`_eq!`/`_ne!` instead; those macros themselves force-evaluate their condition/operands
into a local binding BEFORE branching on `cfg!(debug_assertions)`, so the argument always runs regardless of
profile — only the pass/panic check is debug-only. (`#[cfg(debug_assertions)]` items disappearing from a
shipped build is a separate class this pair does not cover — see below — and this guard is retained until
both the `debug_assert!`-argument class and the `cfg(debug_assertions)`/overflow-checked-arithmetic classes
are structurally eliminated repo-wide.)

- **Nothing else in the repo sees the runtime-no-op class this guard was designed for.** Surface 1 is a
  debug build, so an in-scope effect would happen and the tests would pass there regardless.
  `cargo check --workspace --release` compiles the shipped cfg but RUNS NOTHING, so it cannot observe a
  runtime no-op either. Only executing tests with `debug_assertions` off makes it observable.
- **How.** (a) `cargo check --workspace --all-targets --profile no-debug-assertions` — compile-only, catches
  an item wrongly hidden behind `cfg(debug_assertions)` or anything else that fails to compile under the
  shipped configuration, across the WHOLE workspace, without running anything. (b) A small package-scoped
  `cargo nextest run -p verter_shipped_cfg_contract --cargo-profile no-debug-assertions` — NOT another
  `--workspace` archive, NOT a second whole-workspace run: a normal `cargo nextest run -p <pkg>` that builds
  only that crate + its dependency closure. `verter_shipped_cfg_contract` is deliberately small ("dozens of
  tests at most") and covers only the production code paths its own tests exercise — see the crate's own
  module doc for the current per-crate audit of what has `cfg(debug_assertions)` blocks today.
- **It is also a compile gate, via step (a).** A dependency's item gated on `debug_assertions` is a profile
  accident: the predicate is evaluated per compilation unit, so a dependent crate's test code can reference
  an item that vanishes under another profile. Under this profile that is a COMPILE error in the gate rather
  than a shipped-build surprise.
- **Selection integrity.** The guard fails closed (exit 127) if `cargo nextest run -p
  verter_shipped_cfg_contract` selects a different number of tests than an INDEPENDENT scan of that crate's
  own source finds `#[test]` attributes — not merely "selected zero tests", which a regression that compiles
  out every behavioral test while leaving the two profile-sanity canaries intact would still satisfy.
- **NOT covered, explicitly.** It is not an optimised build: the profile inherits dev codegen (opt-level 0,
  no LTO, many codegen units), so optimisation-, inlining- and LTO-dependent behaviour is out of scope. It
  covers only `verter_shipped_cfg_contract`'s own tests, not the whole workspace — a `debug_assertions`-
  dependent regression in an untested production path elsewhere is not covered by step (b) (step (a) still
  catches a `cfg(debug_assertions)`-hidden COMPILE failure anywhere in the workspace). The real `release`
  profile is compiled only by `cargo check --workspace --release`, which runs no tests.
- **Cost.** One compile-only whole-workspace check under a different profile (a different unit hash, so no
  artifact is shared with the dev archive) plus one small package build+run — not a second whole-workspace
  archive+run.

Without Node, or to debug surface 1 in isolation, run `cargo nextest run --workspace` directly. It does not
cover the shipped-cfg class; the closest manual equivalent is `cargo check --workspace --all-targets
--profile no-debug-assertions` followed by `cargo nextest run -p verter_shipped_cfg_contract --cargo-profile
no-debug-assertions`. Run the gate with `node_modules` present (e.g. `pnpm install --frozen-lockfile` first
in a fresh worktree) so the freshness-tooling preflight is a no-op and the `cases::typeinfo_proto_ts_freshness::*`
byte-pin runs genuinely — with the tooling present a freshness failure is a HARD gate failure (exit 1, a real
stale-binding regression to regenerate + commit), not tolerated. On a buf-less runner (pnpm not resolvable AND
`buf` not resolvable) the Rust byte-pin SKIPS and PASSES, so the gate reports an ordinary PASS; the
verdict-gated tolerance flips ON there only as a latent safety net (PASS-WITH-TOLERATED appears solely if the
pair somehow emitted a tolerated FAIL despite `buf` being absent, which the skip does not). `oxfmt` absence
never grants tolerance (with `buf` present a missing `oxfmt` is a LOUD setup failure).

Bare `cargo test --workspace --tests` historically silently SKIPPED the verter_session integration suite (~4404 tests): a `session_metrics` Cargo feature unified differently standalone vs in the workspace, dropping those binaries from the workspace test set, so the run reported green while never compiling them. That feature has since been replaced by a runtime `HostConfig.metrics_enabled` toggle, and the skip no longer reproduces — confirmed by diffing the built executable sets of `cargo test --workspace --tests --no-run -v` against `cargo test -p verter_session --tests --no-run -v` (same verter_session executables in both). Still, `cargo test --workspace --tests` must NOT be used as the sole Rust gate — run `node scripts/gate.mjs` (Surface 1; the shipped-cfg guard is currently skipped) or `cargo nextest run --workspace` directly.

Do not run bare `cargo test --workspace` (no `--tests`) by default — it also runs doctests and example builds, substantially slower. Run doctests (`cargo test --workspace --doc`) only when rustdoc examples changed or explicitly requested.

### §1a Mutation Recipes

Use a reversible mutation recipe only when preflight identifies a plausible critical fail-closed/correctness boundary or reproduced defect for which the mutation materially proves discrimination. Verify the starting SHA; prove the plant applied; run the named guard and require RED; restore; verify a clean original SHA; run GREEN; and run an unplanted control. Persist commands and results. Read every new test body; reject stubs, always-true assertions, implementation mirrors, duplicate permutations, and non-discriminating characterization. When a mutation recipe is selected as gate-bearing evidence, the independent confirmer replays it; do not sample within that selected recipe set.

Canonical in-tree examples of fully self-contained recipes (in-memory plant → expected verdict → trivial restore + GREEN control): the Vue structural-conformance discriminator `crates/verter_vue_conformance/tests/cases/conformance_discriminator.rs` (cosmetic-PASS vs behavioral-FAIL mutations on committed goldens, each plant proven applied) and the Svelte oracle discriminator in `crates/verter_compiler/tests/cases/svelte_client_emit_topology.rs`.

### Timeout Is Never a Pass

A timeout or incomplete run is never green and never presumed environmental. Rerun the timed-out test in isolation with an adequate timeout and no co-resident heavy work: if it clears → environmental (retain both artifacts); if it repeats → collect hang diagnostics; if classification stays ambiguous → HARD FAIL. The advertised slow-timeout must match the configured one — `.config/nextest.toml` advertises ~60s but configures 5s×3, killing valid tests around 15s on an 8GB host; fix that mismatch rather than tolerating false timeouts. Genuinely long tests get explicit per-test overrides.

### Enum-variant ripple (silent catch-all absorption)

When changing a variant of a widely-matched enum (`SemanticQueryKey`, `TypeExpr`, `WorkKind`, `EmitOp`, etc.), `cargo check` does NOT flag a `_ =>` catch-all that silently absorbs the changed variant. Grep every `match` on the enum for `_ =>` / `..` wildcards (and every TS `default:` switch) and confirm each intends the new behavior. Distinguish ANALYZER-IR consumers (which see the raw analyzer variants) from DISPATCH-RAISED consumers (which see the collapsed forms produced at `raise.rs`) — the same logical change may need edits in both.

### Test Validation Pattern

All codegen tests must validate generated JS syntax:

```rust
let result = compile_sfc(source);
let tpl = result.template.unwrap();
// Parse generated code with OXC to verify valid JS
let parsed = oxc_parser::Parser::new(&alloc, &tpl.code, source_type).parse();
assert!(parsed.errors.is_empty(), "JS parse error: {:?}\n{}", parsed.errors, tpl.code);
```

### Never Hand-Edit Generated Goldens

Regenerate goldens from their authoritative source and record the source-manifest identity in the review evidence packet. A hand-edited golden is a defect, not a fixture update.

### Testing Strategy

- **Unit tests**: Test individual plugins with minimal SFC snippets
- **Integration tests**: Test full transformation pipeline
- **Type tests**: Verify TypeScript inference (using `vitest --typecheck`)
- **Sourcemap tests**: Verify position mappings

### Architecture Guard Rule (MANDATORY)

Every new `CRITICAL` architecture rule must land with primary EXECUTABLE enforcement in the same change. Primary architecture enforcement uses type or capability boundaries, dependency checks, AST-aware analysis, or a discriminating behavioral guard that fails against old behavior. Textual/substring scanning may exist only as a secondary retired-symbol tripwire and cannot establish architectural compliance. Prose plus a future follow-up is insufficient — a rule without primary executable enforcement is not durable enough for this repo's migration style.

### Test Hermeticity (MANDATORY)

Default-run tests must depend only on locally-vendored fixtures. The canonical run (`node scripts/gate.mjs`: one workspace archive/list, then archive-backed Surface 1; the shipped-cfg lane is currently skipped) must compile and pass on a fresh checkout without any `.integration-tests/repos/<third-party>/...` clones, sibling repositories, or other external corpora present alongside the workspace.

When needing fixtures from a third-party project (e.g., `nuxt-ui` Vue corpus), vendor a snapshot into the consuming crate's `tests/<feature>/fixtures/` and refer to them with `include_str!("./fixtures/...")` or path-based loaders. Preserve upstream license attribution in sibling `LICENSE.md` and `README.md` for provenance.

Tests requiring live external corpora (e.g., periodic drift detectors comparing the vendored snapshot against the upstream submodule) must be gated behind a Cargo feature naming the corpus dependency:

```toml
# crates/<crate>/Cargo.toml
[features]
external-corpus = []
```

```rust
#![cfg(feature = "external-corpus")]
//! Optional drift detector — gated so the default gate run
//! (`node scripts/gate.mjs`) stays hermetic.
```

Guard `external_corpus_paths_not_present_outside_gated_tests` (in `crates/verter_session/tests/cases/architecture_guards.rs`) rejects `include_str!` / `include!` / path-string references to `.integration-tests/repos/...` from any test file not gated behind such a feature.
