# Session host + semantic engine async surface — negative finding

## Symptom

The semantic type engine and host operations are a large concurrent surface.
The async-bloat class asks whether they contribute multi-tens-of-KiB futures
or capacity × size multiplication. They do **not** — concurrency there is
**thread / singleflight / scheduler-job** based, not Rust `async` futures.

## Mechanism

### `verter_session` production async

| surface | async? | notes |
|---|---|---|
| `VerterHost` public ops (meta, analyze, compile, upsert, …) | **sync** | block / wait_or_drive on scheduler |
| Cache singleflight / cold dedup | **threads** | `thread::spawn` / scope only in tests; production joiners are sync |
| Batch APIs | **HostCpuPool** | sync pool, not async |
| `typeinfo/oracle_core/gen.rs` (`generate_snapshot`, `drive_hover`) | **async** | offline oracle generation only — not the live host/LSP path |

### `verter_semantic` / `ProjectSemanticDispatch` / `SemanticGraphStore`

No production `async fn`. Names like `reduce_awaited` implement **TypeScript
`await` / Promise unwrapping** on the typed IR, not Rust futures. Query
execution is synchronous recursive reduction with budgets — a separate
stack/recursion class already covered by peer depth-budget docs.

### `verter_compiler`, `verter_workspace`, `verter_bench`, `verter_ffi`

No production Rust async request surfaces relevant to this class
(`verter_workspace` may spawn Node for trusted vite config; that is a
process, not a retained future collection).

## Reproduction

```text
rg -n "async fn" crates/verter_session/src --glob "*.rs" --glob "!*test*"
# production hits: typeinfo/oracle_core/gen.rs only

rg -n "async fn|\.await" crates/verter_semantic/src --glob "*.rs"
# no Rust async; Promise-await reducers are sync

rg -n "async fn" crates/verter_compiler/src crates/verter_ffi/src \
  crates/verter_workspace/src crates/verter_bench --glob "*.rs"
# no production async request handlers
```

## Evidence

| area | largest production async future | polled how |
|---|---|---|
| Live host / dispatch / graph store | **none** | n/a |
| Oracle gen (`generate_snapshot`) | not `size_of_val`-measured (offline tool path; not serve/LSP) | dedicated gen runtime when used |
| Capacity × size on session queues | **n/a** (no future queues) | |

Semantic-engine **sync** stack depth is out of scope for this async class
(see connected-depth / recursion peer docs). This document only answers
the Rust-async question.

## Why deferred

Live path has nothing to shrink. Oracle gen is offline tooling.

## Proposed fix + falsifiable prediction

Do not introduce host-path `async fn` chains without measuring
`size_of_val` and the collection that retains them. Prediction: keeping
host ops sync + scheduler-backed is what keeps this surface out of the
LSP serve-thread stack class.

## Blast radius

None. Mis-attributing semantic recursion to async bloat would send a fix
to the wrong layer.
