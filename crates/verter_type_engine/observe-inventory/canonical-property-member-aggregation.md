# Canonical-property member aggregation inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `OrderedMemberAggregation` (property index keyed by canonical property identity, beside the first-occurrence ordered entries) | Intersection, union-common, macro-union and one-level surface joins, and the macro emit-branch merge, aggregate each property with one index probe per member instead of a scan of the rows met so far | REQUIRED | One surface merge | `src/projection/aggregation.rs` | always |
| `PropertyKeySet` (the `Pick` key selection under canonical property identity) | The walker's `Pick` filter answers each member's membership with one probe | REQUIRED | One `Pick` filter | `src/projection/aggregation.rs` | always |
| Declaration-merge canonical member index, signature and overload-value seen-sets (`merge_declaration_surfaces_core`, `merge_declaration_surfaces`) | First-occurrence deduplication of call, construct and index signatures and of a method's accumulated overload values without rescanning the kept list | REQUIRED | One declaration merge | `src/project_semantic_dispatch/walk.rs` | always |
| `RawCandidatePairing` (raw call candidates indexed by authored occurrence and bucket kind as pairing reaches them) | Overload applicability pairs each candidate with its raw form reading every raw candidate at most once | REQUIRED | One call resolution | `src/project_semantic_dispatch/call_resolve.rs` | always |
| Aggregation key hashing and equality recorded through the existing `note_key_scan_comparison` key-work count | none; the linear key-work growth test only | OPTIONAL | One test thread | `src/projection/aggregation.rs` (count owned by `src/semantic_query.rs`) | `cfg(any(test, feature = "test-support"))` (pre-existing count) |

## verter_semantic_source

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `SelectedValueSignature` (one overload-group member re-borrowed from the statement declaring it) | The locator deref of a `ValueSignature` position lowers one statement instead of the whole overload group | REQUIRED | One locator deref | `src/decl_body_memo.rs` | always |
| `DeclLoweringCounters::decl_statements_lowered`, now also bumped by the transient value re-borrow and the overload-member selection | The overload-group lowering growth test; the provenance snapshot | OPTIONAL (visit history; field and other legacy hooks pre-existing) | Per host; reset only by the provenance facade | `src/decl_lowering.rs` | New transient hooks: `cfg(any(test, feature = "test-support", feature = "semantic-observe"))`; legacy hooks unchanged |
