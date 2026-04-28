# Phase 5d worker report

**Phase:** `phase-05d` — Class A + Class B callsite migration (Props/Slots surfaces + type-decl projection deferral)

**Branch:** `wt/phase-05d-callsite-migration-ab`
**Base at spawn:** `cb2718f7a8604b82c4a245c0095ac13e75685d52` (Phase 5c integrated)
**Worktree HEAD post-work:** `10103e7aeb525854c6372340ad92e0690c550368`
**Integration target:** `refactor/semantic-db-overhaul`

## Commits

| Order | SHA | Message |
|---|---|---|
| 1 | `e3c2a831` | `refactor(meta): migrate Props-surface callers (class A) — phase 5d 4a` |
| 2 | `c5f9552d` | `refactor(meta): migrate Slots-surface callers (class A) — phase 5d 4b` |
| 3 | `10103e7a` | `refactor(meta): document Class B type-decl deferral to 5g — phase 5d 4c` |
| (R7) | (next, after this report commit) | `chore(orchestrator): mark phase 05d complete` |

Total substantive commits: 3 (matches brief's 4a/4b/4c). Report + R7 marker pending.

## Per-commit migration counts

### 4a — Class A Props-surface migrations (commit `e3c2a831`)

| File | Pre-4a Class A engine refs | Post-4a Class A engine refs | Sites migrated |
|---|---|---|---|
| `crates/verter_session/src/meta_resolve.rs` | 17 | 6 (5 slot-cluster + 1 deferred 4942) | 11 Props sites |
| `crates/verter_session/src/host_manage.rs` | 4 (3 A + 1 B) | 1 (B site, deferred to 4c per §4.1) | 3 A sites |
| `crates/verter_session/src/resolver_core/type_expansion_verter.rs` | 2 | 0 | 2 sites |
| **Total 4a** | **23** | **7** | **16 (Props)** |

11 Props sites in `meta_resolve.rs` cover the §4.1 Class A row minus
the 5-site slot cluster (which migrates in 4b). New helpers landed in
`meta_resolve.rs`:

- `project_expr_class_a_via_dispatch(host, scope, expr) -> Option<TypeExpr>`
- `project_expr_class_a_shape_via_dispatch(host, scope, expr) -> Option<ExpandedObjectShape>`
- `project_expr_class_a_via_dispatch_threaded(...)` and `_shape_via_dispatch_threaded(...)` — engine-threaded forward-compat seams.

The helpers preserve the trampoline body's two-path semantics:
1. Registry-route fast path for indexed-access / utility shapes
   (Class D `project_route_surface_expr` / `lower_and_project_to_expanded`
   stay on the engine until 5e/5f).
2. Generic `ProjectPath { base: lowered, path: [], mode: Expanded }`
   dispatch + `expanded-surface` filter.

Plus the result filter (`!type_expr_contains_semantic_miss && type_expr_is_expanded_surface`)
is preserved, so Class A parity is bit-for-bit.

### 4b — Class A Slots-surface migrations (commit `c5f9552d`)

| File | Pre-4b Class A engine refs | Post-4b Class A engine refs | Sites migrated |
|---|---|---|---|
| `crates/verter_session/src/meta_resolve.rs` | 6 | 4 (3 multi-kind deferred-to-5g + 1 deferred 4942) | 2 slot-only sites |

2 of the 5 slot-cluster sites listed in §4.1 (4940, 4955 at pre-5d
HEAD; 5179/5195 post-4a after the helper-block addition) migrated. The
remaining 3 sites (4631, 4646, 4881 at pre-5d HEAD) live inside the
GENERIC multi-macro-kind helpers (`produce_one_macro_object_shape` and
`project_named_ref_imported_scope_shape`) — they serve all macro
kinds, not just slots. Per CLAUDE.md "Fix Quality":

> If the fix would be a workaround, patch, or shim → do NOT apply it.

Migrating those 3 sites without atomically promoting the engine's
load-bearing fuse + scope-payload + request-local prepared-decl
caches caused regressions in `solver_host_resolves_generic_imported_partial_props`
(`Partial<T>` optionality across macro kinds) and
`evaluate_types_hydrates_transitive_imported_pick_dependencies_from_dual_script_vue_deps`
(transitive Pick deps via dual-script Vue files). The 3 sites stay on
the engine helper with `TODO(phase-5g)` markers; they migrate
atomically with the engine retirement in 5g.

### 4c — Class B type-decl projection (commit `10103e7a`)

| File | Pre-4c Class B engine refs | Post-4c Class B engine refs | Sites migrated |
|---|---|---|---|
| `crates/verter_session/src/meta_resolve.rs` | 11 | 11 | 0 (deferred to 5g) |
| `crates/verter_session/src/host_manage.rs` | 1 | 1 | 0 (deferred to 5g) |
| `crates/verter_session/src/meta_resolve_tests.rs` (test 7698) | 1 | 1 | 0 (deferred to 5g) |

**Architectural finding — full Class B deferral to 5g:**

The trampoline's `project_type_surface` body is dispatch-first then
prepared-decl-second:
```rust
self.dispatch_projected_surface(scope, symbol)
    .or_else(|| self.cached_prepared_root_surface(scope, symbol))
```

The `cached_prepared_root_surface` fallback is essential for
re-exported / barrel-routed declarations (transitive heritage chains,
namespace-qualified imports like `JSX.IntrinsicElements`). A
dispatch-only Class B helper without that fallback regressed 47
workspace tests:

- Heritage chain resolution (`solver_host_resolves_transitive_same_file_deps_in_imported_type` etc.)
- Barrel imports (`barrel_many_wildcard_exports_resolves_without_hang` etc.)
- Complex generic Pick/Omit on multi-file types (`get_component_meta_keeps_picked_package_button_form_attrs_*` etc.)
- Cyclic barrel cycles (`get_component_meta_preserves_barrel_cycle_utility_heritage` etc.)
- JSDoc enrichment paths (`jsdoc_descriptions_propagate_through_*` etc.)
- `JSX.IntrinsicElements` resolution (`project_local_intrinsics_*` etc.)
- And ~30 more in the same family.

Even threading the engine's prepared-decl helper inside a Class B
helper does not match the trampoline's
`dispatch_projected_surface → projected_surface_to_type_expr` path
because that path flattens heritage members through the surface
walker; `raise_node_to_type_expr` over a dispatch-`Instantiate` result
does not flatten.

The proper fix threads the prepared-decl resolver through dispatch
atomically with the engine retirement in Phase 5g (sub-plan §5
commit 11). All 13 Class B sites stay on the engine helper for 4c,
each marked with a `TODO(phase-5g)` comment documenting the rationale.

The Class B helper sketches that were prototyped during 4c (which
caused the regressions) were removed from the tree — no half-baked
helpers remain. The 4c discriminating test
`phase_05d_4c_class_b_callers_documented_for_5g_engine_retirement`
asserts:
1. No `project_type_class_b_via_dispatch` helper references remain
   (prevents re-introduction of the regressing helper).
2. ≥5 `TODO(phase-5g)` markers exist in `meta_resolve.rs` so a
   follow-up worker locates the deferred sites.
3. Class B engine ref counts are preserved (no accidental site loss).

## Class A parity test status post-5d

`class_a_invisibility_mapped_pick_two_keys_unchanged` (5b commit
`a190a249`) — **GREEN** at every commit boundary in 5d. Verified
post-4a, post-4b, post-4c via:

```bash
cargo test --package verter_session --lib class_a_invisibility_mapped_pick_two_keys_unchanged
# running 1 test
# test project_semantic_dispatch::tests::class_a_invisibility_mapped_pick_two_keys_unchanged ... ok
```

## Test pass counts measured by this worker

Final workspace test suite ran with `cargo test --workspace --tests --verbose`:

```bash
cargo test --workspace --tests --verbose 2>&1 | tee /tmp/p05d-workspace.txt
```

Aggregate counts (from `/tmp/p05d-workspace.txt`):

- **Passed:** 10166
- **Failed:** 0
- **Ignored:** 13
- **Test result blocks:** 43 (≥ 40 per R11 brief)

`verter_session --lib` block: **1814 passed, 0 failed, 1 ignored, 0 measured, 0 filtered out** in 127.15s.

End-of-change verification commands:

| Check | Status |
|---|---|
| `cargo test --workspace --tests --verbose` | GREEN (10166/10166 passed, see `/tmp/p05d-workspace.txt`) |
| `cargo clippy --workspace --tests -- -D warnings` | GREEN (no warnings beyond ts-rs `failed to parse serde attribute`, which is pre-existing) |
| `cargo fmt --all -- --check` | GREEN |
| `pnpm install --frozen-lockfile` | GREEN |

## Seed tests that flipped green during 5d

None. The §5 sub-plan TDD seeds (`mapped_exclude`, `mapped_extract`,
`template_literal_as_key`, `generic_substitution_via_typeof`,
`userland_shadowing_pick`, `fixture_slots_typed`, `fixture_models`)
are scoped to 5e/5f/5g per sub-plan §5 commits 5/6/7/8/9. Phase 5d
does not migrate the route-loop / route-target / fallthrough /
indexed-paths / package-backed sites that close those seeds.

The `slot_shapes` seed (deferred from 5b) was not enabled in 4b
because its closure requires the `meta_resolve.rs::define_slots` arm
migration that the brief defers to 5e/5f.

## R7 marker

Path: `crates/verter_session/.phase-markers/phase-05d-complete`

Marker JSON populated below per locked R7 schema (sub-plan §0a R7
mandate; `workspace` + `correctness` keys).

## work_head_before_marker SHA

`10103e7aeb525854c6372340ad92e0690c550368` — the post-4c HEAD prior
to the R7 marker commit.

## Notes for follow-up phases

- **5e (route-loop + route-target):** the deferred-4942 slot-cluster
  site (`project_expr_surface_expr_with_compound_objects` inside
  `produce_one_macro_object_shape_for_slots`) is in scope. Plus the 4
  route-loop sites and 4 route-target sites in §4.1.
- **5f (fallthrough + indexed + package):** `fallthrough_resolver.rs`,
  `component_meta.rs`, `host_manage_tests.rs` Class A audits.
- **5g (engine deletion):** must atomically promote the engine's
  prepared-decl resolver into dispatch BEFORE deleting the engine
  trampolines, otherwise the Class B caller sites (deferred to 5g
  per this report) will regress. The `TODO(phase-5g)` markers in
  `meta_resolve.rs` and `host_manage.rs` enumerate the 13 deferred
  sites and the 3 multi-kind macro-shape sites from 4b.

The four 4b multi-macro-kind sites:
- `produce_one_macro_object_shape` body: 2 sites (`project_expr_surface_expr` + `project_expr_surface_shape`).
- `project_named_ref_imported_scope_shape` body: 1 site (`project_expr_surface_shape`).

These need the engine's request-local fuse + scope-payload state;
threading them through dispatch is a 5g-scope change.
