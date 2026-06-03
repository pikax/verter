# Stage 4b Deletion Scope — Materializer + Walker Cluster

## PART 1: DEAD MATERIALIZER FUNCTIONS (in macro_shapes.rs)

### All definitions are in: `crates/verter_session/src/meta_resolve/materialize/macro_shapes.rs`

**Function: `typeinfo_macro_dtos`**
- Definition: line 42–76 (0-indexed: 41–75)
- References (non-test):
  - `define_props_fields_fast_path_allowed` (line 1149)
  - `synthesize_define_props_shape_from_known_surface_with_authority` (line 1274)
  - `synthesize_define_emits_shape_from_known_surface` (line 1581)
  - `synthesize_define_slots_shape_from_known_surface` (line 1689)
- Status: DELETABLE (only called by other materializer functions that are being deleted)

**Function: `produce_macro_object_shapes` (wrapper)**
- Definition: line 233–263
- References:
  - Production: `meta_resolve_tests.rs` (test file) — 12+ test functions
  - Re-exported in `meta_resolve/materialize/mod.rs` line 47 via `#[cfg(test)]`
  - Re-exported in `meta_resolve.rs` line 145 via `#[cfg(test)]`
- Status: TEST-ONLY (zero production callers; all refs are test)

**Function: `produce_macro_object_shapes_for_purpose` (main impl)**
- Definition: line 265–1086
- References:
  - Internal call from `produce_macro_object_shapes` (line 252)
  - Internal calls to `produce_one_macro_object_shape` (lines 339, 387, 521, 614, 658, 844)
  - Internal calls to `produce_one_macro_object_shape_for_slots` (lines 977, 1024)
  - Internal calls to materializer shape functions (lines 429, 478, 768, 899)
- Status: DELETABLE (only called by `produce_macro_object_shapes`, which is test-only)

**Function: `produce_one_macro_object_shape`**
- Definition: line 2118–2387
- References:
  - Called from `produce_macro_object_shapes_for_purpose` (6 sites)
  - Called from `meta_resolve_tests.rs` (4 test functions)
  - Internal call to `project_named_ref_prepared_surface_shape` (line 2147)
- Status: DELETABLE (called only by test-only parent + test files)

**Function: `produce_one_macro_object_shape_for_slots`**
- Definition: line 2692–2860
- References:
  - Called from `produce_macro_object_shapes_for_purpose` (2 sites: lines 977, 1024)
  - No test file references
- Status: DELETABLE (called only from deleted function)

**Function: `project_named_ref_prepared_surface_shape`**
- Definition: line 2423–2481
- References:
  - Called from `produce_one_macro_object_shape` (line 2147) ONLY
- Status: DELETABLE (zero other callers)

**Function: `synthesize_define_props_shape_from_known_surface_with_authority`**
- Definition: line 1243–1397
- References:
  - Called from `produce_macro_object_shapes_for_purpose` (lines 429, 478)
  - Called from `meta_resolve_tests.rs` test functions (2 tests)
  - Re-exported in `meta_resolve/materialize/mod.rs` line 49 via `#[cfg(test)]`
  - Re-exported in `meta_resolve.rs` line 147 via `#[cfg(test)]`
- Status: DELETABLE (called only from deleted function + test files)

**Function: `synthesize_define_emits_shape_from_known_surface`**
- Definition: line 1524–1669
- References:
  - Called from `produce_macro_object_shapes_for_purpose` (line 768) ONLY
- Status: DELETABLE (called only from deleted function)

**Function: `synthesize_define_slots_shape_from_known_surface`**
- Definition: line 1671–1734
- References:
  - Called from `produce_macro_object_shapes_for_purpose` (line 899) ONLY
- Status: DELETABLE (called only from deleted function)

---

## PART 2: WALKER CLUSTER FILES & SYMBOLS

### Files under: `crates/verter_session/src/resolver_core/component_meta_query_engine/`

