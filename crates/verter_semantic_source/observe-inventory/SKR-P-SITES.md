# Flow span containment / read index inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_session_query

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `FunctionBodySkeleton::span_index` (`SkeletonSpanIndex`): expression sites, writes and bindings as `(span, id)` in nesting order, plus every resolved site read grouped by runtime variable (canonical local or captured identity) in site-span order | Flow slice lowering's loop dependencies (`SliceLoop::writes` / `SliceLoop::inferred` and their value-site reads) and the loop transfer checks (selected writes, reads after the loop, reads under a call) | REQUIRED | Built once in `prepare_function_body_skeleton`; retained with the prepared skeleton in the function's flow-graph bundle and retired with it | `src/flow/span_index.rs` | always |
| `FrameSpan::nesting_cmp` / `starts_before` / `starts_after_end_of` frame-to-frame comparisons | `SkeletonSpanIndex` construction and range queries | REQUIRED | Stateless | `src/flow/frame_span.rs` | always |
| `SPAN_INDEX_VISITS` thread-local counter and `span_index_visits` reader: index entries the containment and read queries inspect | none; tests and benchmark readers only | OPTIONAL | Per thread, monotonic; readers take deltas | `src/flow/span_index.rs` | `cfg(any(test, feature = "test-support", feature = "semantic-observe"))` |

## verter_semantic_source

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `lowering_probe::LoweringWork::span_index_visits`: the span-index entries one slice lowering inspected, taken as the thread counter's delta across the lowering's probe scope | none; tests only | OPTIONAL | Owned by the memo's `lowering_work`; reset by its reader | `src/flow_slice_content.rs` | `cfg(any(test, feature = "test-support"))` (the existing probe's gate) |

Loop dependency and transfer questions ask the index for what starts inside
the span they name, so a question inspects that span's descendants (plus any
overlapping entry the containment check rejects) instead of the whole frame.
Each answer is ids into the skeleton's own tables: a write's reads are its
value sites' own resolved reads, never a copied transitive summary. The
counter is added to once per containment query and once per read pulled from a
read group.
