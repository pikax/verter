# Gate Performance — the single-compile archive gate runner

## Overview

`node scripts/gate.mjs` is the canonical Rust test gate. It builds the entire
workspace test universe **once** — via `cargo nextest archive` into a
runner-owned `target/gate-runner` directory — and then runs **both**
verification surfaces from the **same archived artifacts**, with **zero second
compile**:

- **Surface 1 — `cargo nextest run` from the archive.** Per-test **process
  isolation**. Catches the ordinary regression set; nothing that survives a
  fork leaks between tests.
- **Surface 2 — the `verter_session` libtest binaries executed directly.**
  **In-process / multi-test-per-process** execution of every `verter_session`
  suite whose nextest kind is `lib` or `test` (the lib unit-test binary plus
  every `tests/*.rs` integration binary — exactly what `cargo test --tests`
  builds). Surfaces shared-process state bugs the isolated path structurally
  cannot.

The aggregated verdict over both surfaces is the gate result. `gate.mjs` runs the
same workspace nextest surface **and** the same `verter_session` lib/test binaries
directly, preserving nextest's per-test process isolation **and** the in-process
`verter_session` surface. It runs those `verter_session` binaries under the
workspace-unified `session_metrics` feature set (ON — see below), intentionally
replacing the old package-scoped default-feature (`session_metrics` OFF) rebuild
surface rather than reproducing its feature config; that ON config is the one the
shipped LSP binary actually uses, and it is what eliminates the second compile. No
gate is weakened: every test target the old pair compiled is still run, plus the
same direct `verter_session` surface. Clippy
(`cargo clippy --workspace -- -D warnings`), formatting
(`cargo fmt --all --check`), hermeticity, and on-demand doctests remain separate
end-of-change checks layered on top of the gate, exactly as before.

### Why: the `session_metrics` double-compile root cause

The previous gate issued two cargo commands:

```
cargo nextest run --workspace
cargo test -p verter_session --tests
```

`cargo nextest run --workspace` and `cargo test --workspace` share Cargo's
feature unification. A downstream crate (`verter_lsp`) depends on
`verter_session` with `features = ["session_metrics"]`, so the real LSP binary
forces that feature **on** in the workspace build (`verter_napi` exposes an opt-in
`session_metrics` forwarding feature but does not enable it by default). So the
workspace build resolves `verter_session` with `session_metrics` **ON**.

The package-scoped `cargo test -p verter_session` resolution builds
`verter_session` with `session_metrics` **OFF** (its default) and a different
dev-dependency closure. Different features + different dep closure ⇒ a different
unit hash ⇒ an **artifact-reuse miss** ⇒ a full recompile of the
`verter_session` reverse-dependency chain on the very next gate command. This
was a measured **68-unit / ~58 s recompile on every gate cycle** — pure waste,
recompiling the same source under a second feature resolution.

`gate.mjs` issues exactly **one** build command — the single `--workspace`
archive build — and never issues the package-scoped `-p verter_session`
resolution. It therefore tests the **workspace-unified (`session_metrics` ON)**
configuration, which is the **production-reachable** one (it is what the shipped
LSP binary uses, and it is also reachable from an opt-in `verter_napi/session_metrics`
build), and structurally cannot incur the second-resolution recompile. Surface 2 runs the same `verter_session` suite the package-scoped
command would have run — same tests, same `cwd` (the `verter_session` manifest
dir) and the runtime Cargo env those tests read (`CARGO_MANIFEST_DIR` +
`CARGO_TARGET_DIR`) — modulo the `session_metrics` cfg being ON (the production
configuration) rather than OFF.

## The canonical gate flow

```
cargo nextest archive  (single --workspace build → target/gate-runner archive)
        │
        ├── cargo nextest list   (enumerate suites from the archive: kind lib / bin / test)
        │
        ├── SURFACE 1: cargo nextest run  (from the archive — per-test process isolation)
        │
        ├── SURFACE 2: direct execution of every verter_session lib/test binary
        │              from the same archive  (in-process / multi-test-per-process)
        │
        └── aggregated verdict over both surfaces
```

No surface recompiles. Both read the artifacts the archive build already
produced.

### Exit contract

`exit 0` is **operation-scoped**: it means the operation you ran succeeded, not a
blanket gate pass. Only bare `node scripts/gate.mjs` (no mode flag) is **the
gate**, and its `exit 0` is the only true gate-pass contract.

