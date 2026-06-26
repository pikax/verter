# Debug Tooling Usage (Verter)

In-process debugging infrastructure landed in commit `6f603f05`
(`chore(debug): in-process backtrace watchdog + lldb attach wrapper`).
Use when a bench / binary hangs or runs much slower than expected.

The full reference lives in `.claude/skills/debug-tooling/SKILL.md` —
trigger it via `/debug-tooling` or by mentioning "watchdog" / "lldb
attach" / "release-dbg" in a hang/perf context.

## Three independent pieces

### 1. `release-dbg` Cargo profile (`Cargo.toml`)

Inherits `release` (full opt + LTO + cu=1) but keeps
`debug = "line-tables-only"` and `strip = "none"`.

```bash
cargo build --profile=release-dbg --example audit_real_component_meta -p verter_bench
```

Output: `target/release-dbg/examples/`. Use this profile any time you
plan to attach a debugger or capture a self-backtrace — frames will
resolve to `crates/<crate>/src/<file>.rs:LINE`. Regular `release`
profile is unchanged.

### 2. In-process watchdog
(`crates/verter_session/src/loop5_instrumentation.rs`)

Sampling watchdog that captures
`std::backtrace::Backtrace::force_capture()` from the running thread
when triggered. Wired into the entry of `shallow_lower_type_expr`.
**Inert** when not spawned (single relaxed atomic load per call site).

Two modes:

- **Sample** — dump every `interval_ms` regardless of progress. Use
  for slow recursive work where heartbeat advances rapidly.
- **Stall** — dump only after heartbeat stops advancing for
  `stall_threshold_ms`. Use for true hangs.

Bench env vars:

| Env var                       | Default    | Meaning                                  |
| ----------------------------- | ---------- | ---------------------------------------- |
| `VERTER_WATCHDOG_MODE`        | `stall`    | `stall` or `sample`                      |
| `VERTER_WATCHDOG_STALL_MS`    | (no spawn) | Stall mode threshold (setting it spawns) |
| `VERTER_WATCHDOG_INTERVAL_MS` | `1000`     | Sample interval / stall poll period      |

Recipe — sample every 10s while ChatMessage cold-path runs:

```bash
VERTER_AUDIT_PROJECT_ROOT=.integration-tests/repos/nuxt-ui-codex-bench \
  VERTER_AUDIT_OUT_DIR=<scratch>/loop-dbg \
  VERTER_AUDIT_TARGETS=ChatMessage \
  VERTER_AUDIT_PASSES=fresh-cold \
  VERTER_WATCHDOG_MODE=sample \
  VERTER_WATCHDOG_INTERVAL_MS=10000 \
  timeout 120 ./target/release-dbg/examples/audit_real_component_meta.exe \
  > /tmp/audit-watchdog.txt 2>&1

grep -A40 "WATCHDOG_DUMP" /tmp/audit-watchdog.txt | head -100
```

Wiring into a different hot path:

```rust
crate::loop5_instrumentation::watchdog_beat();
crate::loop5_instrumentation::watchdog_check_and_dump("my_hot_function");
```

### 3. LLDB attach wrapper (`tools/debug/lldb-attach.sh`)

Wraps the bundled LLVM `lldb.exe` with the necessary `python311.dll`
PATH (lldb 22.1 dynamically links it on Windows) and emits
`thread backtrace all` + `process detach`.

Prerequisite (one-time, already done on this host):

```bash
winget install Python.Python.3.11
```

Usage:

```bash
tools/debug/lldb-attach.sh audit_real_component_meta.exe /tmp/stack.txt
tools/debug/lldb-attach.sh 12345 /tmp/stack.txt
```

Use when:

- The target isn't a verter bench (no watchdog wired up).
- Want a snapshot at a specific moment without modifying the binary.
- Need ALL thread backtraces, not just the one running
  `shallow_lower_type_expr`.

## Tool selection matrix

| Symptom                                            | Tool                  |
| -------------------------------------------------- | --------------------- |
| Bench hangs in `shallow_lower_type_expr`           | Watchdog Sample mode  |
| Process is silent for minutes, no progress logs    | Watchdog Stall mode   |
| Want to inspect a process you can't restart        | LLDB attach           |
| Want all-thread state, not just lowering thread    | LLDB attach           |
| Need source line numbers on backtraces             | `release-dbg` profile |
| Want full step-through interactive debugging       | LLDB / VS 2022        |

## What the watchdog showed (concrete win)

ChatMessage cold-path investigation (loop 11):

- Symptom: bench hung in `shallow_lower_type_expr` for `leading.avatar`.
- Watchdog Sample mode (10s interval) captured a 30-frame backtrace
  showing recursive descent through `TypeExpr::Union` (lower.rs:549),
  `Intersection` (574), `Object` property (604), `IndexedAccess`
  (1052) and others.
- Conclusion: the function was correctly fast; the input was a 99 MB
  TypeExpr. Bug was upstream in slot-binding extraction, not in the
  lowering path.

The watchdog answered in one run what weeks of guess-instrumentation
had not.

## Caveats

- Target must link against `verter_session` to use the watchdog.
- `Backtrace::force_capture()` ignores `RUST_BACKTRACE` — no env var
  needed. Just build with `release-dbg`.
- Watchdog runs on the **same thread** as the slow function. For
  multi-threaded hangs, use LLDB attach.
- LLDB on Windows needs `.pdb` next to `.exe` (automatic with
  `release-dbg`).

## When to update this memory

- A new tool is added (e.g., `cargo flamegraph`, `samply` once it
  supports Windows record).
- The watchdog gets wired into more hot paths (currently only
  `shallow_lower_type_expr`).
- The `python311.dll` requirement changes (lldb upgrade, different
  Python version pinned).
