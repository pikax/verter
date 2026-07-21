# Async poll sites outside the LSP serve loop

## Symptom

Q1 asks where else a deep async chain is polled **inline** on a thread that
also does other work. The serve-loop case is covered in
`lsp-handler-async-state-machine-sizes.md`. This note ranks the other
production sites so they are not mistaken for the same stack-multiplicity
problem.

## Mechanism

| rank (depth / risk) | site | how polled | multiplies with serve concurrency? |
|---|---|---|---|
| 1 (highest) | tower-lsp `buffer_unordered` → handler chain | serve task, inline | **yes** ×64 |
| 2 | `sync_coordinator::coordinator_loop` | dedicated `tokio::spawn` task | no |
| 3 | `workspace_scanner::scanner_loop` | dedicated spawn | no |
| 4 | `background_init` / drain helpers | spawn / `spawn_blocking` | no |
| 5 | resilient provider actor (`run_actor`) | spawn | no |
| 6 | tsgo/tsserver IPC request futures | boxed `ProviderFuture`, awaited by caller | only inside caller’s future |
| — | `verter_scheduler` | **no async fn** | n/a |

Background loops nest several `async fn`s (`sync_file` → provider open/sync
→ diagnostics publish, etc.) but each runs on **its own task** with a
normal tokio stack. They do not share the serve thread’s
`buffer_unordered` frame and do not multiply by `LSP_MAX_CONCURRENCY`.

Provider transports already box every method (`ProviderFuture`), so the
transport future itself is 16 B at the call boundary; large state, if any,
is inside the box on the heap.

## Reproduction

Code map only for this note (no stack sampler on background tasks in this
pass):

- `crates/verter_lsp/src/main.rs` — concurrency_level + serve
- `crates/verter_lsp/src/sync_coordinator.rs` — `coordinator_loop`
- `crates/verter_lsp/src/workspace_scanner.rs` — `scanner_loop`
- `crates/verter_lsp/src/background_init.rs` — spawn points
- `crates/verter_type_runtime/src/traits.rs` — `ProviderFuture`
- `crates/verter_scheduler` — no `async fn` in production modules

## Evidence

- Serve path future sizes: see inventory doc (up to 38,096 B).
- `TypeProvider::*` outer size: **16 B** (measured).
- Production `join_all` / `FuturesUnordered` holding Verter handlers:
  **none** (tests only).
- Scheduler async surface: **none**.

Background task future sizes were **not** `size_of_val`-measured in this
pass (would require constructing full coordinator deps). Stack peaks on
those tasks were **not** measured. Treat as: same nesting *pattern*
possible, but **not** the same concurrency multiplier as the serve loop.

## Why deferred

Serve-loop class is the proven stack killer in debug. Background sites are
lower priority until a hang or OOM points at them.

## Proposed fix + falsifiable prediction

No change unless a background task shows stack overflow or multi-MiB
futures. If measuring later: construct `sync_file` / `scanner_loop`
futures under test and `size_of_val`; predict they sit in the same
few-to-tens-of-KiB band as handlers, without ×64 multiplication.

## Blast radius

Mis-attributing background work as serve-loop stack would waste a boxing
pass on the wrong task.