| Exit | Meaning |
|------|---------|
| `0`  | PASS. On a genuinely-tooling-less runner (`buf` not resolvable) the Rust freshness pair SKIPS and passes, so the gate reports an ordinary PASS. PASS-WITH-TOLERATED is the *narrow* sub-verdict reached only when the preflight allowed tolerance AND the freshness pair nonetheless produced a tolerated `FAIL` line (a latent safety net, not the normal buf-less path) |
| `1`  | FAIL — at least one non-tolerated test failed |
| `124`| TIMEOUT — the whole-gate hard deadline (default 50 m) expired |
| `125`| STALL — the stall detector tripped (no progress for the stall window) |
| `126`| LOCK-REFUSED — a live gate already holds the single-flight lock |
| `127`| USAGE / SETUP error — bad argv, or required tooling missing |

The **only tolerated failure** is the `cases::typeinfo_proto_ts_freshness::*` pair
(the buf/oxfmt byte-pin, which depends on the workspace `buf` and `oxfmt`
binaries), and its tolerance is **verdict-gated** on a freshness-tooling preflight
that runs before the archive build. The preflight ensures those binaries are
present — auto-running `pnpm install --frozen-lockfile` (inside the
mutex/timeout/stall machinery) when the `node_modules/.bin` shims are missing —
and then gates the byte-pin tolerance on the outcome:

- **Tooling present (the normal `node_modules` path)** — the tools resolve,
  tolerance is **OFF**, and a freshness failure is a **HARD FAIL** (exit `1`): a
  real stale-binding regression, to be regenerated via the proto pipeline and
  committed, not waved through.
- **Genuinely-tooling-less runner (pnpm not resolvable AND `buf` not resolvable)**
  — this is exactly the condition under which the Rust freshness pair **skips
  gracefully and passes** (`locate_buf_binary` early-returns `None`, the test
  `eprintln!`s a skip and `return`s — a passing libtest test with no `FAIL` line).
  So the gate sees no freshness failure and reports an **ordinary PASS** (exit
  `0`). The preflight does flip the verdict-gated tolerance **ON** here, but that
  is a **latent safety net**: it would surface **PASS-WITH-TOLERATED** only in the
  unusual case the pair produced a tolerated `FAIL` line *despite* `buf` being
  absent — which the normal skip path does not emit. `oxfmt` absence **never**
  grants tolerance: with `buf` present the test runs, so a missing `oxfmt` is a
  **LOUD setup failure** (exit `127`), not a degraded un-oxfmt'd run.
- **Deterministic install failure (e.g. a frozen-lockfile mismatch)** — a **LOUD
  setup failure** (exit `127`), never silently tolerated. This applies **when an
  install is actually attempted**: when both `node_modules/.bin/{buf,oxfmt}` shims
  are already present the preflight returns `already-present` and never runs
  `pnpm install`, so no install failure can occur.

Every other failure is a real FAIL.

### Cross-platform process containment

A gate step (cargo → rustc → test binary) is a process tree that must be reaped
cleanly on timeout, stall, or interrupt, on every platform:

- **POSIX** — the step is spawned **detached** in its own process group
  (`PGID == PID`); the whole tree inherits the PGID. Reap is a **negative-PGID**
  `SIGTERM` → grace → `SIGKILL`, followed by a verification poll that confirms
  the group is actually dead.
- **Windows** — `taskkill /PID <pid> /T /F` (tree kill) followed by a re-query
  poll.

A **provenance sweep** is the backstop after any abnormal termination: it
`TERM`→`KILL`s any `cargo`/`rustc`/`cargo-nextest`/`nextest` process whose command
line references the **runner-owned** `target/gate-runner` dir — never the repo
root — so a developer's interactive `cargo` or rust-analyzer (which carry the
repo root but write the default `target/debug`) is never touched. A
single-flight mutex (an atomic lockdir with a gate-owned sentinel storing the
owning repo realpath) refuses concurrent gates (`LOCK-REFUSED`) and never deletes
a foreign checkout's lock.

### Operation-scoped exit (non-gate operations)

`gate.mjs` exposes only the real gate plus two strict non-gate operations. Both
are mutually exclusive and **argv-strict** (any stray flag or positional
alongside either is a usage error, exit `127`):

