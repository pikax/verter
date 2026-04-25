# Step 0 — Pre-flight spike output

Source plan: `D:/tmp/architectural-debt-closure.md` (revision 10).

Two spike tests live in `crates/verter_session/src/meta_resolve_tests.rs`
(at `spike_dispatch_handles_props_t_substitution_via_macro_shell` and
`spike_classify_engine_cache_work_origin`). They are pre-flight only:
they validate the design assumptions Step 1 and Step 3 depend on, and
are removed when Step 1 lands its dispatch-substitution regression test
and Step 3 captures this classification table into its disposition
commit.

Run command:

```
cargo test --package verter_session spike_ -- --nocapture
```

## Spike #1 — macro-shell substitution

`spike_dispatch_handles_props_t_substitution_via_macro_shell` constructs
the parent macro shell `Props<T>` as a `TypeExpr` against the
script-setup-generic fixture `/Generic.vue`, calls
`dispatch.lower_type_expr_in_scope("/Generic.vue", &props_t)`, projects
the lowered node through `SemanticQueryKey::ProjectPath { ..., path:
[Member("items")], mode: Expanded }`, and raises the result back to
`TypeExpr`.

**Outcome: PASS.** The lowered `items` projection raises to
`Array { element: TypeParameter { name: "T", constraint: Some(Ref { name:
"Item", .. }) } }` — `T` is preserved as a `TypeParameter` carrying its
declared constraint.

**Implication for Step 1.** `dispatch.lower_type_expr_in_scope` correctly
substitutes the script-setup generic when given the *parent* macro shell
directly, without any per-field rewrite. Step 1's closure-rewrite
approach (route the macro shell through dispatch in the rewired closure
boundary, drop the per-field re-resolution legacy walker) is viable.

If Spike #1 had failed, dispatch substitution itself would have been
broken upstream, and Step 1 could not have proceeded — STOP CONDITION
#1.

## Spike #2 — empirical (b) cache classification

`spike_classify_engine_cache_work_origin` runs eight independent fixtures
under one `spike_instrumentation::enable()` window. Each fixture resets
the per-thread `LOWER_CALLED` marker (via
`spike_instrumentation::reset_lower_marker`) before running so each
fixture's reads are classified against THAT fixture's first
`dispatch.shallow_lower_type_expr` call rather than the cumulative
suite's. Classification semantics:

- `reads == 0` → `UNUSED_FIXTURE_INCOMPLETE` (HARD STOP — not delete authorization)
- `reads > 0` and any read happened before `LOWER_CALLED` was set → `PRE_LOWER` (MIGRATE)
- `reads > 0` and no read happened before `LOWER_CALLED` was set → `POST_LOWER` (DELETE candidate)

### Fixture suite (8 fixtures)

| # | Fixture function | What it drives |
|---|---|---|
| A | `run_classification_fixture_barrel_import` | Barrel-resolved generic Props target — drives `prepared_target_cache`, `prepared_surface_cache`, `routed_expr_surface_cache`, `prepared_member_cache`, `imported_registry_symbols` |
| B | `run_classification_fixture_generic_macro` | Script-setup generic macro shell — drives `materialize_memo`, `materialized_member_surfaces`, `owner_collection_exprs` |
| C | `run_classification_fixture_indexed_member_route` | Indexed-access member route — drives `declarations`, `resolvable` |
| D | `run_classification_fixture_pick_through_barrel` | `Pick<>` over barrel re-export hop — drives `prepared_target_cache`, `prepared_member_cache` |
| E | `run_classification_fixture_pick_with_key_alias` | `Pick<Target, KeyAlias>` (alias defined in another file) — drives the Ref-arm `prepared_string_literal_keys` path |
| F | `run_classification_fixture_omit_with_recursive_target` | `Omit<>` over recursively-extending interface — drives `prepared_member_cache` `or_else` fallback at `project_type_member` |
| G | `run_classification_fixture_alias_to_imported_ref` | Type alias whose body is a non-builtin Ref — drives the `_ =>` arm of `project_prepared_surface_from_ref` |
| H | `run_classification_fixture_direct_prepared_route_caches` | Direct `ComponentMetaQueryEngine` API exercise (`project_prepared_type_surface_expr` + `project_route_surface_expr`) — characterizes prepared-route caches that the public `get_component_meta` fixtures do not reach |

