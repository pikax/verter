# Type-runtime provider / IPC async future sizes

## Symptom

Every LSP handler that talks to TypeScript ends in `verter_type_runtime`
(`TypeProvider`, tsgo/tsserver IPC, resilient actor, deadline helpers). The
LSP pass measured the **outer** hop as **16 B** (`ProviderFuture`). This
document measures the rest of that crate: wrapper layers, transport-shaped
state, pending maps, and the only concurrent collection that multiplies work
(`JoinSet` for completion-detail enrichment).

## Mechanism

1. **Trait boundary boxes everything.**
   `ProviderFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, _>> + Send + 'a>>`
   (`crates/verter_type_runtime/src/traits.rs`). Every method returns
   `Box::pin(async move { … })`. Callers (LSP, MCP, dx-baseline) always see
   a **16 B** outer future.

2. **Pending maps hold oneshots, not futures.**
   tsgo IPC (`tsgo/ipc.rs`) and tsserver IPC keep
   `HashMap<i64, oneshot::Sender<serde_json::Value>>`. In-flight request
   **futures** live on the caller's stack/task (the LSP handler future);
   the map only retains an **8 B** sender handle per id. Drop of the
   caller's future removes the entry and cancels (`PendingRequest`).

3. **Lanes are channel capacity, not future slots.**
   `DEFAULT_LANE_CAPACITY = 1024` bounds stdin message frames
   (Interactive / Normal / Background), not unboxed handler futures.

4. **Completion-detail enrichment** is the only multi-future fan-out:
   `JoinSet` + semaphore `COMPLETION_DETAIL_RESOLVE_CONCURRENCY = 8`,
   list cap `MAX_COMPLETION_DETAIL_ENRICH = 50`
   (`tsgo/ipc.rs`). Tasks are **spawned** (heap), not stored unboxed in
   a `FuturesUnordered` of large futures.

5. **Resilient actor** (`resilient.rs`) tracks in-flight **query
   fingerprints** (multiset counts), not futures. Commands use oneshot
   acks.

## Reproduction

```bash
cargo test -p verter_type_runtime --lib future_size_measure -- --nocapture --ignored
cargo test -p verter_type_runtime --lib future_size_measure --release -- --nocapture --ignored
```

Harness: `crates/verter_type_runtime/src/future_size_measure_tests.rs`.

## Evidence

Measured debug profile (future **layout** sizes; same technique as the LSP
pass — `size_of_val` on constructed futures, drop without polling).

### Outer boundary (what callers hold)

| future | size |
|---|---|
| `TypeProvider::get_definition` / `get_hover` / `get_completions` / `get_diagnostics` | **16 B** |
| `size_of::<ProviderFuture<'_, ()>>()` | **16 B** |
| `Box::pin(timeout+oneshot)` | **16 B** |

### Wrapper layers

| future | size |
|---|---|
| tiny `async { 7u8 }` | 1 B |
| `tokio::time::timeout(tiny)` | 120 B |
| `with_deadline(tiny)` | 56 B |
| `with_deadline(timeout(tiny))` | 288 B |
| `with_deadline_at(tiny)` | 56 B |

### Unboxed transport-shaped state (what the box *would* contain)

| future | size |
|---|---|
| synthetic oneshot wait | 16 B |
| synthetic timeout+oneshot | 128 B |
| **get_definition-shaped hop** (path+uri+Arcs+lock+timeout request) | **168 B** |
| completion-detail resolve task shape | 192 B |

### Capacity × size (this crate)

| collection | capacity | element | product |
|---|---|---|---|
| `buffer_unordered` of large handlers | — | — | **not present** |
| Pending map (full of senders) | 1024 (lane scale) | 8 B oneshot sender | **8,192 B (~8 KiB)** headers only |
| JoinSet detail tasks (inline handles) | 8 | 8 B `JoinHandle` | **64 B** inline |
| Counterfactual 8 × unboxed detail task | 8 | 192 B | **1,536 B** (tasks actually spawned) |
| Counterfactual 64 × unboxed definition hop | 64 | 168 B | **10,752 B (~10.5 KiB)** — not how production holds them |

**Largest unboxed shape found here: 192 B** (detail-resolve task) /
**168 B** (definition-shaped hop). Orders of magnitude below LSP trait
methods (38,096 B).

## Why deferred

Documentation only. Boxing is already paid at the trait boundary; no
release stack evidence of a type-runtime-local overflow.

## Proposed fix + falsifiable prediction

No change indicated for size. If hang detection / RSS work reopens this:

1. Profile pending-map **value** heap (JSON responses), not future
   state machines.
2. Keep completion-detail concurrency at 8; raising it multiplies
   engine load more than future RAM.

**Prediction:** under a 50-item completion enrich, peak concurrent resolve
tasks ≤ 8 and each task's future state is &lt;1 KiB; RSS attribution to
those futures stays negligible next to engine process memory.

## Blast radius

- **Depends:** every provider hop from LSP/session consumers.
- **If over-boxed further:** pure noise alloc.
- **If unboxed into a concurrent collection:** would create a new
  capacity × size site (currently avoided).
