# NAPI / WASM async boundary future sizes

**Audit verdict (2026-07-22): OUT-OF-SCOPE.** NAPI and WASM boundaries are explicitly outside this audit mandate.

## Symptom

FFI boundaries can pin large futures across the JS runtime. Inventory of
`verter_napi` and `verter_wasm` for that class.

## Mechanism

### `verter_napi`

Only **workspace VFS** methods are `async` (`NapiWorkspace::read_file`,
`file_exists`, `write_file`, `walk`, `resolve_import`, …). Comments state
they are async so the Node event loop is not blocked; the underlying VFS
ops are **synchronous** and run on the libuv thread pool via NAPI-RS async
machinery.

Host semantic APIs (`get_component_meta`, compile, typeinfo, …) remain
**sync** NAPI exports (or use NAPI's own async patterns without Verter-owned
multi-await chains).

There is **no** Verter-owned `FuturesUnordered` / concurrency pool of large
futures on the NAPI side.

### `verter_wasm`

**No** production `async fn` / `.await` surface in the crate (grep-clean).
WASM bindings are sync from JS's perspective for the host API mirror.

## Reproduction

```bash
cargo test -p verter_napi --lib measure_napi_async -- --nocapture --ignored
# wasm: static search only
rg -n "async fn|\.await" crates/verter_wasm --glob "*.rs"
```

## Evidence

Measured (`size_of_val` of constructed method futures, debug):

| future | size |
|---|---|
| `NapiWorkspace::read_file` | **40 B** |
| `NapiWorkspace::file_exists` | **40 B** |
| `NapiWorkspace::write_file` | **64 B** |
| `NapiWorkspace::walk` | **88 B** |
| `NapiWorkspace::resolve_import` | **112 B** |
| `verter_wasm` production async futures | **none** |

These sizes are argument-capture state machines (paths / option lists /
`self`), not nested provider/audit chains. They complete in one poll of
sync VFS work once scheduled on the pool.

| capacity × size | result |
|---|---|
| Verter-owned concurrent collection of NAPI futures | **none** |
| Worst measured single future | 112 B |

## Why deferred

No bloat class comparable to LSP. JS runtimes already own scheduling of
NAPI async work.

## Proposed fix + falsifiable prediction

None for size. If a future host API becomes deeply async across await
points holding analysis snapshots, measure before merge.

**Prediction:** current VFS methods stay &lt;256 B forever unless someone
embeds a full host analysis future into the NAPI async state machine.

## Blast radius

None for the async-bloat class. Leaving VFS methods async is correct for
Node responsiveness.
