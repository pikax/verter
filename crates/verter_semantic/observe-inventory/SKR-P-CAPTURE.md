# Shared closure capture summary inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_session_query

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `CaptureSummaries` per-file arenas: frame ranges and depths, nested-frame links, distinct captured bindings, capture occurrences and captured reads (each with its declaring-frame depth and previous same-binding occurrence) | `FunctionProgramEntry::captures` / `captured_reads` / `nested_captures` / `captures_exhaustive` views, read by skeleton construction, nested-function lowering and flow-return scheduling | REQUIRED | Frozen once when `FunctionProgramIndex::from_discovery` seals a parsed file version; shared through one `Arc` by every entry of that index and released with the last of them | `src/function_program/capture_summary.rs` | always |
| `FunctionProgramEntry` capture handle (`Arc` of the file's summary + frame ordinal), replacing the per-entry copied `captures`, `captured_reads` and `nested_captures` slices | The entry's capture views | REQUIRED | Owned by the sealed entry | `src/function_program.rs` | always |
| `FunctionProgramDiscovery::captures_exhaustive` narrowed to the frame's OWN flag (whether it creates a callable no entry serves); sealing folds it over nested frames | `CaptureSummaries::freeze` | REQUIRED | Discovery record, consumed by sealing | `src/function_program.rs` | always |
| `CaptureSummaryCounts` occupancy (frame, nested-link, binding, capture and read records plus the arenas' backing bytes) read through `FunctionProgramIndex::capture_summary_occupancy` / `capture_summary_identity`: the file's one shared summary, counted once | `HostRetentionSnapshot::capture_summaries` / `capture_summary_files` (via `FileArtifactStore::capture_summary_occupancy`, deduplicated by summary identity over live and retained artifact versions) | REQUIRED-lifetime | Computed on read from the summary's arenas; nothing stored. Counts follow the summary: held while any live or root-retained artifact version, entry or reader holds it | `src/function_program/capture_summary.rs`, `src/function_program.rs` | always |
| `FunctionProgramEntry::capture_summary_counts` / `shares_capture_summary`: per-entry measurement reads of the shared summary | none; tests and measurement only | OPTIONAL | Computed on read; nothing stored | `src/function_program.rs` | `cfg(any(test, feature = "test-support", feature = "semantic-observe"))` |
| `SkeletonNameIndex` on `FunctionBodySkeleton`: name → id map and per-name binding lists | `FunctionBodySkeleton::name_id`, `bindings_named`, `bindings_of_name_in_scope`, `declares_meaning_in_scope` (capture-scope and frame-gate name resolution) | REQUIRED | Built once when the skeleton is assembled; retained and released with the skeleton | `src/flow/skeleton.rs` | always |
| `SkeletonNameIndexOccupancy` (names mapped, binding entries, backing bytes of the map by capacity and the two arrays) read through `SkeletonNameIndex::occupancy` / `storage_identity` | `HostRetentionSnapshot::skeleton_name_indexes` (via `FlowSliceStores::skeleton_name_index_occupancy` over the retained graph bundles, each index counted once) | REQUIRED-lifetime | Computed on read from the index; nothing stored | `src/flow/skeleton.rs` | always |

## verter_semantic_source

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `DeclBodyMemo::retained_function_program_index`: the memo's already-built index, never building one | `FileArtifactStore::capture_summary_occupancy` | REQUIRED-lifetime | Read-only accessor over the memo's existing index cell | `src/decl_body_memo.rs` | always |

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `FunctionFlowGraphStore::skeleton_name_index_occupancy` / `FlowSliceStores::skeleton_name_index_occupancy`: sum of the retained bundles' skeleton name indexes, deduplicated by storage identity | `HostRetentionSnapshot::skeleton_name_indexes` | REQUIRED-lifetime | Computed on read under the store's shard locks; nothing stored | `src/cache_runtime/flow_slice_node.rs` | always |

## verter_session

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `FileArtifactStore::capture_summary_occupancy` and `HostRetentionSnapshot::{capture_summaries, capture_summary_files, skeleton_name_indexes}` | `VerterHost::retention_snapshot`, the host's production retention snapshot | REQUIRED-lifetime | Computed on read from the live and retained artifact versions and graph bundles; nothing stored | `src/file_artifact_store.rs`, `src/types.rs`, `src/host_lifecycle.rs` | always |

## Transient operational state (not retained semantic stores or instrumentation)

These fields are per-operation bookkeeping, not semantic stores, caches or counters; they are classified so the inventory covers every field this task adds or changes.

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `CaptureSummaries::freeze` work state: entry-key positions, child lists, open-frame stack, enclosing-key depth chain, binding-id and last-occurrence maps | The freeze itself | REQUIRED | One `from_discovery` call | `crates/verter_session_query/src/function_program/capture_summary.rs` | always |
| `attach_declaration_closures` per-declaration closure cache, open name table and per-site dedup sets | Hoisted local function closures attached to the sites reading them | REQUIRED | One skeleton preparation | `crates/verter_session_query/src/flow/skeleton.rs` | always |
| Parameter-callable capture dedup set in `resolve_captures` | Exact parameter-list callable capture sets | REQUIRED | One discovery build | `crates/verter_semantic/src/analysis/function_program.rs` | always |
| Extended / considered capture sets in `lower_function_value` | Nested-function value lowering's extended-capture selection | REQUIRED | One nested-function lowering | `crates/verter_semantic_source/src/flow_slice_content.rs` | always |

A function's transitive captures are a filtered range of the file's single
arena: no function's set is materialized and no enclosing function copies a
nested one's, so the summary's records grow with the captures the file
authors, never with how deeply its functions nest.
