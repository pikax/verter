# host_resolve — Tier 0 Step 0.3 god-module split audit

**File:** `crates\verter_session\src\host_resolve.rs`  
**LOC:** 4186  
**Function definitions:** 106  
**Intra-file call edges:** 123  
**Method:** automated extraction via `tmp/audit_extract.py` (regex-based function-and-call enumeration; Tarjan SCC). The plan's §2.1.0 "Default tool" is a `syn`-AST extension to the architecture-guards scanner; that extension is deferred — this document was produced by the lighter-weight extractor in the same time window. The Tier 2 worker assigned to this module should re-derive any sections that look noisy with the syn-AST tool when it lands.

## 1. Intra-file strongly-connected components

### Non-trivial SCCs (size ≥ 2)

**SCC 1 (size 2):** `resolve_named_type_export_route_from_target`, `resolve_named_type_export_route_uncached`

Frontier-engine BFS uncached path (`*_uncached`) calls back into the cached shim (`*_from_target`) when a route hop reuses an already-resolved target. Tier 2 should keep these together in the frontier sub-module.

### Self-recursive functions (size 1)

- `len`
- `materialize_frontier_resolved_type_with_memo`

(Single-function SCCs report self-recursion or method-name collisions where a same-named library method is invoked on a borrowed receiver. The Tier 2 split must check each one against the syn-AST tool when it lands.)

## 2. Recursion-budget edges

| Function | Budget identifier | Line |
|---|---|---|
| `external_type_trace_error_status` | `depth_limit` | 201 |

## 3. Cache-identity edges

| Function | DB | Op | Line |
|---|---|---|---|
| `resolve_component_meta_macro_elements_target` | `ImportedRootDb` | read | 1317 |
| `ensure_route_owned_shallow_entry` | `IndexedReadyDb` | read | 2260 |
| `ensure_shallow_state` | `IndexedReadyDb` | read | 4134 |

## 4. Public-surface edges

`pub fn` count: 47.

- `pub(crate) fn from_entry` — line 303 (span 303-316)
- `pub(crate) fn forbid_route_frontier_for_tests` — line 434 (span 434-439)
- `pub(crate) fn forbid_import_route_shadow_for_tests` — line 442 (span 442-447)
- `pub(crate) fn assert_import_route_shadow_allowed` — line 459 (span 459-464)
- `pub(crate) fn route_frontier_forbidden_for_current_thread` — line 467 (span 467-469)
- `pub(crate) fn import_route_shadow_forbidden_for_current_thread` — line 472 (span 472-474)
- `pub(crate) fn assert_import_route_shadow_allowed` — line 481 (span 481-481)
- `pub(crate) fn invalidate_route_owned_shallow_cache` — line 573 (span 573-577)
- `pub(crate) fn snapshot_route_owned_shallow_cache_entries` — line 585 (span 585-593)
- `pub fn expand_relative_candidates` — line 601 (span 601-615)
- `pub(crate) fn authoritative_import_route` — line 617 (span 617-651)
- `pub(crate) fn import_route_target` — line 653 (span 653-660)
- `pub(crate) fn import_route_is_known_miss` — line 662 (span 662-668)
- `pub(crate) fn prefer_type_dependency_target_from_resolution` — line 718 (span 718-751)
- `pub(crate) fn normalize_live_type_dependency_target` — line 753 (span 753-783)
- `pub(crate) fn fallback_relative_type_companion` — line 785 (span 785-795)
- `pub(crate) fn resolve_loaded_dependency_canonical` — line 840 (span 840-885)
- `pub(crate) fn resolve_type_dependency_canonical` — line 887 (span 887-939)
- `pub(crate) fn resolve_type_dependency_canonical_shallow` — line 942 (span 942-991)
- `pub(crate) fn resolve_external_type_from_loaded_files` — line 995 (span 995-1289)
- `pub(crate) fn resolve_component_meta_macro_surface` — line 1445 (span 1445-1484)
- `pub(crate) fn resolve_component_meta_macro_elements` — line 1486 (span 1486-1504)
- `pub(crate) fn resolve_route_type_edge` — line 1989 (span 1989-2051)
- `pub(crate) fn cached_route_owned_shallow_whole_hash` — line 2117 (span 2117-2128)
- `pub(crate) fn cached_route_owned_eval_state` — line 2133 (span 2133-2147)
- `pub(crate) fn cached_route_owned_snapshot` — line 2152 (span 2152-2158)
- `pub(crate) fn ensure_route_owned_shallow_entry` — line 2178 (span 2178-2336)
- `pub(crate) fn route_owned_entry_is_fresh_for_test` — line 2377 (span 2377-2383)
- `pub(crate) fn route_owned_shallow_state` — line 2425 (span 2425-2431)
- `pub(crate) fn build_named_type_export_route_entry` — line 2496 (span 2496-2530)
- `pub(crate) fn resolve_named_type_export_target_shallow` — line 2564 (span 2564-2579)
- `pub(crate) fn resolve_prepared_decl_target` — line 2609 (span 2609-2626)
- `pub(crate) fn resolve_decl_in_scope_with_reexport_chain` — line 2650 (span 2650-2682)
- `pub(crate) fn resolve_named_type_export_target` — line 2684 (span 2684-2700)
- `pub(crate) fn read_dep_source_for_type_resolution` — line 2708 (span 2708-2751)
- `pub fn resolve` — line 2794 (span 2794-2829)
- `pub fn ensure_compiled` — line 2919 (span 2919-2973)
- `pub(crate) fn compile_slot_is_warm` — line 2993 (span 2993-3033)
- `pub fn get_virtual_file` — line 3041 (span 3041-3367)
- `pub fn list_virtual_files` — line 3370 (span 3370-3372)
- `pub fn get_ide` — line 3379 (span 3379-3397)
- `pub fn get_public_api` — line 3407 (span 3407-3409)
- `pub fn get_public_api_with_mode` — line 3419 (span 3419-3540)
- `pub(crate) fn store_latest_diagnostics` — line 3543 (span 3543-3553)
- `pub(crate) fn compile_entry` — line 3557 (span 3557-3861)
- `pub(crate) fn template_converter_inputs` — line 3865 (span 3865-3912)
- `pub(crate) fn extract_vue_script_content` — line 3919 (span 3919-3933)

## 5. Cross-file shared-cache edges

| Target | Function references | Sample line |
|---|---|---|
| `IndexedReadyDb` | 2 | `ensure_route_owned_shallow_entry` (line 2260) |

## 6. Tier 2 split sketch

**Tier 2 W5c candidate split** — 4 sub-modules. This is a SUGGESTION; the W5* worker assigned to this module is free to deviate.

### `external_type_frontier.rs`

BFS frontier engine, frontier-local layer cache, route-vs-target distinction, the `resolve_named_type_export_route_*` recursion (the file's main SCC), and frontier trace plumbing.

### `named_type_cache.rs`

Host-cached named-type adapter that talks to `SemanticGraphStore::get_resolved_named_type`. Plus the per-canonical invalidation surface.

### `dependency_collection.rs`

Import-graph traversal, owner direct-import resolution, and the `materialize_*` shim that publishes into the project store.

### `snapshot_helpers.rs`

Small `len`/`is_empty` lookups, request-stat aggregators, and trace-flag readers — leaf utilities that should not pull the resolver into a workspace.