Fixture H is the direct-engine fixture added per the disposition note:
the public `get_component_meta` path does not exercise the
`project_prepared_type_surface_expr` and `project_route_surface_expr`
APIs in a way that hits the prepared-route caches under route demand,
so the spike must drive them directly to characterize them rather than
treating a public-fixture miss as dead code.

### Classification output (verbatim)

```
CACHE_CLASSIFICATION imported_registry_symbols:    PRE_LOWER  (reads=4)
CACHE_CLASSIFICATION declarations:                 PRE_LOWER  (reads=182)
CACHE_CLASSIFICATION resolvable:                   PRE_LOWER  (reads=2)
CACHE_CLASSIFICATION owner_collection_exprs:       POST_LOWER (reads=1)
CACHE_CLASSIFICATION prepared_target_cache:        PRE_LOWER  (reads=7)
CACHE_CLASSIFICATION materialize_memo:             POST_LOWER (reads=32)
CACHE_CLASSIFICATION materialized_member_surfaces: PRE_LOWER  (reads=60)
CACHE_CLASSIFICATION prepared_surface_cache:       PRE_LOWER  (reads=59)
CACHE_CLASSIFICATION prepared_member_cache:        PRE_LOWER  (reads=12)
CACHE_CLASSIFICATION routed_expr_surface_cache:    PRE_LOWER  (reads=7)

spike #2 summary: 8/10 caches PRE_LOWER (MIGRATE candidates),
                  2 POST_LOWER (DELETE candidates — parity-test gated in Step 2/3).
```

### Step 3 disposition table (derived from spike output)

| Cache | Classification | Disposition | Sub-task |
|---|---|---|---|
| `imported_registry_symbols` | PRE_LOWER | MIGRATE | 3.2 |
| `declarations` | PRE_LOWER | MIGRATE | 3.2 |
| `resolvable` | PRE_LOWER | MIGRATE | 3.2 |
| `prepared_target_cache` | PRE_LOWER | MIGRATE | 3.2 |
| `materialized_member_surfaces` | PRE_LOWER | MIGRATE | 3.2 |
| `prepared_surface_cache` | PRE_LOWER | MIGRATE | 3.2 |
| `prepared_member_cache` | PRE_LOWER | MIGRATE | 3.2 |
| `routed_expr_surface_cache` | PRE_LOWER | MIGRATE | 3.2 |
| `owner_collection_exprs` | POST_LOWER | DELETE candidate | 3.1 (gated by sub-task 2.0 parity) |
| `materialize_memo` | POST_LOWER | DELETE candidate | 3.1 (gated by sub-task 2.0 parity) |

Both DELETE candidates remain gated by Step 2's caller-class parity
matrix. If a parity test for either cache fails, that cache shifts to
MIGRATE.

### Per-fixture lower-marker reset (architectural note)

Earlier iterations of Spike #2 used a single global `LOWER_CALLED` flag
that was set once and never cleared between fixtures. Once *any*
fixture triggered `dispatch.shallow_lower_type_expr`, every subsequent
fixture's reads were misclassified as POST_LOWER even when the read
came before that fixture's own first lower call. The fix is the
per-fixture `reset_lower_marker()` call on the test driver helper
`run_spike_classification_fixture`; classification is meaningful only
relative to each fixture's own lowering ordering, not the cumulative
suite's.

This matters because the misclassification used to mask two PRE_LOWER
caches as POST_LOWER, which under the (now-removed) "presumed-dead"
escape hatch in revision 9 would have driven them to deletion. The
HARD STOP rule (UNUSED is never delete authorization) plus per-fixture
reset together close that gate-bypass.

## Verification

- `cargo test --package verter_session spike_ -- --nocapture` → both
  spikes PASS, all 10 caches `reads > 0`, no `UNUSED_FIXTURE_INCOMPLETE`
  classifications, HARD STOP `assert!(unused_caches.is_empty())` does
  not fire.
- Hook coverage verified by `grep` for `spike_instrumentation::record_*`:
  one entry hook in `project_semantic_dispatch::lower::shallow_lower_type_expr`,
  ten cache hooks across `resolver_core::component_meta_query_engine`
  (including both `prepared_member_cache` `.get()` sites at lines 2594
  and 5076) plus one in `meta_resolve::materialize_component_meta_type_expr_until_stable_full`.
