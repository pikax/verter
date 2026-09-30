# Testing Guide

::: warning Pre-Release
Verter is pre-release software. APIs may change between releases — see the [API Stability](/api-stability) document.
:::

Verter requires thorough testing for all changes. This guide covers testing patterns for both TypeScript and Rust code.

## TypeScript Tests

```bash
pnpm test                                      # Every package-owned test script
pnpm --filter @verter/typescript-plugin test  # One package
pnpm exec vitest run path/to/test.ts           # One test file
```

Tests are co-located as `*.spec.ts` files next to their source files. Type tests in `packages/types/` use `vitest --typecheck`.

### Public Type Contracts

Vitest does not evaluate type-level assertions and the package builds exclude
spec files, so type-only contract fixtures need their own TypeScript project.
`packages/component-meta` checks `test/*.test-d.ts` through
`tsconfig.contract-tests.json`:

```bash
pnpm --filter @verter/component-meta build      # the fixtures assert built dist declarations too
pnpm --filter @verter/component-meta test:types
```

CI runs that script in the *JS Build & Test* lane, after the TypeScript
packages are built. A type contract that no lane compiles is not a check — see
[Removing a Mechanism](./removing-a-mechanism.md).

### Sourcemap Testing

For testing sourcemap accuracy (see `macros.map.spec.ts` for examples):

```typescript
const { s, source, result } = processMacrosForSourcemap(code);
const map = s.generateMap({ source: "test.vue" });
```

## Rust Tests

```bash
node scripts/gate.mjs                           # Canonical provider-free core Rust gate
node scripts/compile-contracts.mjs              # Standalone compile-fail contracts
pnpm proto:check                                # Generated proto freshness
cargo test -p verter_compiler test_name         # Specific test by name
cargo test -p verter_compiler -- --nocapture    # With stdout output
```

Real tsserver/tsgo provider suites and Svelte conformance run in dedicated CI
jobs, outside the canonical nextest surface. Provider jobs use serial libtest
execution so each third-party engine has one managed lifecycle.

### Test File Organization

When a Rust source file's inline `#[cfg(test)] mod tests` block exceeds ~400 lines, extract tests to a separate sibling file to keep source files focused on production code.

**For standalone files** (e.g., `analysis.rs`):

```rust
// In analysis.rs:
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

The extracted file contains `use super::*;`, helper functions, and `#[test]` functions directly — no wrapping `mod tests { }` block.

Small rule files (e.g., diagnostic rules at 50-150 lines) can keep tests inline.

## TDD Workflow

**Test-Driven Development is mandatory.** For every change:

1. **Write failing tests first** -- demonstrate the expected behavior and verify the tests fail
2. **Implement the minimum code** to make the failing tests pass
3. **Refactor** while keeping tests green

This applies to:

- **New features**: Add tests covering the new functionality
- **Bug fixes**: Add tests that would have caught the bug
- **Refactoring**: Ensure existing tests pass, add tests for edge cases discovered
- **Behavioral changes**: Add tests verifying the new behavior

## Assertion Requirements

### Always Include Negative Assertions

Every test must verify both what SHOULD be present AND what should NOT be present. A test that only checks for expected output can pass even when the output contains invalid or broken content alongside the expected content.

#### Rust Example

```rust
// GOOD: Both positive and negative assertions
let result = gen_tsx_template(
    r#"<template><div v-if="show">hello</div></template>"#
);
assert!(
    result.contains("_ctx.show ?"),
    "should have ternary condition"
);
assert!(
    !result.contains("v-if"),
    "v-if attribute must be removed from JSX"
);

// BAD: Only positive assertion -- passes even if v-if="show" leaks into output
let result = gen_tsx_template(
    r#"<template><div v-if="show">hello</div></template>"#
);
assert!(
    result.contains("_ctx.show ?"),
    "should have ternary condition"
);
// Missing negative assertion!
```

#### TypeScript Example

```typescript
// GOOD
expect(output).toContain('_createElementVNode("div")');
expect(output).not.toContain("v-for");
expect(output).not.toContain("v-if");

// BAD
expect(output).toContain('_createElementVNode("div")');
// No check that directives were removed
```

### Type Tests

Type tests must include both positive assertions and `@ts-expect-error` negative assertions. This prevents `any`, `unknown`, or `never` types from silently passing tests:

