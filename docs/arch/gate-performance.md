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
| `0`  | PASS (or PASS-WITH-TOLERATED — the env-only tolerated failure was the only failure) |
| `1`  | FAIL — at least one non-tolerated test failed |
| `124`| TIMEOUT — the whole-gate hard deadline (default 50 m) expired |
| `125`| STALL — the stall detector tripped (no progress for the stall window) |
| `126`| LOCK-REFUSED — a live gate already holds the single-flight lock |
| `127`| USAGE / SETUP error — bad argv, or required tooling missing |

The **only tolerated failure** is the env-only `cases::typeinfo_proto_ts_freshness::*`
pair (the buf/oxfmt byte-pin, which depends on the locally available `buf` and
`oxfmt` binaries). `gate.mjs` surfaces it as **PASS-WITH-TOLERATED** and still
exits `0`. Every other failure is a real FAIL.

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
| L6 | Opt-in build linker + CI sccache | DEFERRED | Alternate linker config (`.cargo/config.perf.toml` for lld/mold/zld) plus CI-only `sccache`. Deferred because it is not a default contributor or canonical-gate behavior, requires per-platform availability/correctness validation on macOS/Windows/Linux, and the already-landed single-compile archive runner plus integration-test binary consolidation removed the dominant measured gate waste. Rough value: incremental link/build latency reduction for opted-in local users and warm CI jobs, likely meaningful only after measuring the current consolidated-gate link time. Pick up when future gate profiling shows link time or repeated CI cold builds remain a top bottleneck, or when CI queue cost justifies a hermetic cache-key rollout. Risk of not doing it: leaves some local opt-in and warm-CI compile/link savings unrealized; no known correctness, coverage, or default gate-performance regression. |

## Tooling

| Script | Role |
|--------|------|
| `scripts/gate.mjs` | The canonical Rust gate CLI — archive → nextest run → direct libtest → aggregated verdict. Only bare `node scripts/gate.mjs` is the gate. |
| `scripts/gate-internals.mjs` | Reusable gate internals (classifiers, single-flight mutex, contained-step runner, multi-step seam); imported by `gate.mjs` and by the self-test. |
| `scripts/gate-selftest.mjs` | Drives the cargo-free seam/classifier scenarios in-process against `gate-internals.mjs` — exercises gate logic without building the workspace. |
| `scripts/gate-selftest-runner.mjs` | Runner harness for the gate self-test scenarios. |
| `scripts/target-health.mjs` | Inspects the runner-owned target tree (size / file-count health) for gate-build and disk-reclaim diagnostics. |
| `scripts/check-integration-test-layout.mjs` | Fast-fail CI Node check of the anti-binary-growth integration-test layout; runs before the Rust gate. |
| `scripts/integration-test-layout-allowlist.json` | Single source-of-truth allowlist for the two sanctioned separate-process integration-test binaries; read by both the Node check and the in-gate Rust guard. |
