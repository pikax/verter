# semantic_query_memo — Tier 0 Step 0.3 god-module split audit

**File:** `crates\verter_session\src\semantic_query_memo.rs`  
**LOC:** 5765  
**Function definitions:** 181  
**Intra-file call edges:** 516  
**Method:** automated extraction via `tmp/audit_extract.py` (regex-based function-and-call enumeration; Tarjan SCC). The plan's §2.1.0 "Default tool" is a `syn`-AST extension to the architecture-guards scanner; that extension is deferred — this document was produced by the lighter-weight extractor in the same time window. The Tier 2 worker assigned to this module should re-derive any sections that look noisy with the syn-AST tool when it lands.

## 1. Intra-file strongly-connected components

### Non-trivial SCCs (size ≥ 2)

**SCC 1 (size 3):** `default`, `new`, `with_cap`

`new` calls `with_cap` for the default capacity; `with_cap` cycles back into `default` for sub-buffers; `default` re-enters via `Default::default()` elsewhere in the file. This is a constructor-ladder cycle and is benign — Tier 2 may keep these together in `intern.rs`.

**SCC 2 (size 2):** `push`, `push_impl`

Public `push` delegates to private `push_impl`; `push_impl` re-enters `push` for resize-and-retry. Standard collection-grow loop.

### Self-recursive functions (size 1)

- `get`
- `len`
- `drop`
- `origins_of_kind`
- `origins`
- `node_data`
- `deref_mut`
- `deref`

(Single-function SCCs report self-recursion or method-name collisions where a same-named library method is invoked on a borrowed receiver. The Tier 2 split must check each one against the syn-AST tool when it lands.)

## 2. Recursion-budget edges

| Function | Budget identifier | Line |
|---|---|---|
| `default` | `budget_fallback_count` | 925 |
| `stats_snapshot` | `budget_fallback_count` | 1989 |
| `stats_snapshot` | `budget_fallback_count` | 1989 |
| `record_budget_fallback` | `budget_fallback_count` | 2030 |
| `record_path_length_and_projection_depth_drive_percentiles` | `projection_depth` | 5026 |

## 3. Cache-identity edges

No `*Db` cache reads or writes detected in this file.

## 4. Public-surface edges

`pub fn` count: 53.

- `pub fn new` — line 1210 (span 1210-1212)
- `pub fn derivation_signature_pool_size` — line 1220 (span 1220-1222)
- `pub fn with_provenance` — line 1259 (span 1259-1264)
- `pub fn intern_node` — line 1278 (span 1278-1280)
- `pub fn intern_node_with_scope` — line 1292 (span 1292-1298)
- `pub fn intern_preserving_scope` — line 1317 (span 1317-1327)
- `pub fn intern_preserving_scope_call_count` — line 1335 (span 1335-1339)
- `pub fn node_scope` — line 1356 (span 1356-1358)
- `pub fn node_data` — line 1363 (span 1363-1365)
- `pub fn node_count` — line 1369 (span 1369-1371)
- `pub fn memo_entry_count` — line 1377 (span 1377-1383)
- `pub fn canonical_to_entries_count` — line 1390 (span 1390-1395)
- `pub fn invalidate_canonical` — line 1426 (span 1426-1560)
- `pub fn invalidate_all` — line 1566 (span 1566-1571)
- `pub fn insert_resolved_named_type` — line 1578 (span 1578-1586)
- `pub fn get_resolved_named_type` — line 1594 (span 1594-1603)
- `pub fn resolved_named_type_node_id` — line 1612 (span 1612-1617)
- `pub fn clear_resolved_named_types` — line 1625 (span 1625-1627)
- `pub fn invalidate_resolved_named_types_for_canonical` — line 1634 (span 1634-1645)
- `pub fn resolved_named_type_count` — line 1650 (span 1650-1652)
- `pub fn get_relation` — line 1664 (span 1664-1672)
- `pub fn insert_relation` — line 1677 (span 1677-1685)
- `pub fn relation_memo_count` — line 1689 (span 1689-1691)
- `pub fn clear_relation_memo` — line 1696 (span 1696-1698)
- `pub fn record_origin_edge` — line 1732 (span 1732-1850)
- `pub fn origins` — line 1857 (span 1857-1860)
- `pub fn origins_of_kind` — line 1864 (span 1864-1867)
- `pub fn walk_origin_chain` — line 1874 (span 1874-1888)
- `pub fn origin_edge_count` — line 1893 (span 1893-1895)
- `pub fn export_all_origin_edges` — line 1898 (span 1898-1900)
- `pub fn origins_with_fence` — line 1915 (span 1915-1927)
- `pub fn stats_snapshot` — line 1938 (span 1938-2000)
- `pub fn record_instantiate` — line 2005 (span 2005-2007)
- `pub fn record_conditional_decided` — line 2008 (span 2008-2012)
- `pub fn record_conditional_deferred` — line 2013 (span 2013-2017)
- `pub fn record_branch_selection_true` — line 2018 (span 2018-2022)
- `pub fn record_branch_selection_false` — line 2023 (span 2023-2027)
- `pub fn record_budget_fallback` — line 2028 (span 2028-2032)
- `pub fn record_path_length` — line 2035 (span 2035-2037)
- `pub fn record_projection_depth` — line 2041 (span 2041-2043)
- `pub fn record_decl_subexpression_lowering` — line 2044 (span 2044-2048)
- `pub fn record_relation_check` — line 2049 (span 2049-2053)
- `pub fn get` — line 2062 (span 2062-2071)
- `pub fn execute_cooperative` — line 2095 (span 2095-2348)
- `pub(crate) fn warm_publish_one_if_absent` — line 2451 (span 2451-2491)
- `pub(crate) fn publish_warm_if_absent` — line 2528 (span 2528-2545)
- `pub(crate) fn test_trigger_inflight_abort` — line 2577 (span 2577-2595)
- `pub fn new` — line 2684 (span 2684-2686)
- `pub fn intern` — line 2695 (span 2695-2742)
- `pub fn intern_canonical` — line 2746 (span 2746-2756)
- `pub fn sweep` — line 2766 (span 2766-2771)
- `pub fn bucket_count` — line 2777 (span 2777-2779)
- `pub fn live_signature_count` — line 2784 (span 2784-2795)

## 5. Cross-file shared-cache edges

| Target | Function references | Sample line |
|---|---|---|
| `SemanticGraphStore` | 69 | `fmt` (line 545) |

## 6. Tier 2 split sketch

**Tier 2 W5a candidate split** — 4 sub-modules. This is a SUGGESTION; the W5* worker assigned to this module is free to deviate.

### `intern.rs`

Identity tables, scope-preserving interning, NodeId/EdgeId arenas, and `node_data` / `node_scope` / `node_count` / `intern_node*` accessors.

### `memo.rs`

`FamilyKey` / `FamilySlots` warm memo (`entries`), inflight tracking, family-level backfill on completion, and the `memo_entry_count` / `clear_resolved_named_types` lifecycle.

### `execute.rs`

Cooperative-admission cold-path runner (`execute_cooperative*`), same-path recursion sentinel, mid-flight-supersede dispatch, and the dispatch-queue fairness logic.

### `stats.rs`

`AtomicSemanticGraphStats`, percentile reservoirs, `stats_snapshot`, telemetry histograms, and the public `SemanticGraphStats` struct.
