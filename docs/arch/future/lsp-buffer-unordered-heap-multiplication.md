# Serve-loop `buffer_unordered` capacity × future-size multiplication

## Symptom

`tower-lsp-server` admits up to **`LSP_MAX_CONCURRENCY = 64`** in-flight
request futures on one serve task
(`crates/verter_lsp/src/lib.rs`, `main.rs` → `Server::concurrency_level`).
Each pending request retains a full handler state machine for its lifetime.
That is a **memory** finding as well as a stack finding: a concurrent storm
multiplies one large future by the concurrency cap.

## Mechanism

`tower-lsp-server` 0.23.0 `Server::serve`
(`…/tower-lsp-server-0.23.0/src/transport.rs`):

1. Incoming requests become `service.call(req)` futures.
2. They are sent on an mpsc and driven by
   `server_tasks_rx.buffer_unordered(self.max_concurrency)`.
3. `LspService::call` returns
   `BoxFuture<'static, …>` — each slot is a **boxed** future
   (`service.rs`: `Box::pin(async move { … })`).
4. Router layers also `.boxed()` method futures
   (`service/layers.rs`, `jsonrpc/router.rs`).

So:

| quantity | value | meaning |
|---|---|---|
| concurrency cap | **64** | `LSP_MAX_CONCURRENCY` |
| inline slot size | **16 B** | `size_of::<BoxFuture<_>>()` |
| inline collection footprint | **64 × 16 = 1,024 B** | `buffer_unordered` element storage |
| heap per in-flight request | = unboxed handler future size | allocation inside the `Box` |

`FuturesUnordered` / `buffer_unordered` hold the **outer** `BoxFuture`
inline (small). The **large** state lives on the **heap**, one allocation
per in-flight request, retained until the request completes.

Production code has **no** `join_all` / `FuturesUnordered` / `select_all`
holding large unboxed Verter futures. The only `join_all` call sites in
`verter_lsp` are **tests** (`server_tests.rs`). Completion-detail
enrichment in tsgo IPC uses `JoinSet` + semaphore
(`COMPLETION_DETAIL_RESOLVE_CONCURRENCY = 8`, list cap 50) — spawned
tasks, not an unboxed `FuturesUnordered` of large futures.

## Reproduction

```bash
cargo test -p verter_lsp --lib future_size_measure -- --nocapture --ignored
```

Read `LSP_MAX_CONCURRENCY`, `size_of` of `BoxFuture`, and
`size_of_val` of the largest trait method futures (same harness as the
size inventory).

## Evidence

Measured (debug = release for future sizes):

| item | bytes |
|---|---|
| `BoxFuture` slot | 16 |
| capacity × slot (inline) | 1,024 |
| largest trait future (`LanguageServer::completion`) | 38,096 |
| `LanguageServer::rename` | 37,680 |
| `LanguageServer::goto_definition` | 37,168 |
| **worst capacity × handler heap** | **64 × 38,096 = 2,438,144 B (~2.32 MiB)** |
| capacity × rename | 64 × 37,680 = 2,411,520 B (~2.30 MiB) |
| capacity × goto_definition | 64 × 37,168 = 2,378,752 B (~2.27 MiB) |

Using the **peer’s** larger audited-definition size (136,088 B) as an
upper historical bound: **64 × 136,088 = 8,709,632 B (~8.30 MiB)** of
heap if every slot held that shape concurrently.

That heap is **resident while requests are in flight**, not a one-shot
stack frame. It is a plausible contributor to monotonic RSS growth under
request storms (alongside other caches), but this investigation did
**not** re-measure process RSS under load — only capacity × size of the
pending-request set.

## Why deferred

Documentation only. Changing concurrency, boxing policy, or the serve
loop is out of scope for this investigation pass.

## Proposed fix + falsifiable prediction

Options (pick by measurement, do not apply all):

1. **Lower `LSP_MAX_CONCURRENCY`** if control-request liveness still holds
   with a smaller cap (current 64 exists specifically so control methods
   are not starved — see comment on `LSP_MAX_CONCURRENCY`).
2. **Box only the largest handlers** so the heap shape is explicit and
   uniform; does not reduce heap mass but may simplify stack/debug.
3. **Shrink handler futures** (fewer locals across awaits, split phases)
   — reduces capacity × size linearly.

**Prediction:** under a synthetic 64-wide storm of the same method,
process heap attributed to request futures ≈ `N × size_of(handler)` for
`N ≤ 64`. After shrinking the completion future by factor `k`, that
band shrinks by `k`. RSS delta under storm should track within noise of
other caches.

## Blast radius

- **Depends:** every concurrent LSP request; hang detection / deadlines
  interact with how long a slot stays full.
- **If fixed wrongly (cap too low):** reintroduces stdin starvation /
  control-request death under wedged semantic handlers (already fixed
  once by raising the cap to 64 + always-on deadlines).
- **If left alone:** ~2.3 MiB peak for a full completion storm is
  modest for a language server, but multiplies with other growth; not
  free.