```typescript
it("type is correctly inferred", () => {
  type Result = SomeTypeHelper<Input>;

  // Positive assertion -- type matches expected
  assertType<Result>({} as ExpectedType);
  assertType<ExpectedType>({} as Result);

  // @ts-expect-error -- Result is not any/unknown/never
  assertType<{ unrelated: true }>({} as Result);
});
```

### Codegen Tests (Rust)

All codegen tests must validate that the output is syntactically valid JavaScript using the OXC parser. Use the `gen_and_validate()` helper:

```rust
use crate::test_utils::gen_and_validate;

#[test]
fn test_v_for_codegen() {
    let result = gen_and_validate(
        r#"<template><div v-for="item in items" :key="item.id">{{ item.name }}</div></template>"#
    );
    assert!(result.contains("_renderList"), "should use _renderList helper");
    assert!(!result.contains("v-for"), "v-for must not appear in output");
}
```

## Test Output Best Practices

When running tests where you need to inspect output, redirect to a temp file first to avoid re-running expensive test suites:

```bash
# Good: capture once, search multiple times
pnpm exec playwright test --project=preview 2>&1 | tee /tmp/e2e-output.log
grep -i "fail\|error" /tmp/e2e-output.log

# Bad: re-running the full suite each time
pnpm exec playwright test --project=preview 2>&1 | grep "fail"
pnpm exec playwright test --project=preview 2>&1 | grep "error"  # wasteful re-run
```

## Integration Tests

Integration tests verify Verter against real-world open-source Vue projects:

```bash
# Run integration test for a specific project (skip baseline, reuse checkout).
# Substitute the project name for $PROJECT — angle-bracket placeholders are shell
# redirects, so a copy-pasted `<project>` is a syntax error, not a prompt to fill in.
pnpm integration-test --skip-build --skip-baseline --no-clone "$PROJECT"
```

See the [CI/CD page](./ci-cd.md) for details on the integration test workflow.

## Compiler Probes and Benchmarks Under a Resource Cap

Never run a compiler probe (tsc, a Verter release binary) or a benchmark arm
bare: one probe once reached 132 GB and hung the host. Run it under
`verter-supervise` (crate `verter_supervise`), which contains the whole process
tree, enforces a deadline, tears every descendant down, and writes a result
document:

```bash
cargo build -p verter_supervise --release
verter-supervise run --mem-mb 8192 --timeout-ms 120000 --out out/result.json \
  [--sample-ms 50] [--cwd DIR] [--env KEY=VALUE ...] [--allow-sampled] [--host-reserve-mb N] \
  -- PROGRAM ARGS...
```

The child's stdout and stderr go to `out/result.stdout.log` and
`out/result.stderr.log`. The supervisor exits with the child's exit code, or
124 (deadline), 137 (memory cap), 130 (cancelled: Ctrl-C, Ctrl-Break, SIGINT,
SIGTERM or SIGHUP to the supervisor) or 125 (the supervisor refused or failed:
the run is invalid). A Unix child killed by a signal it did not get from the
supervisor exits 128 plus the signal.