#### File: `prepared_surface.rs`
- **Walker-specific symbols (DELETE):**
  - `cached_prepared_root_surface` (pub(crate) method, line 45–74): Called by `project_prepared_type_surface_shape_via_host_threaded` (production! dispatch_helpers.rs:1144)
  - `project_prepared_root_surface` (pub(super) method, line 76–88): Called by `cached_prepared_root_surface` only
  - `project_prepared_root_surface_inner` (priv method, ~90-700): Internal to walker
  - `project_prepared_requested_member_from_symbol` (pub(crate) method, line 697–889): Walker entrypoint
  - `project_prepared_requested_member_from_expr` (priv method, line 957–1181): Walker internal

- **Status**: MOSTLY DELETABLE but `cached_prepared_root_surface` has production caller in `dispatch_helpers.rs` — must check if that function is still active.

#### File: `routed_expr.rs`
- **Walker-specific symbols (DELETE):**
  - `project_routed_expr_surface_expr` (method, line 41–276): Walker routing entrypoint, calls `dispatch_member_for_root_symbol`
  - `project_pick_route_surface_expr_via_members` (method, line 1572–1632): Walker routing, calls `dispatch_member_for_root_symbol`
  - `cached_routed_expr_surface_expr`, `cache_routed_expr_surface_expr`, `cache_pick_members_from_projected_expr`, etc.: All walker-internal

- **Shared dispatch symbols (KEEP):**
  - `project_routed_expr_surface_expr_direct` (method): Dispatch path
  - `project_pick_route_surface_expr_via_routed_expr` (method): Dispatch alternative

#### File: `surface.rs`
- **Dispatch symbols (KEEP):**
  - `projected_surface_from_semantic_node` (fn): Used by dispatch paths
  - `projected_compound_root_surface_via_dispatch` (fn): Dispatch entrypoint
  - `dispatch_route_expr_is_materialized` (fn): Dispatch routing
  - All cache-key builders: `arc_prepared_*_cache_key`, `prepared_substitution_key`
  - `projected_surface_to_type_expr`, `projected_surface_to_expanded_shape`: Dispatch output converters

- **Status**: FULLY KEEP (surface.rs hosts only dispatch + shared utilities, no walker-specific)

#### File: `registry_decl.rs`
- **Dispatch symbols (KEEP):**
  - `resolve_imported_registry_symbol` (method): Dispatch registry
  - `dispatch_decl_anchor`, `dispatch_projected_surface`, `dispatch_projected_member`: Dispatch entrypoints
  - `dispatch_root_instantiated`, `dispatch_projected_keyspace`, `dispatch_routed_expr_surface_expr`: Dispatch operations

- **Status**: FULLY KEEP (all dispatch authority)

#### File: `shallow_preserve.rs`
- **Status**: FULLY KEEP (shallow walking is dispatch path, not walker)

#### File: `route_keys.rs`
- **Status**: FULLY KEEP (route caching for dispatch)

#### File: `mod.rs`
- **Dead function (DELETE):**
  - `dispatch_member_for_root_symbol` (fn, line 1229–1247): Bridge between walker routing + dispatch, called by:
    - `project_routed_expr_surface_expr` (routed_expr.rs:60, line 204)
    - `project_pick_route_surface_expr_via_members` (routed_expr.rs:1605)
  - Status: DELETABLE once `project_routed_expr_surface_expr` + `project_pick_route_surface_expr_via_members` are gone

---

## PART 3: WALKER CACHE FIELDS (in ComponentMetaQueryEngine struct, mod.rs:619-741)

Located in: `crates/verter_session/src/resolver_core/component_meta_query_engine/mod.rs`

These are struct fields on `ComponentMetaQueryEngine`:
- `prepared_surface_cache` (field, line 691–698): DeleteEW
- `prepared_member_cache` (field, line 699–701): DELETE
- `prepared_target_cache` (field, line 702–704): DELETE
- `routed_expr_surface_cache` (field, line 705–712): DELETE

