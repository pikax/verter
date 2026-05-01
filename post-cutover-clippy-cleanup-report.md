# Post-Cutover Clippy Cleanup Report

## Context

Following the Verter Architecture Cutover (HEAD `92e455cd`), the workspace
test suite passed at 10297/0/2/45 but `cargo clippy --workspace -- -D
warnings` produced 75 baseline errors. Phase 9b worker noted these were
pre-existing tech debt that predated the cutover. This phase resolves
all 75.

## Pre-fix baseline

`cargo clippy --workspace -- -D warnings` — 75 errors:

| Category                                                                                                | Count |
|---------------------------------------------------------------------------------------------------------|-------|
| `unused-imports` (multi-symbol per error)                                                               | 30    |
| `private_interfaces` (`ResolverContext` more private than items)                                        | 17    |
| `dead-code` (functions / methods never used)                                                            | 9     |
| `arc_with_non_send_sync`                                                                                | 1     |
| `doc_lazy_continuation`                                                                                 | 2     |
| `empty_line_after_doc_comments`                                                                         | 1     |
| Sub-total (after `verter_session` lib build aborts)                                                     | 60    |
| Reported by clippy as "75 errors" — the difference reflects multi-cause errors                          | 75    |

`cargo clippy --workspace --all-targets -- -D warnings` additionally
produced an `enum_variant_names` warning in `tests/architecture_guards.rs`.

## Auto-fix sweep

`cargo clippy --fix --allow-dirty --allow-staged --workspace -- -D
warnings` resolved 35 of the unused-import errors. The auto-fix was
over-aggressive — it removed re-exports needed by the in-tree test
modules (included via `#[cfg(test)] #[path]`). Workspace test build
broke with 104 unresolved-symbol errors. Per §0.6.2 STOP condition, the
auto-fix commit was reverted and the imports were re-added selectively
gated `#[cfg(test)] pub(crate) use ...` so the non-test surface stays
narrow while preserving the test re-export contract.

Files touched by the import sweep:
- `crates/verter_session/src/component_meta_materialize.rs`
- `crates/verter_session/src/host_manage.rs`
- `crates/verter_session/src/host_manage/analysis_io.rs`
- `crates/verter_session/src/host_manage/component_meta_methods.rs`
- `crates/verter_session/src/host_manage/jsdoc_resolve.rs`
- `crates/verter_session/src/meta_resolve.rs`
- `crates/verter_session/src/meta_resolve/dispatch_helpers.rs`
- `crates/verter_session/src/meta_resolve/graph_predicates.rs`
- `crates/verter_session/src/meta_resolve/materialize/field_types.rs`
- `crates/verter_session/src/meta_resolve/materialize/macro_shapes.rs`
- `crates/verter_session/src/meta_resolve/materialize/mod.rs`
- `crates/verter_session/src/meta_resolve/registry_materialize.rs`

Net: ~30 imports removed from non-test re-exports, ~25 imports re-added
as `#[cfg(test)] pub(crate) use` in `meta_resolve.rs` /
`host_manage.rs` / `meta_resolve/materialize/mod.rs`.

## Manual fix sweep

### Doc-comment formatting (4 errors → 0)

- `meta_resolve/graph_predicates.rs` — 25-line orphaned `///` doc-block
  preceding the `RouteExtraction` struct converted to non-doc `//`-style
  prose. The doc-block originally documented a predicate function that
  was extracted during the structural-materialisation refactor;
  preserving the rationale as a comment retains the historical context
  without triggering `empty_line_after_doc_comments`.
- `meta_resolve/materialize/mod.rs` — list continuation reformatted so
  the `+ _full` token in the `field_types` bullet does not hit
  `pulldown-cmark`'s `+`-as-list-marker parsing path; this resolved both
  `doc_lazy_continuation` errors.
- `verter_lsp/src/server/mod.rs` — trailing `///` blank line before
  `pub struct VerterLanguageServer` removed.

### Visibility downgrades (17 errors → 0)

The `ResolverContext` trait is `pub(crate)` (sealed, only `VerterHost`
implements it). All `pub fn` items that take `&dyn ResolverContext`
trigger the `private_interfaces` lint. Resolution: downgrade to
`pub(crate) fn` since they aren't usable externally anyway. Affected:

| File                                                                          | Item                                             |
|-------------------------------------------------------------------------------|--------------------------------------------------|
| `component_meta_caches.rs`                                                    | 9× `get_or_compute<F>` + 7× `peek` (16 items)    |
| `component_meta_materialize.rs`                                               | `materialize_component_meta_structure`           |
| `project_semantic_dispatch/mod.rs`                                            | `ProjectSemanticDispatch::new`                   |
| `project_semantic_dispatch/mod.rs`                                            | `SessionDispatchHost::new`                       |
| `project_semantic_dispatch/mod.rs`                                            | `node_data_for`                                  |
| `resolver_core/component_meta_query_engine/mod.rs`                            | `ComponentMetaQueryEngine::new`                  |
| `resolver_core/type_expansion_verter.rs`                                      | `resolved_macro_to_expansion_via_solver`         |

### Dead-code resolution (9 errors → 0)

- **Deleted as truly orphaned (no caller in any tree):**
  - `direct_macro_type_reference_expr` (resolver_core/component_meta.rs)
  - `find_matching_angle` (resolver_core/component_meta.rs)
  - `split_top_level_type_args` (resolver_core/component_meta.rs)
  - `project_type_surface_expr_via_host` (sync wrapper, dispatch_helpers.rs)
  - `project_type_surface_shape_via_host` (sync wrapper, dispatch_helpers.rs)
  - `project_prepared_type_surface_shape_via_host` (sync wrapper, dispatch_helpers.rs)
  - Total: ~150 lines removed.