- **`--help`** — prints usage, exits `0`. Not a gate pass.
- **`--prepare`** — a **Gatekeeper warm-pass**: it builds the archive and warms
  the first-launch assessment so a subsequent real gate starts warm. It does
  **not** run tests, and it does **not** disable the Gatekeeper or remove any
  gate cost — it only pre-pays the build. Its `exit 0` means **PREPARED**, carries
  a `PREPARED_NOT_GATE` marker, and contains no `PASS` token (so a CI `grep PASS`
  cannot mistake it for a verdict).

There is no test-seam, classifier hook, custom-command mode, or environment
variable that can make `node scripts/gate.mjs <anything>` return the gate-success
contract without actually building and running the suite. The reusable internals
(classifiers, mutex, contained-step runner) live in `gate-internals.mjs` and are
exercised by `gate-selftest.mjs` in-process, never via a magic flag on the
production CLI.

## The integration-test consolidation end-state

The gate's build cost is dominated by the number and size of distinct test
binaries. The codebase consolidated **136 standalone integration-test binaries
→ 15 integration targets**: **13** crate `tests/main.rs` binaries (cases live
under `tests/cases/` and are wired through `main.rs`) plus **2** allowlisted
separate-process binaries.

The 2 allowlisted exceptions are genuine "needs a separate test process" cases —
process-global state that must be isolated in its own binary:

| Package | Target | Why a separate process |
|---------|--------|------------------------|
| `verter_session` | `allocator_canaries` | a counting `#[global_allocator]` — the process-global allocator must own its own binary so the canary counts are not perturbed by other tests |
| `verter_lsp` | `lsp_audit_trace_out_env_var` | a process-global `VERTER_LSP_AUDIT_TRACE_OUT` env mutation — a separate binary isolates the mutation and preserves exact test parity |

The layout is enforced by an **anti-binary-growth dual guard** (see the
`### Anti-Binary-Growth Integration-Test Layout (CRITICAL)` heading in
[`CLAUDE.md`](../../CLAUDE.md)): a fast-fail CI Node check
(`scripts/check-integration-test-layout.mjs`, runs before the Rust gate) and an
in-gate Rust mirror (`crates/verter_session/tests/cases/integration_test_layout_guard.rs`),
both reading the single source-of-truth allowlist
(`scripts/integration-test-layout-allowlist.json`). The allowlist is exact
(package + target + repo-relative `src_path`, no globs/prefixes) and
**stale-failing**: an allowlisted target that no longer exists in
`cargo metadata`, or whose `src_path` moved, fails the guard, so a removed binary
cannot leave a dead exception behind. A second top-level `tests/*.rs` (which
Cargo would auto-promote into its own test binary and re-balloon the gate) is
forbidden unless exactly allowlisted.

## Cache architecture note

The double-compile this design eliminates is a direct consequence of Cargo's
feature unification, not a tooling bug:

- `cargo nextest run --workspace` builds `verter_session` with `session_metrics`
  **ON** (forced by `verter_lsp` in the unified workspace graph; `verter_napi` only
  exposes an opt-in forwarding feature) — and the archive build reuses exactly those
  artifacts, so a re-run is **noop-warm**.
- `cargo test -p verter_session` builds `verter_session` with `session_metrics`
  **OFF** (the crate default) and a narrower dev-dep closure ⇒ a different unit
  hash ⇒ an artifact-reuse **miss** ⇒ a full recompile of the reverse-dependency
  chain.

The archive flow sidesteps this by issuing **one** `--workspace` build and
deriving **both** surfaces from its artifacts. Surface 2 reuses the
already-built `verter_session` lib/test binaries directly instead of asking Cargo
to re-resolve and rebuild them under the package-scoped feature set. The
configuration under test is the production-reachable workspace-unified one.

## Measured improvement

| Metric | Baseline (`@e0c621b31`, cold) | After (consolidated + `gate.mjs`) |
|--------|-------------------------------|-----------------------------------|
| 2nd-command recompile units | 68 units (~58 s) per gate cycle | 0 (single-compile archive) |
| Distinct nextest test binaries | 136 | 47 (−65.4%) |
| Integration-test targets | ~104 test-kind | 15 (13 `main` + 2 allowlist) |
| Summed test-binary size | 2712 MB | 1045.9 MiB (~−60%) |
| Cold gate wall-clock | ~323 s (201 s build + 58 s recompile + 64 s run) | 243 s (4 m 3 s) single-compile, both surfaces — 15551 pass / 0 fail / 547 skip |
| `target/` after `cargo clean` | 79.3 GB / 289,371 files (accumulated) | 10.6 GB / 26,972 files (clean cold gate build) |

