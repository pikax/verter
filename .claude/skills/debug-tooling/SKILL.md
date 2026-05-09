---
name: debug-tooling
description: "In-process backtrace watchdog + LLDB attach wrapper + release-dbg profile for diagnosing hangs and slow paths in Verter benches and binaries on Windows / macOS / Linux. Use when something hangs, runs unexpectedly slow, or needs a stack snapshot mid-execution."
---

# Debug Tooling — Watchdog + LLDB Attach

This skill explains the debugging infrastructure landed in commit `6f603f05`
(`chore(debug): in-process backtrace watchdog + lldb attach wrapper`). Use it
when:

- A bench or binary hangs and you do not know where.
- A function runs much slower than expected and you need a stack snapshot
  to see what it is recursing through.
- An external sampling debugger (`samply --record`, `cdb`, `windbg`, `perf`) is
  unavailable on the host (typical on Windows without the Windows Kits).

The infrastructure has three independent pieces. Combine as needed.

## 1. `release-dbg` Cargo profile

`Cargo.toml` defines a `release-dbg` profile that inherits `release` (full
optimisation, LTO, single codegen-unit) but keeps `debug = "line-tables-only"`
and `strip = "none"`. This means attached backtraces resolve to
`crates/<crate>/src/<file>.rs:LINE` for free, with a small (~5%) binary
size increase over `release`.

**Build:**

```bash
cargo build --profile=release-dbg --example audit_real_component_meta -p verter_bench
```

Output lives in `target/release-dbg/examples/`. Use this profile any time you
plan to attach a debugger or capture a backtrace. The regular `release`
profile is unchanged (small, frame-pointer-omitted) so production benchmarks
stay clean.

## 2. In-process watchdog backtrace dumper

`crates/verter_session/src/loop5_instrumentation.rs` exposes a sampling
watchdog that captures `std::backtrace::Backtrace::force_capture()` from the
running thread when triggered. It is wired into the entry of
`shallow_lower_type_expr` (the recursive workhorse of TypeExpr → SemanticNodeId
lowering) and is **inert** when not spawned (single relaxed atomic load per
call site).

### Two modes

- **Sample** — `[WATCHDOG_DUMP]` every `interval_ms` regardless of progress.
  Use this for slow recursive work where the heartbeat advances rapidly but
  the call is stuck deep in one subtree (e.g., walking a 99 MB TypeExpr tree).
- **Stall** — `[WATCHDOG_DUMP]` only after the heartbeat (`watchdog_beat`)
  stops advancing for `stall_threshold_ms`. Use this for true hangs where
  the function never returns.

### Bench harness env vars

`audit_real_component_meta.rs` reads:

| Env var                          | Default     | Meaning                                           |
| -------------------------------- | ----------- | ------------------------------------------------- |
| `VERTER_WATCHDOG_MODE`           | `stall`     | `stall` or `sample`                               |
| `VERTER_WATCHDOG_STALL_MS`       | (no spawn)  | Stall mode threshold; setting this enables stall  |
| `VERTER_WATCHDOG_INTERVAL_MS`    | `1000`      | Sample interval / stall poll period               |

### Recipes

**Sample every 10s while a bench is hanging on ChatMessage cold:**

```bash
VERTER_AUDIT_PROJECT_ROOT=.integration-tests/repos/nuxt-ui-codex-bench \
  VERTER_AUDIT_OUT_DIR=D:/tmp/loop-dbg \
  VERTER_AUDIT_TARGETS=ChatMessage \
  VERTER_AUDIT_PASSES=fresh-cold \
  VERTER_WATCHDOG_MODE=sample \
  VERTER_WATCHDOG_INTERVAL_MS=10000 \
  timeout 120 ./target/release-dbg/examples/audit_real_component_meta.exe \
  > /tmp/audit-watchdog.txt 2>&1
```

**Detect a true hang (no heartbeat for 30s):**

```bash
VERTER_WATCHDOG_MODE=stall VERTER_WATCHDOG_STALL_MS=30000 ./target/release-dbg/...
```

**Read the result:**

```bash
grep -A40 "WATCHDOG_DUMP" /tmp/audit-watchdog.txt | head -100
```

A typical dump looks like:

