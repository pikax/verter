# Tier 2 W5c — host_resolve.rs split

**Branch base:** `refactor/legacy-to-graph-dispatch-migration` HEAD `f5a1d10e`
**Plan:** `<scratch>/verter-debt-and-deferred-fixes-plan.md` §4 (Tier 2)
**Authority audit:** `docs/arch/debt-closure/13-god-module-split-audit/host_resolve.md`
**Step:** `2.3`
**Marker:** `phase-tier-2-step-2.3-w5c-complete`
**`prior_known_passed_count`:** `10552`

## Summary

Split `crates/verter_session/src/host_resolve.rs` (4207 LOC) into a
`host_resolve/` directory of 11 sub-modules. The public surface at
`crate::host_resolve::*` is unchanged: every item that pre-split callers
named is preserved through `pub(crate) use` re-exports in the new
`host_resolve/mod.rs`. All sub-modules are under the 4000-LOC budget.

## Layout

| File                                              | LOC  | Responsibility |
|---|---|---|
| `host_resolve/mod.rs`                             |   94 | Module decls + re-exports |
| `host_resolve/frontier_helpers.rs`                |  385 | Type aliases, frontier traces, `RouteOwnedShallowStateSnapshot`, wildcard match scoring, `external_type_debug` |
| `host_resolve/test_guards.rs`                     |   86 | `forbid_*_for_tests` thread-local guards |
| `host_resolve/external_macro_collector.rs`        |   89 | `HostExternalMacroTypeCollector` adapter |
| `host_resolve/dependency_resolution.rs`           |  471 | `impl VerterHost` — import-route + dependency canonical (`authoritative_import_route`, `resolve_loaded_dependency_canonical`, `resolve_type_dependency_canonical`, `cache_import_route_result`, …) |
| `host_resolve/external_type_resolution.rs`        |  599 | `impl VerterHost` — `resolve_external_type_from_loaded_files`, `resolve_component_meta_macro_*`, `lookup_resolved_external_type_cache` / `store_resolved_external_type_cache` |
| `host_resolve/frontier_engine.rs`                 |  791 | `impl VerterHost` — frontier closure, materialise, `resolve_named_type_export_route_*` SCC, `route_shallow_state` |
| `host_resolve/route_owned_shallow.rs`             |  515 | `impl VerterHost` — `ensure_route_owned_shallow_entry`, prepared-decl walking, `read_dep_source_for_type_resolution`, `collect_external_types_from_loaded_files` |
| `host_resolve/virtual_file_pipeline.rs`           | 1112 | `impl VerterHost` — `resolve` / `ensure_compiled` / `get_virtual_file` / `get_ide` / `get_public_api*` / `compile_entry` |
| `host_resolve/vue_script_extract.rs`              |  240 | `template_converter_inputs`, `extract_vue_script_content` and helpers |
| `host_resolve/frontier_adapter.rs`                |  101 | `HostFrontierAdapter` request-scoped bridge |
| **Total**                                         | **4483** | each file < 4000 LOC |

## Authority audit alignment

- The audit's identified intra-file SCC
  (`resolve_named_type_export_route_from_target` ↔
  `resolve_named_type_export_route_uncached`) stays co-located in
  `frontier_engine.rs`, alongside the `route_shallow_state` reader they
  cycle through.
- `ensure_route_owned_shallow_entry` (the `IndexedReadyDb`-keyed
  cache-identity edge in §3 of the audit) lands in
  `route_owned_shallow.rs`.
- The `external_type_trace_error_status` recursion-budget edge is
  preserved in `frontier_helpers.rs`.

The audit's 4-module sketch (frontier / named-type-cache / dependency-collection / snapshot helpers) was extended to 10 sub-modules to also accommodate the virtual-file pipeline, vue-script extraction, and frontier adapter that lived in the same source file but fell outside the audit's resolution-only sketch.

## Public-surface preservation

Re-exported through `host_resolve/mod.rs`:

- `pub(crate) use frontier_adapter::HostFrontierAdapter;`
- `pub(crate) use frontier_helpers::RouteOwnedShallowStateSnapshot;`
- `pub(crate) use vue_script_extract::{extract_vue_script_content, template_converter_inputs};`
- `pub(crate) use test_guards::{assert_import_route_shadow_allowed, forbid_import_route_shadow_for_tests, forbid_route_frontier_for_tests, import_route_shadow_forbidden_for_current_thread, route_frontier_forbidden_for_current_thread, ImportRouteShadowGuard, RouteFrontierGuard};` (cfg(test))
- `pub(crate) use test_guards::assert_import_route_shadow_allowed;` (cfg(not(test)))