The **47 distinct nextest binaries** is the full post-consolidation
`cargo nextest list` binary count (its recorded decomposition at measurement time
was 21 lib + 10 bin + 1 proc-macro + 15 integration targets — a nextest-list
breakdown, which counts test-runnable binaries and is not the same axis as a raw
`cargo metadata` target list); the **15 integration targets** is the test-kind
subset. They measure different things and are both kept — do not conflate them.

A `cargo clean` reclaimed the accumulated **79.3 GiB / 289,371 files** (as the
`cargo clean` report stated it); the cold gate then rebuilds clean to **~10.6 GB /
26,972 files** (`du` report). The two figures come from different tools, hence the
GiB/GB suffix difference.

## L1 — DROPPED (measured no-op)

A proposed `[profile.test] debug = "line-tables-only"` was **dropped** as a
measured no-op: the `test` profile already inherits
`[profile.dev] debug = "line-tables-only"`, so setting it explicitly on the
`test` profile changes nothing about the emitted debug info or the build cost.

## Deferred work (debt ledger)

| ID | Item | Status | Notes |
|----|------|--------|-------|
| L6 | Opt-in build linker + CI sccache | SPLIT: sccache opt-in LANDED / linker DEFERRED | The sccache half is landed as an opt-in helper: `scripts/sccache-env.mjs` (see "Opt-in shared sccache (orchestration)" below) computes a shared compiler-cache environment on demand; the default build, `.cargo/config.toml`, and every CI runner without sccache are untouched. The alternate-LINKER half remains DEFERRED: repo M3/M4 measurements show the macOS `ld-prime` default already beats brew `lld` by ~9% and `mold` is Mach-O-N/A, so no alternate linker wins on measured hosts; pick the linker piece up only if future measurements on other hosts show a win. |

## Opt-in shared sccache (orchestration)

Multi-agent orchestration spawns many `git worktree`s, and each one cold-compiles
its own `target/`. `scripts/sccache-env.mjs` is a portable, opt-in helper that
lets all worktrees SHARE one sccache compiler cache. It computes the sccache
environment and either prints it or runs a child command with it merged in —
it is never itself a rustc wrapper, never execs rustc, and never impersonates
sccache.

Opt-in invocations:

```bash
# RECOMMENDED run path: run the canonical gate under the shared cache
# (hard-fail if sccache is missing). --exec passes argv straight to the child
# (spawnSync, no shell), so values with spaces are safe:
node scripts/sccache-env.mjs --exec --required -- node scripts/gate.mjs

# Inspect the computed KEY=VALUE lines (line-parseable output for tooling and
# inspection ONLY — values are unquoted, so it is NOT safe for raw shell
# `eval`; use --exec to run commands):
node scripts/sccache-env.mjs --print-env
```

Contract:

- **CI-safe default: OFF.** Nothing activates sccache unless a caller explicitly
  invokes the helper. `.cargo/config.toml` is untouched, no default
  `rustc-wrapper` exists, and a runner without sccache is unaffected. Without
  `--required`, a missing sccache is a LOUD no-op: the helper prints a stderr
  warning and runs the child with the caller's unmodified environment (plain
  rustc); with `--required` it exits non-zero without running the child.
- **`CARGO_INCREMENTAL=0`** is set only inside the computed environment because
  sccache cannot cache incremental compilation artifacts. It is opt-in-only —
  never a global dev default; a normal `cargo` invocation keeps incremental
  compilation.
- **Cross-worktree sharing.** `SCCACHE_DIR` defaults to
  `~/.cache/verter-sccache` (outside any worktree; respected if already set),
  and `SCCACHE_BASEDIRS` defaults to every root reported by
  `git worktree list --porcelain`, joined with the platform path delimiter, so
  builds under different worktree roots relativize to identical cache keys and
  hit the same cache entries. Both are overridable via the environment.
- **Tunable cache size.** `SCCACHE_CACHE_SIZE` defaults to `10G`; respected if
  already set.
- **Run via `--exec`, not shell eval.** `--print-env` emits newline `KEY=VALUE`
  lines with UNQUOTED values — line-parseable for tooling/inspection, but NOT
  safe for raw `eval "$(...)"` when a value contains spaces. The recommended
  run path is `--exec -- <cmd>`: argv reaches the child directly (spawnSync,
  no shell), so no quoting hazard exists.
