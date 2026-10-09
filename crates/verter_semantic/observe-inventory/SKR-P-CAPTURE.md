# Shared closure capture summary inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_session_query

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `CaptureSummaries` per-file arenas: frame ranges and depths, nested-frame links, distinct captured bindings, capture occurrences and captured reads (each with its declaring-frame depth and previous same-binding occurrence) | `FunctionProgramEntry::captures` / `captured_reads` / `nested_captures` / `captures_exhaustive` views, read by skeleton construction, nested-function lowering and flow-return scheduling | REQUIRED | Frozen once when `FunctionProgramIndex::from_discovery` seals a parsed file version; shared through one `Arc` by every entry of that index and released with the last of them | `src/function_program/capture_summary.rs` | always |
| `FunctionProgramEntry` capture handle (`Arc` of the file's summary + frame ordinal), replacing the per-entry copied `captures`, `captured_reads` and `nested_captures` slices | The entry's capture views | REQUIRED | Owned by the sealed entry | `src/function_program.rs` | always |
| `FunctionProgramDiscovery::captures_exhaustive` narrowed to the frame's OWN flag (whether it creates a callable no entry serves); sealing folds it over nested frames | `CaptureSummaries::freeze` | REQUIRED | Discovery record, consumed by sealing | `src/function_program.rs` | always |
| `CaptureSummaries::resident` charge: the summary's `CaptureSummaryCounts` (frame, nested-link, binding, capture and read records plus the arenas' backing bytes), added to the process's resident capture ledger when the summary is frozen and released by its `Drop`; read through `CaptureSummaryCounts::resident` | `HostRetentionSnapshot::capture_summaries` / `capture_summary_files` | REQUIRED-lifetime | Rides the summary allocation: counted once from its freeze until the last index, artifact version or reader holding it drops it, including after cache eviction, source removal and retired-artifact reclamation; never keeps the summary alive | `src/function_program/capture_summary.rs`, `src/retention/resident.rs` | always |
| `FunctionProgramIndex::capture_summary_occupancy`: what one index's shared summary holds | tests (the expected value of the resident count) | OPTIONAL | Computed on read from the summary's arenas; nothing stored | `src/function_program.rs` | always |
| `FunctionProgramEntry::capture_summary_counts` / `shares_capture_summary`: per-entry measurement reads of the shared summary | none; tests and measurement only | OPTIONAL | Computed on read; nothing stored | `src/function_program.rs` | `cfg(any(test, feature = "test-support", feature = "semantic-observe"))` |
| `SkeletonNameIndex` on `FunctionBodySkeleton`: name → id map and per-name binding lists | `FunctionBodySkeleton::name_id`, `bindings_named`, `bindings_of_name_in_scope`, `declares_meaning_in_scope` (capture-scope and frame-gate name resolution) | REQUIRED | Built once when the skeleton is assembled; retained and released with the skeleton | `src/flow/skeleton.rs` | always |
| `SkeletonNameIndex` storage charges: the name map (`NameIds`, names and map capacity bytes, re-counted when a name is interned) and the binding tables (`NameTables`, binding entries and array bytes), each added to the process's resident name-index ledger when built and released by its `Drop`; read through `SkeletonNameIndexOccupancy::resident` | `HostRetentionSnapshot::skeleton_name_indexes` | REQUIRED-lifetime | Rides each backing allocation: counted once from its build until the last skeleton, graph bundle or reader sharing it drops it; never keeps the storage alive | `src/flow/skeleton.rs`, `src/retention/resident.rs` | always |
| `SkeletonNameIndex::occupancy`: what one index holds | tests (the expected value of the resident count) | OPTIONAL | Computed on read from the index; nothing stored | `src/flow/skeleton.rs` | always |
| `retention::resident` ledgers: one set of atomic totals per counted storage kind | `CaptureSummaryCounts::resident`, `SkeletonNameIndexOccupancy::resident` | REQUIRED-lifetime | Process-wide; each total moves only with a charged allocation's build and drop | `src/retention/resident.rs` | always |

## verter_semantic_source

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `DeclBodyMemo::retained_function_program_index`: the memo's already-built index, never building one | tests (a reader held past the file's release) | OPTIONAL | Read-only accessor over the memo's existing index cell | `src/decl_body_memo.rs` | `cfg(any(test, feature = "test-support"))` |

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `FunctionFlowGraphStore::retained_bundles_for_test`: the retained graph bundles, as readers a test holds past their eviction | tests only | OPTIONAL | Clones of the store's bundle handles | `src/cache_runtime/flow_slice_node.rs` | `cfg(any(test, feature = "test-support"))` |

## verter_session

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `HostRetentionSnapshot::{capture_summaries, capture_summary_files, skeleton_name_indexes}` | `VerterHost::retention_snapshot`, the host's production retention snapshot | REQUIRED-lifetime | Read from the process's resident ledgers; nothing stored | `src/types.rs`, `src/host_lifecycle.rs` | always |

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