Also check `mod.rs` cache-access methods:
- `cached_prepared_surface`, `cache_prepared_surface_projection` (in routed_expr impl)
- `cached_prepared_requested_member`, `cache_prepared_requested_member` (in routed_expr impl)
- `cached_routed_expr_surface_expr`, `cache_routed_expr_surface_expr` (in routed_expr impl)
- `debug_prepared_surface_cache_len`, `debug_routed_expr_surface_cache_len`, `debug_prepared_target_cache_len` (debug methods)

---

## PART 4: GUARDS & TEST FILES

### File: `crates/verter_session/tests/no_legacy_walker.rs`
- Materializer functions NOT in retired symbols list yet
- Walker functions mostly already retired from production
- Need to add to RETIRED_SYMBOLS after 4b deletion:
  - `produce_macro_object_shapes`
  - `produce_macro_object_shapes_for_purpose`
  - `produce_one_macro_object_shape`
  - `produce_one_macro_object_shape_for_slots`
  - `project_named_ref_prepared_surface_shape`
  - `synthesize_define_props_shape_from_known_surface_with_authority`
  - `synthesize_define_emits_shape_from_known_surface`
  - `synthesize_define_slots_shape_from_known_surface`
  - `typeinfo_macro_dtos`
  - `cached_prepared_root_surface`
  - `project_prepared_root_surface`
  - `project_prepared_requested_member_from_symbol`
  - `project_prepared_requested_member_from_expr`
  - `project_routed_expr_surface_expr`
  - `project_pick_route_surface_expr_via_members`
  - `dispatch_member_for_root_symbol`

### File: `crates/verter_session/tests/architecture_guards.rs`
- Guard: `root_surface_bridges_carry_no_prepared_decl_fallback` (checks that `project_type_surface_*` do NOT call `cached_prepared_root_surface`)
- After 4b: This guard STILL applies but must verify dispatch path has zero walker fallback.
- Other guards naming walker symbols:
  - Lists `cached_prepared_root_surface`, `project_prepared_type_surface_shape_via_host_threaded`, etc. as forbidden in certain modules

### Test Files to Delete/Rewrite:
- `crates/verter_session/src/resolver_core/component_meta_query_engine/prepared_surface_tests.rs` (entire file — walker-specific tests)
- Many test functions in `meta_resolve_tests.rs` calling `produce_macro_object_shapes*` functions (12+ tests)

---

## PART 5: CRITICAL PRODUCTION STILL-ACTIVE SYMBOLS (STATUS CHECK)

**FOUND IN PRODUCTION:**
- `project_prepared_type_surface_shape_via_host_threaded` (dispatch_helpers.rs:1138) calls `cached_prepared_root_surface` (line 1144)
  - This is PRODUCTION code (not `#[cfg(test)]`)
  - Callers: NONE found in grep for non-test files except materialize/macro_shapes.rs (which will be deleted)
  - Likely status: Dead after Stage 4a, but needs verification

- `project_prepared_type_surface_expr_via_host_threaded` (dispatch_helpers.rs:1128) is `#[cfg(test)]`
  - Test-only, calls `cached_prepared_root_surface`

---

## SUMMARY FOR ARCHITECT

**To-delete functions (9 in macro_shapes.rs):**
- All in one file, all zero production callers post-Stage-4a
- Requires: Delete all 9 functions + the #[cfg(test)] re-exports from meta_resolve/materialize/mod.rs + meta_resolve.rs

**To-delete walker cluster (5 files):**
- `prepared_surface.rs`: Most methods (keep nothing, delete entire walker entry+body)
- `routed_expr.rs`: Walker routing methods (keep dispatch alternate paths)
- `mod.rs`: `dispatch_member_for_root_symbol` function (delete)
- ComponentMetaQueryEngine struct: 4 walker cache fields (delete)

**Architecture guard changes:**
- Delete `root_surface_bridges_carry_no_prepared_decl_fallback` guard (now vacuous)
- Add materializer symbols to `no_legacy_walker.rs` RETIRED_SYMBOLS after deletion

**Critical check needed:**
- Verify `project_prepared_type_surface_shape_via_host_threaded` (dispatch_helpers.rs:1138) has NO production callers post-Stage-4a
- If it does, it's a 4a leak that must be fixed before 4b proceeds
