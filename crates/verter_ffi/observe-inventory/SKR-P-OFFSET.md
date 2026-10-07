# Offset-conversion batch inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| Per-batch `OffsetIndex` over one source (diagnostic snapshots, destructured-binding metadata, code-action edits, document-symbol and selector-match spans) | UTF-8 → UTF-16/UTF-32 span conversion at the native and WASM boundaries | REQUIRED-lifetime | One conversion call; borrows the caller's source and is dropped when the batch returns; no resident source cache | `src/convert/{offset,output,actions}.rs`; `verter_napi/src/lib.rs`, `verter_wasm/src/lib.rs` | always |
| `visit_counter`: thread-local source-bytes-visited counter, `reset` / `source_units_visited` readers and recording hooks at every prefix scan and index build | none; tests and measurement builds only | OPTIONAL | Per thread; explicit `reset()` opens a measurement window | `src/convert/offset.rs` | `cfg(any(test, feature = "semantic-observe"))` |

Default builds compile the recording hooks to an empty inline function; there
is no per-operation enabled check.