```
[WATCHDOG_SAMPLE] serial=1 beat=1234567
[WATCHDOG_DUMP] serial=1 label=shallow_lower_type_expr backtrace=
   6: shallow_lower_type_expr  at lower.rs:87        (entry hook)
   7: shallow_lower_type_expr  at lower.rs:1335      (TypeExpr::IndexedAccess.index)
   8: shallow_lower_type_expr  at lower.rs:639       (TypeExpr::Ref arg)
   9: shallow_lower_type_expr  at lower.rs:549       (TypeExpr::Union arm)
  ...
  35: until_stable_full         at field_types.rs:189
  36: until_stable              at field_types.rs:61
  37: (the per-field rescue cascade, since retired)
  ... [up to bench main]
```

The line numbers map to `match` arms in `shallow_lower_type_expr` —
identifying which TypeExpr variant the recursion is processing at each
frame.

### Wiring the watchdog into a new hot path

If you want backtrace dumps from a different function:

```rust
// In your hot-path entry:
crate::loop5_instrumentation::watchdog_beat();
crate::loop5_instrumentation::watchdog_check_and_dump("my_hot_function");
```

Both calls are inert when the watchdog is not spawned, so they cost a single
relaxed atomic load each in production.

## 3. LLDB attach wrapper

`tools/debug/lldb-attach.sh` wraps the bundled LLVM `lldb.exe` for Windows. It
fixes up the `python311.dll` PATH (lldb 22.1 dynamically links it) and emits
`thread backtrace all` + `process detach`.

**Prerequisite (one-time):**

```bash
winget install Python.Python.3.11
```

After that, `python311.dll` lives at
`%LOCALAPPDATA%\Programs\Python\Python311\python311.dll` and the wrapper
finds it automatically.

**Usage:**

```bash
# Attach by process name (case-sensitive)
tools/debug/lldb-attach.sh audit_real_component_meta.exe /tmp/stack.txt

# Attach by PID
tools/debug/lldb-attach.sh 12345 /tmp/stack.txt
```

Output goes to both stdout (live) and the file. The wrapper detaches
automatically so the target process keeps running.

Use this when the watchdog isn't sufficient — for example, when:

- The process you want to inspect isn't a verter bench (no watchdog wired up).
- You want a snapshot at a specific moment without modifying the binary.
- You want **all** thread backtraces, not just the one running
  `shallow_lower_type_expr`.

## When to use which tool

| Symptom                                            | Tool                  |
| -------------------------------------------------- | --------------------- |
| Bench hangs in `shallow_lower_type_expr`           | Watchdog Sample mode  |
| Process is silent for minutes, no progress logs    | Watchdog Stall mode   |
| Want to inspect a process you can't restart        | LLDB attach           |
| Want all-thread state, not just the lowering thread| LLDB attach           |
| Need source line numbers on attached backtraces    | `release-dbg` profile |
| Want full step-through interactive debugging       | LLDB or VS 2022       |

## What to do with a backtrace

A captured stack trace tells you **where** the time / hang lives. The next
step is usually:

1. Map line numbers back to source: each line in the dump is
   `at file.rs:LINE` — open it and read the surrounding code.
2. Check whether the slow function is "fast code walking a giant input"
   or "slow code walking a normal input". The fix differs:
   - Fast code, giant input → the bug is upstream in whoever built the
     input. Trace back through the call chain in the dump.
   - Slow code, normal input → optimise the function itself
     (algorithm, caching, allocation pattern).
3. If the recursion is deep but per-frame is fast, the input shape is
   pathological. Dump it (`format!("{:?}", expr)` at a strategic
   checkpoint, gated on `VERTER_PROGRESS_STREAM`) and check size.

## Caveats

- The watchdog requires the target binary to be linked against the version
  of `verter_session` that includes the watchdog module. Other workspaces
  cannot use it without that dependency.
- `Backtrace::force_capture()` ignores `RUST_BACKTRACE`; you do not need
  to set the env var. Release-mode backtraces work as long as the binary
  has line-tables-only debug info (built via `release-dbg`).
- The watchdog's check-and-dump runs on the **same thread** as the slow
  function. It does not capture other threads. For multi-threaded
  hangs, use LLDB attach.
- LLDB on Windows is sensitive to symbol availability. If frames show
  `<unknown>` instead of source lines, verify the binary was built with
  `release-dbg` and that the `.pdb` file lives next to the `.exe`.