- **Gated `#[cfg(test)]` (test-only):**
  - `host_resolve.rs::resolve_prepared_decl_target`
  - `host_resolve.rs::resolve_decl_in_scope_with_reexport_chain`
  - `resolved_macro_to_expansion_via_solver` (post-downgrade, then gated)
  - `project_prepared_type_surface_expr_via_host_threaded`
- **`#[allow(dead_code)]` with rationale (retained for API symmetry):**
  - `dispatch_projected_keyspace` — paired with `dispatch_projected_member` /
    `dispatch_projected_surface` as part of the
    `ComponentMetaQueryEngine` surface contract.
  - `ResolverContext::current_dependency_fact_versions` —
    component-meta-tier bridge surface for future adapters.
  - `ResolverContext::get_raw_analysis_snapshot` — same routing as above.

### `arc_with_non_send_sync` rationale (1 error → 0)

`RequestBudget::new` annotated:

```rust
#[allow(
    clippy::arc_with_non_send_sync,
    reason = "request-scoped, TLS-pinned; Arc retained for resolver-core API symmetry"
)]
```

The `Arc<RequestBudget>` is request-scoped and lives behind the
request-context TLS axis; it never crosses a thread boundary, so the
`!Sync` `Cell<usize>` counter is safe even though `Arc::new` triggers
the lint. Switching to `Rc` would lose the structural compatibility
with the rest of the resolver-core API surface (which threads `Arc<…>`
everywhere else).

### `enum_variant_names` rationale (1 all-targets error → 0)

`tests/architecture_guards.rs::ViolationKind { UsePath, TypePath, ExprPath }`
annotated `#[allow(clippy::enum_variant_names)]` — the postfixes
preserve the AST-position distinction (`use` statement vs type
position vs expression position).

### Architecture-guard discriminator update

`phase_05l_engine_resolver_methods_deleted` test scans for the
surviving constructor signature in `component_meta_query_engine/mod.rs`.
Updated discriminator from `pub fn new(ctx: &'a dyn ResolverContext)` to
`pub(crate) fn new(ctx: &'a dyn ResolverContext)` to match the
visibility downgrade in §3.

## Final clippy state

`cargo clippy --workspace -- -D warnings` — clean (0 errors).
`cargo clippy --workspace --all-targets -- -D warnings` — clean (0 errors).
`cargo fmt --all --check` — clean.

### `#[allow(...)]` annotations added

All annotations carry an explanatory comment per the prompt's
constraint:

| File                                                                          | Lint                                  | Rationale (one-line)                                                       |
|-------------------------------------------------------------------------------|---------------------------------------|----------------------------------------------------------------------------|
| `request_context.rs`                                                          | `arc_with_non_send_sync`              | request-scoped, TLS-pinned; Arc retained for resolver-core API symmetry    |
| `resolver_core/component_meta_query_engine/registry_decl.rs`                  | `dead_code`                           | API symmetry with `dispatch_projected_member` / `_surface`                 |
| `resolver_core/resolver_context.rs`                                           | `dead_code` (×2)                      | sealed-trait component-meta-tier bridge surface                            |
| `tests/architecture_guards.rs`                                                | `enum_variant_names`                  | postfixes preserve AST-position distinction                                |

## Test count verification

`cargo test --workspace --tests --verbose` — **10297 passed / 0 failed
/ 2 ignored / 45 blocks**. Exact match to the pre-fix invariant.

`cargo test -p verter_session --test correctness` — 18 passed / 0
failed / 1 ignored.

`pnpm install --frozen-lockfile` — passed.

## Files touched (consolidated)

- `crates/verter_lsp/src/server/mod.rs`
- `crates/verter_session/src/component_meta_caches.rs`
- `crates/verter_session/src/component_meta_materialize.rs`
- `crates/verter_session/src/host_manage.rs`
- `crates/verter_session/src/host_manage/analysis_io.rs`
- `crates/verter_session/src/host_manage/component_meta_methods.rs`
- `crates/verter_session/src/host_manage/jsdoc_resolve.rs`
- `crates/verter_session/src/host_resolve.rs`
- `crates/verter_session/src/meta_resolve.rs`
- `crates/verter_session/src/meta_resolve/dispatch_helpers.rs`
- `crates/verter_session/src/meta_resolve/graph_predicates.rs`
- `crates/verter_session/src/meta_resolve/materialize/field_types.rs`
- `crates/verter_session/src/meta_resolve/materialize/macro_shapes.rs`
- `crates/verter_session/src/meta_resolve/materialize/mod.rs`
- `crates/verter_session/src/meta_resolve/registry_materialize.rs`
- `crates/verter_session/src/project_semantic_dispatch/mod.rs`
- `crates/verter_session/src/request_context.rs`
- `crates/verter_session/src/resolver_core/component_meta.rs`
- `crates/verter_session/src/resolver_core/component_meta_query_engine/mod.rs`
- `crates/verter_session/src/resolver_core/component_meta_query_engine/registry_decl.rs`
- `crates/verter_session/src/resolver_core/resolver_context.rs`
- `crates/verter_session/src/resolver_core/type_expansion_verter.rs`
- `crates/verter_session/tests/architecture_guards.rs`

## Commits

1. `0af11fa5` — `style(workspace): resolve 75 baseline clippy errors (post-cutover cleanup)`
2. (pending) — `chore(orchestrator): mark post-cutover-clippy-cleanup complete`
