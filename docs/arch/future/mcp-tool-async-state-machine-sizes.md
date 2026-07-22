# MCP tool async state-machine sizes

## Symptom

`verter_mcp` exposes dozens of `async fn` tools on `VerterMcpServer`. That
looks like a second deep async surface after the LSP. Measured: the
**outer** tool future is already **16 B**, tool bodies are effectively
**await-free sync host work**, and the only multiplication is
**unbounded per-request task spawn** in `rmcp` — each holding a small boxed
state machine, not a 38 KiB LSP-class handler.

## Mechanism

1. **`#[tool]` macro boxes every method** (`rmcp-macros` 1.7.0):
   rewrites `async fn` →
   `fn … -> Pin<Box<dyn Future + Send + '_>> { Box::pin(async move { body }) }`.
   Direct calls therefore report **16 B** for every measured tool.

2. **Bodies do not await.** Tools call `VerterHost` **sync** APIs
   (`get_analysis`, `get_public_api`, `audit_mcp_tool_call`, compile, …)
   inside the first poll. There is no nested multi-layer audit →
   deadline → provider chain like LSP handlers.

3. **`rmcp` serve loop** (`rmcp` 1.7.0 `service.rs`): on each inbound
   peer request, `spawn_service_task` runs
   `service.handle_request(…).await` on a **separate task**. No
   Verter-owned concurrency cap (unlike `LSP_MAX_CONCURRENCY = 64`).
   Concurrent tools multiply as **N × (task + boxed body)**, with body
   heap in the tens of bytes of capture, not tens of KiB of state machine.

4. **stdio vs HTTP:** both use the same service loop pattern; HTTP adds
   axum connection tasks but does not change tool future layout.

## Reproduction

```bash
cargo test -p verter_mcp --lib future_size_measure -- --nocapture --ignored
```

Harness: `crates/verter_mcp/src/future_size_measure_tests.rs`.
Synthetic path only; host is empty standalone.

## Evidence

| future | size |
|---|---|
| `VerterMcpServer::analyze_file` (method boundary) | **16 B** |
| `VerterMcpServer::get_component_api` | **16 B** |
| `VerterMcpServer::get_framework_surface` | **16 B** |
| `VerterMcpServer::compile_file` | **16 B** |
| `Pin<Box<dyn Future + Send>>` slot | **16 B** |
| synthetic tool body capture (path + `Arc` host, no await) | **40 B** |
| synthetic analyze_file-ish (path + sections + host) | **64 B** |
| synthetic compile_file-ish | **40 B** |

| multiplication | value |
|---|---|
| Verter-owned concurrency cap | **none** |
| Per-request outer slot | 16 B |
| Per-request unboxed capture (order of magnitude) | ~40–64 B |
| N concurrent tools heap (bodies only) | **N × ~64 B** (plus tokio task overhead) |
| N needed to match LSP 64×38 KiB handler heap (~2.3 MiB) | **~36,000** concurrent tools (impractical) |

**Largest MCP-owned future class:** tens of bytes of argument capture on
the heap inside the macro's `Box::pin`, not multi-KiB state machines.

## Why deferred

Not a release stack risk; not in the same class as LSP serve-thread
poll frames. Unbounded spawn is a product/load question for MCP hosts,
not an async-layout defect.

## Proposed fix + falsifiable prediction

Only if MCP is observed under a deliberate request flood:

1. Add a server-side concurrency limit (semaphore) around tool execution.
2. Do **not** re-box tools (already boxed).

**Prediction:** with a cap of `C` concurrent tools, resident tool-future
heap ≈ `C × 64 B` plus task overhead; lowering `C` does not change the
16 B outer layout.

## Blast radius

- **Depends:** every MCP client tool call.
- **If a concurrency cap is too low:** agent workflows serialize.
- **If left alone:** large concurrent MCP storms cost host/CPU more than
  future RAM; semantic work dominates.