- **`SCCACHE_BASEDIRS` is a point-in-time snapshot** captured when the helper
  runs: a worktree created AFTER the value was computed is not covered until
  the helper runs again. Invoke `--exec` per build (each invocation re-derives
  the worktree list) rather than capturing the env once and reusing it.
- **Fail-closed basedir validation.** Every `SCCACHE_BASEDIRS` entry — derived
  or overridden via the environment — must be a non-empty absolute path;
  sccache rejects relative basedirs (its server refuses to start), so the
  helper exits non-zero with a diagnostic instead of emitting an invalid value.
- **Executable-only discovery.** A candidate becomes `RUSTC_WRAPPER` only if it
  is a regular file AND executable (`X_OK` on POSIX; on Windows, where `X_OK`
  is meaningless, an executable extension per `isWindowsExecutableName`: the
  default set `.exe`/`.com`/`.bat`/`.cmd` is ALWAYS accepted — a real
  `sccache.exe` is executable even when `PATHEXT` is empty or omits `.EXE` —
  unioned with the `PATHEXT` entries, which extend but never shrink the set).
  A non-executable file named `sccache` counts as ABSENT — optional mode
  no-ops, `--required` fails cleanly — never a broken wrapper.
- **`--exec` child must be a real executable.** The child is spawned without a
  shell (no `shell: true`), so on Windows a `.cmd`/`.bat` shim cannot be
  spawned directly — invoke the underlying `.exe` or pass an explicit
  interpreter. The documented usage (`node <script>`) is unaffected.
- **Portability.** Node stdlib only (`node:os`, `node:path`, `node:fs`,
  `node:process`, `node:child_process`): discovery scans `PATH` split on
  `path.delimiter` (plus `sccache.exe` on win32), honors an absolute
  `VERTER_SCCACHE_BIN` override, and contains no hardcoded per-OS paths — the
  helper behaves identically on macOS, Windows, and Linux.

Self-tests: `scripts/sccache-env.test.mjs` (Vitest; hermetic —
presence/absence forced via `VERTER_SCCACHE_BIN`; the win32 extension decision
is the exported pure `isWindowsExecutableName`, unit-tested on any OS). Run
via the root `pnpm run test:scripts` script (an explicit-path root vitest run
— `scripts/` is not a workspace package, so the root `pnpm test` /
`pnpm -r run test` never reaches it). In CI the self-tests run in the
`js-build-test` job ("Rust build-tooling helper self-tests (sccache-env)"
step in `.github/workflows/ci.yml`); that job is path-filtered, and the
`detect-changes` `js` filter lists `scripts/sccache-env.mjs` and
`scripts/sccache-env.test.mjs` explicitly, so a change to either file
triggers the job.

## Tooling

| Script | Role |
|--------|------|
| `scripts/gate.mjs` | The canonical Rust gate CLI — archive → nextest run → direct libtest → aggregated verdict. Only bare `node scripts/gate.mjs` is the gate. |
| `scripts/sccache-env.mjs` | Opt-in shared-sccache environment helper (`--exec -- <cmd>` recommended; `--print-env` for tooling/inspection, not shell-eval; `--required`); see "Opt-in shared sccache (orchestration)" above. |
| `scripts/sccache-env.test.mjs` | Vitest self-tests for `sccache-env.mjs` (`pnpm run test:scripts`; runs in the `js-build-test` CI job, which the `js` path filter triggers on changes to either sccache-env file); hermetic via `VERTER_SCCACHE_BIN`. |
| `scripts/gate-internals.mjs` | Reusable gate internals (classifiers, single-flight mutex, contained-step runner, multi-step seam); imported by `gate.mjs` and by the self-test. |
| `scripts/gate-selftest.mjs` | Drives the cargo-free seam/classifier scenarios in-process against `gate-internals.mjs` — exercises gate logic without building the workspace. |
| `scripts/gate-selftest-runner.mjs` | Runner harness for the gate self-test scenarios. |
| `scripts/target-health.mjs` | Inspects the runner-owned target tree (size / file-count health) for gate-build and disk-reclaim diagnostics. |
| `scripts/check-integration-test-layout.mjs` | Fast-fail CI Node check of the anti-binary-growth integration-test layout; runs before the Rust gate. |
| `scripts/integration-test-layout-allowlist.json` | Single source-of-truth allowlist for the two sanctioned separate-process integration-test binaries; read by both the Node check and the in-gate Rust guard. |
