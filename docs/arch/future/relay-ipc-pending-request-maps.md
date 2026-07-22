# Relay / tsgo_api IPC: pending maps and per-request futures

## Symptom

The relay shim and `verter_tsgo_api` keep long-lived processes and
in-flight requests. Suspected capacity × size site: a map of large
futures per outstanding RPC. Measured reality: maps hold **oneshot
senders (8 B)**, caller futures are **tens–hundreds of bytes**, and
long-lived tasks are **pumps** (I/O loops), not retained multi-KiB
handler state machines.

## Mechanism

### Pending request tables

| site | map value | caller future |
|---|---|---|
| `JsonRpcConnection` (`tsgo_api/jsonrpc/connection.rs`) | `oneshot::Sender<Value>` | `request()` awaits receiver; `PendingGuard` drops map entry |
| tsgo IPC (`type_runtime/tsgo/ipc.rs`) | same pattern | inside boxed `ProviderFuture` |
| tsserver IPC | same pattern | inside boxed `ProviderFuture` |
| actor `ClientHandle::request` (`tsgo_api/actor`) | oneshot reply in `ActorRequest` on **mpsc lane** | caller awaits reply; cancel via `select!` |

Dropping the caller future is **abandon-only cancel** (remove pending,
discard late response) — the map never retains the caller's future type.

### Long-lived tasks

- `verter_relay_shim`: `run_relay` / accept loop / `ControlServer::serve` —
  process lifetime, polled on dedicated tokio tasks.
- `LspRelay` pumps (`editor_to_server_pump`, `server_to_editor_pump`,
  writer tasks): stream loops; egress policy is sync classification of
  frames, not large nested handler futures.
- Actor `run` / `serve_one` / `serve_frames`: single-writer wire loop;
  **one** in-flight wire request at a time per actor design (restart to
  preempt).

### Queue capacity

Actor lanes: `mpsc::channel(queue_depth)` of **`ActorRequest`**
(method `String` + payload `Vec<u8>` + oneshot + options) — **not**
`impl Future`. Backpressure is message queue depth, not future size.

## Reproduction

```bash
cargo test -p verter_tsgo_api --lib future_size_measure -- --nocapture --ignored
# plus static review of relay_shim main.rs / tsgo_api actor + jsonrpc
```

Harness: `crates/verter_tsgo_api/src/future_size_measure_tests.rs`.

## Evidence

| future / element | size |
|---|---|
| synthetic `JsonRpcConnection::request` shape (unboxed) | **32 B** |
| synthetic actor `ClientHandle::request` shape (unboxed) | **192 B** |
| `timeout(jsonrpc request shape)` | **144 B** |
| `select!(reply_rx, cancel)` request shape | **280 B** |
| `oneshot::Sender<serde_json::Value>` | **8 B** |
| `oneshot::Receiver<serde_json::Value>` | **8 B** |
| `BoxFuture` slot (connection-source pin) | **16 B** |
| `String` / `Vec<u8>` headers (ActorRequest fields) | 24 B each |

| multiplication | product |
|---|---|
| Full pending map of N senders | **N × 8 B** (+ HashMap overhead) |
| Actor queue of depth D | **D × (ActorRequest headers + payload heap)** — payload is bytes, not futures |
| Unboxed large handler × concurrency | **not present** |

**Largest measured request-shaped future here: 280 B** (select+cancel).
Still ~100× smaller than LSP `LanguageServer::completion` (38,096 B).

`run_relay` itself was **not** fully constructed under test (needs a real
child process + control dir). Its steady state is I/O select loops on
dedicated tasks — same pattern as LSP background loops, without the
serve-thread ×64 multiplier.

## Why deferred

No large unboxed future collection found. Process RSS for the relay is
dominated by the child engine and frame buffers, not async state machines.

## Proposed fix + falsifiable prediction

None for future size. If leak hunting: instrument pending-map **length**
and oneshot drop rates under cancel storms.

**Prediction:** pending-map length tracks in-flight RPCs and returns to 0
after idle; it never stores multi-KiB futures.

## Blast radius

- **Depends:** shared-mode attach, owned tsgo, control plane.
- **If pending entries leaked:** session-long map growth (already guarded
  by Drop).
- **If left alone:** correct design for this class.