For test-only consumers (`host_resolve_tests.rs` references internals via `super::*`):
- `#[cfg(test)] pub(crate) use frontier_helpers::{external_type_frontier_layer_*, external_type_trace_*, ExternalTypeTraceBaseline, FrontierCompanionPlans, FrontierRequestedRoutes, PlannedFrontierCompanion};`

## Tier 1C-α invariants preserved

- `lookup_resolved_external_type_cache` and
  `store_resolved_external_type_cache` (now in
  `external_type_resolution.rs`) still delegate to
  `self.resolved_type_cache()` (= `project_type_store.resolved_type_cache()`).
- The bounded clear-all-at-4096 cap on `ResolvedTypeCacheDb` lives
  inside `project_type_store.rs`. Untouched by this split.

## Visibility lifts (cross-module impl-block calls)

After the split, four private `fn`s on `impl VerterHost` were lifted to
`pub(super)` / `pub(crate)` because their callers crossed module
boundaries:

- `cache_import_route_result` → `pub(super)` (called from `virtual_file_pipeline::hydrate_compile_blockers`)
- `run_external_type_frontier_closure` → `pub(super)` (called from `external_type_resolution::resolve_external_type_from_loaded_files` and `resolve_component_meta_macro_elements_target`)
- `materialize_frontier_resolved_type` → `pub(super)` (called from external type resolution paths)
- `route_shallow_state` → `pub(super)` (called from `frontier_adapter::HostFrontierAdapter::ensure_shallow_state`)
- `resolve_named_type_export_target_uncached` → `pub(super)` (called from `route_owned_shallow::resolve_named_type_export_target`)
- `collect_external_types_from_loaded_files` → `pub(super)` (called from `virtual_file_pipeline::get_public_api_with_mode` and `compile_entry`)
- `collect_frontier_companion_seeds` → `pub(crate)` (called from `host_resolve_tests`)

The trait impls (`DeclarationMetadataResolver`, `FrontierHost`,
`ExternalMacroTypeCollectorHost`) and shared types
(`PlannedFrontierCompanion`, `FrontierCompanionPlans`,
`ExternalTypeTraceBaseline`, `ResolvedExternalTypes`,
`FrontierRequestedRoutes`, `RouteShallowStateCache`,
`FrontierCompanionPlanCache`, `ExternalTypeCache`) are
`pub(crate)` so they can flow through `pub(crate)` signatures and the
test-only re-exports.

## Companion change in tests

`crates/verter_session/src/project_global_cache_tests.rs` —
`request_view_is_retired_from_crate_sources` was previously loading
`host_resolve.rs` via `include_str!`; updated to load each of the 11
new sub-modules. The retired-symbol scan still runs across the same
total source surface.

## Verification

```bash
cargo test -p verter_session                               # 2556 passed, 0 failed
cargo test --workspace --tests -j 4                        # 10552 passed, 0 failed
cargo clippy --workspace --tests -- -D warnings            # clean
cargo fmt --all --check                                    # clean
```

`prior_known_passed_count: 10552` matched exactly.

## Marker

`crates/verter_session/.phase-markers/phase-tier-2-step-2.3-w5c-complete`

## Deviations / notes

- The audit's 4-module sketch (`external_type_frontier`, `named_type_cache`, `dependency_collection`, `snapshot_helpers`) covered only the resolution layer. The actual file also contained the SFC virtual-file pipeline (~1080 LOC), the Vue script-extraction helpers (~225 LOC), and the `HostFrontierAdapter` (~95 LOC), all of which had to land somewhere. They were placed in dedicated modules (`virtual_file_pipeline.rs`, `vue_script_extract.rs`, `frontier_adapter.rs`) rather than forced into the four sketched modules.
- The named-type-cache adapter the audit sketched is **not** a separate file; the `NamedTypeCache` parser-side adapter lives in `verter_parser` and was not part of this file's content. The Tier 0 audit doc names a "host-cached named-type adapter that talks to `SemanticGraphStore::get_resolved_named_type`"; on inspection, that adapter logic is in `crates/verter_session/src/host_resolve/external_type_resolution.rs::lookup_resolved_external_type_cache` (delegating to `ResolvedTypeCacheDb` via `project_type_store`) plus the parser-side `NamedTypeCache`. No new module was introduced for it.
- The `RouteOwnedShallowStateSnapshot` type's `pub(crate) use` re-export currently shows as "unused" in the lint surface (no in-crate caller names it; `resolver_store.rs` only iterates the returned `Vec` and reads fields). The re-export is retained behind `#[allow(unused_imports)]` to preserve the public surface guarantee.