`result.json` (`schema: 1`) records `program`, `args`, `cwd`, `startedAt`,
`launched`, `wallMs` (from the child's release to its exit), `exitCode`,
`signal`, `killedBy` (`null`, `"memory"`, `"timeout"`, `"cancel"`,
`"supervisor-error"`, `"pressure"`), `memLimitBytes`, `killTriggerBytes`,
`timeoutMs`, `peakBytes` with the OS metric it came from in `peakMetric`,
`containment` (`"hard"` or `"sampled"`), `backend`, `overshootBoundBytes`,
`observedOvershootBytes`, `terminationLatencyMs`, `sampling` (interval, count,
largest observation age and sweep time), a bounded `samples` series
(`{tMs, bytes}` of `sampleMetric`), `processCount`, `descendantsKilled`,
`cpuUserMs`, `cpuKernelMs`, `stdoutPath`, `stderrPath` and `errors`.
Telemetry that could not be read is `null`, never `0`. A killed supervisor
writes no result: a missing `result.json` means the run is invalid.

Containment per platform:

| Platform | `backend` | `containment` | Mechanism | `peakMetric` |
| --- | --- | --- | --- | --- |
| Windows 10+ | `windows-job-object` | `hard` | The child is created suspended and already inside a job object (`PROC_THREAD_ATTRIBUTE_JOB_LIST`) with a job-wide committed-memory limit, kill-on-close and no breakaway, then resumed. The kernel refuses commit past the cap; the limit notification kills the tree; the supervisor's death closes the job and kills the tree. | `job-peak-commit-charge`: the kernel's high-water mark of the tree's commit charge. At a memory kill it includes the request the kernel refused, so it can sit above the cap by that request; granted commit never exceeds the cap. |
| Linux | `linux-cgroup-v2` | `hard` | A dedicated cgroup v2 (`memory.max`, `memory.swap.max=0`, `memory.oom.group=1`) the child joins before `exec`; teardown writes `cgroup.kill`; a sentinel process kills the cgroup if the supervisor dies. Needs a delegated subtree with the memory controller (for example `systemd-run --user --scope -p Delegate=yes verter-supervise ...`); without one the supervisor refuses. `RLIMIT_AS` is never substituted: it is a per-process address-space limit, not a tree cap. | `cgroup-memory.peak` (kernel 5.19+), else `cgroup-memory.current-sampled-max` |
| macOS | `macos-phys-footprint` | `sampled` | macOS gives an unprivileged process no kernel-enforced tree cap. The child is `posix_spawn`ed suspended into its own process group; the supervisor sums `phys_footprint` over the group and every tracked descendant every `--sample-ms` (default 10 ms), wakes on every fork (kqueue), and kills the tree at the cap less 1/16 headroom. Runs only with `--allow-sampled`. | `sampled-tree-phys-footprint-sum` |
| Other | `unsupported` | none | Refuses to launch. | none |

Fail closed: if containment or telemetry cannot be established, the program
never runs (`launched: false`, exit 125). On macOS the preflight also requires
a cap within physical memory minus the host reserve (`--host-reserve-mb`,
default a quarter of RAM, at least 2 GiB), normal host memory pressure and a
sampling sweep that fits its age budget. Mid-run, lost telemetry, a dead
sentinel, a sweep older than twice the sampling interval (macOS), a descendant
that leaves the process group (macOS) or host memory pressure (macOS) kills
the tree and invalidates the run. Sampling proves no overshoot bound, so a
sampled result reports `overshootBoundBytes: null` and the observed overshoot
instead; a descendant that detaches before the sampler sees it, or a
simultaneous loss of supervisor and sentinel, is outside a sampled backend's
reach.

`VERTER_SUPERVISE_FAULT` (`containment`, `telemetry`, `telemetry-midrun`)
injects a supervisor fault for the tests that prove each fail-closed path; it
is never passed to the child. The crate's integration tests
(`cargo test -p verter_supervise`) run the real supervisor over the
`verter-supervise-fixture` process tree. On Linux the backend-dependent cases
are ignored by default because they need a delegated cgroup; run them with
`--include-ignored` inside one (or as root).

## Server Cleanup

After starting any dev server, preview server, or other long-running process for testing, always terminate it when done — stale servers interfere with subsequent test runs. Capture the PID at spawn and terminate **that** PID. A port is a diagnostic, not proof of ownership: `lsof -t -i:<port>` returns whoever holds the port, which may be your own editor's server or another agent's. Never terminate by image name or pattern (`pkill -f node`, `taskkill /F /IM node.exe`, `Stop-Process -Name`).

```bash
pnpm --filter @verter/playground preview & SERVER_PID=$!   # capture at spawn

kill "$SERVER_PID"                                          # Unix — terminate only what you started
taskkill //F //T //PID "$(cat /proc/$SERVER_PID/winpid)"    # Windows — see both caveats below
```

Two Windows caveats, both established by running the commands, not by reading the flags:

- **`$!` is the MSYS pid, not the Windows pid `taskkill` expects.** Passing `$SERVER_PID` directly prints `ERROR: The process "…" not found`, exits 128, and terminates nothing — hence the `/proc/<pid>/winpid` lookup.
- **`//T` does not reap descendants.** It terminates the named process (exit 0, `SUCCESS: … has been terminated`) while its children survive, so `pnpm`'s child `vite`/`node` can outlive the kill. Confirm the server is really gone (`kill -0 "$SERVER_PID"`, or re-probe the port) instead of trusting the success line, and terminate any survivor by its own recorded PID.
