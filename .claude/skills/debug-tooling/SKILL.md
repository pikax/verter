---
name: debug-tooling
description: "In-process backtrace watchdog + LLDB attach wrapper + release-dbg profile for diagnosing hangs and slow paths in Verter benches and binaries on Windows / macOS / Linux. Use when something hangs, runs unexpectedly slow, or needs a stack snapshot mid-execution."
---

# Debug Tooling — Watchdog + LLDB Attach

Three independent pieces (combine as needed), from commit `6f603f05` (`chore(debug): in-process backtrace watchdog + lldb attach wrapper`). Use when:

- A bench/binary hangs and you don't know where.
- A function is much slower than expected and you need a stack snapshot.
- An external sampler (`samply --record`, `cdb`, `windbg`, `perf`) is unavailable (typical on Windows without the Windows Kits).

## 1. `release-dbg` Cargo profile

`Cargo.toml` defines `release-dbg`: inherits `release` (full optimisation, LTO, single codegen-unit) but keeps `debug = "line-tables-only"` and `strip = "none"`. Backtraces resolve to `crates/<crate>/src/<file>.rs:LINE` with ~5% binary size increase over `release`. Regular `release` profile is unchanged (small, frame-pointer-omitted) so production benchmarks stay clean.

**Build:**

```bash
cargo build --profile=release-dbg --example audit_real_component_meta -p verter_bench
```

Output: `target/release-dbg/examples/`. Use whenever attaching a debugger or capturing a backtrace.

## 2. In-process watchdog backtrace dumper

`crates/verter_session/src/loop5_instrumentation.rs` — sampling watchdog that captures `std::backtrace::Backtrace::force_capture()` from the running thread. Wired into `shallow_lower_type_expr` (recursive workhorse of TypeExpr → SemanticNodeId lowering). **Inert when not spawned** (single relaxed atomic load per call site).

### Two modes

- **Sample** — `[WATCHDOG_DUMP]` every `interval_ms` regardless of progress. For slow recursive work where the heartbeat advances rapidly but the call is stuck deep in one subtree (e.g., walking a 99 MB TypeExpr tree).
- **Stall** — `[WATCHDOG_DUMP]` only after `watchdog_beat` stops advancing for `stall_threshold_ms`. For true hangs where the function never returns.

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

A typical dump:

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

Line numbers map to `match` arms in `shallow_lower_type_expr`, identifying which TypeExpr variant the recursion is processing at each frame.

### Wiring the watchdog into a new hot path

```rust
// In your hot-path entry:
crate::loop5_instrumentation::watchdog_beat();
crate::loop5_instrumentation::watchdog_check_and_dump("my_hot_function");
```

Both calls are inert when the watchdog is not spawned (single relaxed atomic load each in production).

## 3. LLDB attach wrapper

`tools/debug/lldb-attach.sh` wraps bundled LLVM `lldb.exe` for Windows. Fixes `python311.dll` PATH (lldb 22.1 dynamically links it) and emits `thread backtrace all` + `process detach`.

**Prerequisite (one-time):**

```bash
winget install Python.Python.3.11
```

`python311.dll` lives at `%LOCALAPPDATA%\Programs\Python\Python311\python311.dll`; the wrapper finds it automatically.

**Usage:**

```bash
# Attach by process name (case-sensitive)
tools/debug/lldb-attach.sh audit_real_component_meta.exe /tmp/stack.txt

# Attach by PID
tools/debug/lldb-attach.sh 12345 /tmp/stack.txt
```

Output goes to stdout (live) and the file. Detaches automatically so the target keeps running.

Use when watchdog isn't sufficient:
- Process isn't a verter bench (no watchdog wired up).
- Want a snapshot at a specific moment without modifying the binary.
- Want **all** thread backtraces, not just the one running `shallow_lower_type_expr`.

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

A stack trace tells you **where** the hang/slowness lives:

1. Map line numbers to source: each frame is `at file.rs:LINE` — open and read surrounding code.
2. Determine: "fast code walking giant input" vs "slow code walking normal input":
   - Fast code, giant input → bug is upstream in whoever built the input; trace back through the call chain.
   - Slow code, normal input → optimise the function (algorithm, caching, allocation pattern).
3. If recursion is deep but per-frame is fast, input shape is pathological. Dump it (`format!("{:?}", expr)` at a strategic checkpoint, gated on `VERTER_PROGRESS_STREAM`) and check size.

## Caveats

- Watchdog requires the target binary to be linked against the version of `verter_session` that includes the watchdog module. Other workspaces cannot use it without that dependency.
- `Backtrace::force_capture()` ignores `RUST_BACKTRACE`; no need to set that env var. Release-mode backtraces work as long as the binary has line-tables-only debug info (built via `release-dbg`).
- The watchdog's check-and-dump runs on the **same thread** as the slow function — does not capture other threads. For multi-threaded hangs, use LLDB attach.
- LLDB on Windows is sensitive to symbol availability. If frames show `<unknown>` instead of source lines, verify the binary was built with `release-dbg` and that the `.pdb` file lives next to the `.exe`.
