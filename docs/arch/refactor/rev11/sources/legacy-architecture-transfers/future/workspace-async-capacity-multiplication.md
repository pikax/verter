# Workspace-wide capacity × future-size multiplication (Q3)

**Audit verdict (2026-07-22): NEGATIVE.** No second workspace-wide capacity-times-large-future multiplier exists outside the already bounded LSP request inventory.

## Symptom

The LSP pass found the only serious multiplier in `verter_lsp`:
`buffer_unordered(64)` × boxed handler heap
(**64 × 38,096 B ≈ 2.32 MiB** for completion). This document answers Q3
for **every other crate** and ranks the workspace.

## Mechanism

A capacity × size problem requires **all three**:

1. a collection that retains futures (or boxed future payloads) until complete,
2. a non-trivial capacity (or unbounded growth),
3. a large per-element state machine.

Outside the LSP serve loop, Verter mostly fails (1) or (3).

## Reproduction

Cross-crate inventory + measured harnesses:

```bash
# LSP (existing)
cargo test -p verter_lsp --lib future_size_measure -- --nocapture --ignored
# type_runtime / mcp / tsgo_api / napi (this pass)
cargo test -p verter_type_runtime --lib future_size_measure -- --nocapture --ignored
cargo test -p verter_mcp --lib future_size_measure -- --nocapture --ignored
cargo test -p verter_tsgo_api --lib future_size_measure -- --nocapture --ignored
cargo test -p verter_napi --lib measure_napi_async -- --nocapture --ignored
# static: scheduler / session / wasm / compiler
rg -n "FuturesUnordered|join_all|buffer_unordered|select_all" crates --glob "*.rs"
```

## Evidence

### Ranked multipliers (production)

| rank | site | capacity | per-element | product | notes |
|---|---|---|---|---|---|
| **1** | LSP `buffer_unordered` | **64** | **~38 KiB** boxed handler | **~2.32 MiB** | see `lsp-buffer-unordered-heap-multiplication.md` |
| 2 | MCP `rmcp` spawn-per-request | **unbounded** | **~16 B slot + ~40–64 B body** | **N × ~64 B** | practical N ≪ LSP heap |
| 3 | Completion-detail `JoinSet` | **8** concurrent | **~192 B** task shape (spawned) | **~1.5 KiB** futures | list cap 50 serial waves |
| 4 | IPC pending maps | in-flight count | **8 B** oneshot sender | **N × 8 B** | not futures |
| 5 | tsgo stdin lanes | **1024** frames | frame bytes | channel of **messages** | not futures |
| 6 | Actor mpsc lanes | `queue_depth` | `ActorRequest` | messages | not futures |
| — | Scheduler job queues | pool depth | sync jobs | **n/a** | no async futures |
| — | Session host / semantic | — | — | **n/a** | sync |

### Production `join_all` / unboxed `FuturesUnordered` of large Verter futures

| crate | production hit? |
|---|---|
| `verter_lsp` | **no** (tests only) |
| `verter_type_runtime` | **no** (`JoinSet` of spawned tasks only) |
| `verter_mcp` / `verter_tsgo_api` / `verter_napi` / `verter_scheduler` / `verter_session` | **no** |

### Largest futures outside `verter_lsp` (this pass)

| future | size | polled where |
|---|---|---|
| `select!(reply, cancel)` actor request shape | **280 B** | caller task (tsgo_api) |
| completion-detail resolve task shape | **192 B** | spawned JoinSet task |
| definition-shaped provider hop (unboxed) | **168 B** | heap inside `ProviderFuture` box; polled by LSP handler |
| NAPI `resolve_import` | **112 B** | NAPI async / libuv |
| MCP tool body capture | **~40–64 B** | heap inside `#[tool]` box; spawned rmcp task |
| `TypeProvider::*` outer | **16 B** | nested in LSP/MCP/dx callers |

**Workspace winner remains LSP:** `LanguageServer::completion` **38,096 B**,
multiplied by **64** on the serve path.

## Why deferred

Outside LSP, no capacity × multi-KiB-future product was found that warrants
a structural fix in this investigation pass.

## Proposed fix + falsifiable prediction

1. Treat **LSP buffer_unordered × handler size** as the sole material
   member of this class today (see LSP docs).
2. Optionally bound MCP concurrency if agent storms appear — for **CPU**,
   not future RAM.
3. Do not "box the scheduler" — it has no async futures.

**Prediction:** a workspace heap profile under mixed LSP + MCP load still
attributes multi-MiB request-future residency to the **LSP pending set**,
not to type-runtime pending maps or MCP tool futures.

## Blast radius

Mis-prioritizing MCP/scheduler "multipliers" would delay the only real
~2.3 MiB concurrent-handler finding (LSP).
