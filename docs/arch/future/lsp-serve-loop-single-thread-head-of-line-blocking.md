# Unassigned: every LSP request is polled on one thread, so one slow handler wedges the server

**Status:** RECORDED, NOT FIXED. **This is IN SCOPE** for the current effort (LSP-path
performance) but does not appear in the ratified scope directive's classification table, so
nobody owns it. It is written here so it is not lost, not because it is deferred.

**Not to be confused with** the serve-thread *stack size*, which was fixed separately
(`verter_lsp::SERVE_THREAD_STACK_BYTES`, commit *"run the serve loop on an explicitly sized
thread"*). That change gives the serve thread more stack; it does not make the serve loop
concurrent. This record is about concurrency.

---

## Symptom

On a real multi-package Vue workspace, with the crash fixed, a 40-file / 1160-request
session ends **wedged**: the server process is alive, runtime worker threads keep ticking
their heartbeats, and `$/verter/getStatistics` — a trivial provider-free control request —
times out after 10 s and never answers again.

The last lines before the wedge are:

```
HANDLER_ENTER completion active=1 thread=<serve thread>
completion ENTER <an SFC> at <line>:<col>
… no further serve-thread event ever …
heartbeat TICK active_handlers=1   (on a runtime worker)
heartbeat TICK active_handlers=1   (on a runtime worker)
```

The completion handler entered and never exited. Nothing else could be polled.

Corroborating measurements from the same session:

- **4196 of ~4900** log lines were emitted by the single serve thread; workers carried only
  background and heartbeat work.
- Requests routinely returned **long after their own deadline**: a hover with a 1500 ms
  budget returned at **12 379 ms** — and returned `ok`. `tokio::time::timeout` cannot fire
  while its task is not being polled, and when the task is finally polled the inner future is
  already ready, so the timeout returns success.
- The largest gap between two consecutive trace events on the serve thread was **14 979 ms**
  with no event of any kind in between.
- A goto-definition span of 7603 ms contained **zero** provider round-trips; the provider's
  own transport was healthy throughout (4958 round-trips, p50 38 ms, p95 145 ms, max 1188 ms).

## Mechanism

`tower-lsp-server` 0.23's `Server::serve` drives requests with
`StreamExt::buffer_unordered(concurrency_level)`. `buffer_unordered` is **cooperative
concurrency on one task**: it polls up to N futures from a single `FuturesUnordered`, inside
the `serve` future, which runs on whichever thread called `block_on`. Handler futures are
never `tokio::spawn`ed, so they never migrate to runtime workers.

Captured native backtrace from inside a running handler (abridged; frame numbers as
captured):

```
  6: verter_lsp::server::handler_guard::HandlerGuard::new
  7: verter_lsp::server::nav_features_navigation::handle_goto_definition::async_fn$0
 18: verter_lsp::server::impl$7::goto_definition::async_fn$0        crates/verter_lsp/src/server/mod.rs:1308
 19: tower_lsp_server::jsonrpc::router …
 27: tower_lsp_server::service::pending::execute
 38: futures_util::stream::futures_unordered::poll_next
 40: futures_util::stream::stream::buffer_unordered::poll_next
 51: tower_lsp_server::transport::serve                             tower-lsp-server-0.23.0/src/transport.rs:173
 52: verter_lsp::<server entry future>                              crates/verter_lsp/src/main.rs
 57: tokio::runtime::park::CachedParkThread::block_on
```

Consequences:

1. `LSP_MAX_CONCURRENCY = 64` (`crates/verter_lsp/src/lib.rs`) buys 64 *interleaving slots on
   one thread*, not 64 threads. Raising it cannot help.
2. Any handler that does not yield — a long synchronous section, a blocking call, or simply
   a lot of straight-line work — occupies the serve loop and stalls **every** other request,
   including `$/cancelRequest` and `$/verter/getStatistics`, which is why a wedged server
   also looks dead to a client trying to rescue it.
3. Per-request deadlines cannot rescue it either, for the reason above: the timer belongs to
   a task that is not being polled.
4. One measured synchronous section on the serve thread —
   `recompile_and_refresh_mapper` inside the current-file repair — took **4139 ms**, during
   which nothing else could run.

## Reproduction

Needs a real project (a synthetic fixture is too small to produce a multi-second handler).

1. Start the language server over a multi-package Vue workspace.
2. Open ~10 SFCs in sequence, issuing hover / definition / completion / references at
   authored positions in each.
3. Between files, issue `$/verter/getStatistics` with a 10 s timeout as a liveness check.
4. Observe: a liveness check eventually times out while the process is alive and worker
   heartbeats continue.

Cheaper structural check, no corpus required: log the thread id in `HandlerGuard::new`
(`crates/verter_lsp/src/server/handler_guard.rs`) and confirm every request handler reports
the same thread id as the one that called `block_on`.

## Evidence

Measured on the investigation branch with throwaway instrumentation (`perf/inv-opus`,
commits `1a34847dd` / `31ad96631`, reverted by `dc959bb80`). Numbers reproduced inline above
because the raw artifacts were written to a session-scoped scratch directory and are not
durable.

## Proposed fix and falsifiable prediction

**Proposed fix:** stop polling handler futures inside the transport task. Either
`tokio::spawn` each request future so it runs on the runtime's worker pool, or wrap the
handler body in `tokio::task::block_in_place` so a blocking section releases the poller.
(`block_in_place_if_available` already exists at
`crates/verter_lsp/src/server/handler_guard.rs` and is used in a few places; it is not
applied at the handler boundary.) Spawning changes cancellation and ordering semantics, so it
needs the existing deadline and supersession tests run against it.

**Falsifiable prediction:** with request futures spawned, on the same session,
`liveness.failures` goes from 1 to 0 and `wedged` from true to false; the count of requests
returning after their own deadline falls from 49 to the ~43 genuine deadline cancellations;
and hover's maximum falls from 12 379 ms to below its 1500 ms budget. If hover's maximum
stays far above its budget, the cause is not the poller and this record is wrong.

## Blast radius

- **If fixed:** concurrency and cancellation semantics change. Handlers that today rely on
  being serialised with each other would need review. The per-document singleflight guards
  already in `server/sync_orchestration.rs` exist for exactly that and should absorb most of
  it, but this is not a drive-by change.
- **If left alone:** the server remains one slow handler away from total unresponsiveness,
  and no per-request deadline can prevent it, because the deadline is enforced by the same
  poller that is stuck. Every latency percentile above p50 stays contaminated by queueing.
