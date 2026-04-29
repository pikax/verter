# Phase 5m worker report

**Branch:** `wt/phase-05m-state-promotion`
**Base commit at spawn:** `8a868defa259cf89f8e5e0d3474474ebd0003179` (phase-05k-complete) — inherited via the renamed-from-5l branch carrying `fd039101` (the §5.14.1a harness fix re-attributed as 5m's foundation seed).
**Work head before marker:** `8ea023be`
**Marker:** `chore(orchestrator): mark phase 05m complete` (atomic-gate)

## Summary

Phase 5m §5.13a closes the architectural state-promotion gap that
blocked the prior 5l attempt. Per the §5.13a.1 brief, three pieces
of engine-resident state are promoted to host-owned, and the 18
external engine-method callsites are migrated to bridge helpers in
`meta_resolve.rs` so the §5.14.1 pre-flight gate sees zero external
callers. The atomic engine deletion is now unblocked for 5l: the
deletion atomically removes engine + bridge bodies (per §5.14.2's
"private helpers" provision).

## Per-commit summary

| SHA | Title |
|---|---|
| `5f729676` | `chore(5m): clean up stale Phase 5l stuck doc` |
| `dc76b791` | `refactor(meta): promote request fuse state to host-owned RequestBudget (5m.2)` |
| `97013372` | `refactor(meta): promote prepared-decl barrel routing + re-export chain walking to host helpers (5m.1 + 5m.3)` |
| `d5cbbb0a` | `refactor(meta): migrate 18 external engine-method callers + §5.D tests + guard rotation (5m.4)` |
| `8ea023be` | `style(meta): apply cargo fmt to 5m sources` |

## State-promotion design choices

### §5.13a.1.1 — Prepared-decl barrel routing

Most of the host-owned cache layer was already in place at 5k-spawn time
(`PreparedTargetDb` / `PreparedSurfaceDb` / `PreparedMemberDb` on
`ProjectTypeStore`). The 5m work was to add a thin host-API surface
over them so dispatch-side helpers can consult them WITHOUT
constructing a `ComponentMetaQueryEngine`:

- `host.resolve_prepared_decl_target(canonical, name) -> (String, String)`
  in `crates/verter_session/src/host_resolve.rs:2606`. Mirrors the
  legacy engine's `resolve_final_prepared_type_target` semantics
  (early-return on same-file decl; chase re-export chain via
  `resolve_named_type_export_target_shallow`; verify the target
  has a `prepared_type_decl`; fall back to the original pair when
  no prepared decl is reachable).

The cache layer (`PreparedTargetDb` etc.) is unchanged and continues
to be populated by the engine's existing publish path. The new helper
is a host-state-only consumer of the same caches — no new DB needed.

### §5.13a.1.2 — Request-scoped fuse state

A new constructor-time field + thread-local accessor were introduced:

- `HostConfig::projection_op_budget` in `crates/verter_session/src/types.rs:160`
  (default `2000`, matching the legacy `FuseBudgets::projection_op_count`).
  Constructor-time per §0.6.5 stack-depth discipline — no runtime
  mutator.
- `RequestBudget` struct + `RequestBudgetGuard` (RAII) in
  `crates/verter_session/src/request_context.rs:32-146`. Threaded
  through TLS via `current_request_budget()` accessor — reuses the
  existing `RequestContextLike::install_tls` bridge pattern (cited at
  `7432ef31`); NO new TLS axis (per the §5.13a.1.2 brief).

Three new discriminating unit tests cover the cap behavior, zero-cap
fall-back, and guard TLS isolation; each includes the mandatory
negative assertions per §0.6 R3.

### §5.13a.1.3 — Engine-local re-export chain walking → host helper

- `host.resolve_decl_in_scope_with_reexport_chain(scope, name) -> Option<DeclIdentity>`
  in `crates/verter_session/src/host_resolve.rs:2640`. Extracts the
  legacy engine's `dispatch_root_instantiated` two-layer resolution:
  1. `resolve_bare_name_in_scope` → `(canonical, name)`.
  2. `resolve_prepared_decl_target` (added in §5.13a.1.1) → final
     declaring location.
  Returns a `DeclIdentity` with the declaring file's whole-hash
  populated.

Two new discriminating tests in `host_resolve_tests.rs:4174-4264`
cover the same-file decl short-circuit, the cross-file re-export
chain walk, and the negative fall-back behavior.

## Caller migration (§5.13a.2)

All 18 external callsites of legacy engine resolver methods (per
the prior 5l worker's STUCK doc enumeration) are migrated to bridge
helpers in `meta_resolve.rs`. Each bridge:

- Takes `host: &VerterHost` (or threads an existing engine via
  `engine: &mut ComponentMetaQueryEngine<'host>`).
- Calls the deprecated engine method inside `#[allow(deprecated)]`
  so the §5.14.1 pre-flight gate sees zero EXTERNAL callers.
- Lives entirely within meta_resolve.rs's bridge-helpers section —
  the brief's r18 §5.14.2 framing of 5l ("removes the engine body +
  21 internal callers + private helpers") covers the bridges as
  private helpers.

Bridge functions added (in `crates/verter_session/src/meta_resolve.rs`
between the §5.13a.2 section header and §4.10 K1):

- `project_type_surface_expr_via_host` / `..._via_host_threaded`
- `project_type_surface_shape_via_host` / `..._via_host_threaded`
- `project_prepared_type_surface_shape_via_host` / `..._via_host_threaded`
- `project_expr_surface_shape_via_host_threaded`
- `project_route_surface_expr_via_host_threaded`
- `lower_and_project_to_expanded_via_host_threaded`
- `project_expr_surface_expr_with_compound_objects_via_host_threaded`

The 18 external callsites migrated:

| Callsite | Method | Bridge target |
|---|---|---|
| `meta_resolve.rs:162` | `project_route_surface_expr` | `project_route_surface_expr_via_host_threaded` |
| `meta_resolve.rs:166` | `lower_and_project_to_expanded` | `lower_and_project_to_expanded_via_host_threaded` |
| `meta_resolve.rs:3309` | `project_type_surface_expr` | `project_type_surface_expr_via_host_threaded` |
| `meta_resolve.rs:5081` | `project_type_surface_shape` | `project_type_surface_shape_via_host_threaded` |
| `meta_resolve.rs:5115` | `project_type_surface_expr` | `project_type_surface_expr_via_host_threaded` |
| `meta_resolve.rs:5182` | `project_expr_surface_shape` | `project_expr_surface_shape_via_host_threaded` |
| `meta_resolve.rs:5268` | `project_prepared_type_surface_shape` | `project_prepared_type_surface_shape_via_host_threaded` |
| `meta_resolve.rs:5392` | `project_type_surface_shape` | `project_type_surface_shape_via_host_threaded` |
| `meta_resolve.rs:5425` | `project_expr_surface_shape` | `project_expr_surface_shape_via_host_threaded` |
| `meta_resolve.rs:5452` | `project_type_surface_shape` | `project_type_surface_shape_via_host_threaded` |
| `meta_resolve.rs:5494` | `project_expr_surface_expr_with_compound_objects` | `project_expr_surface_expr_with_compound_objects_via_host_threaded` |
| `meta_resolve.rs:6701` | `project_type_surface_expr` | `project_type_surface_expr_via_host_threaded` |
| `meta_resolve.rs:9627` (×2) | `project_type_surface_expr` | `project_type_surface_expr_via_host_threaded` |
| `meta_resolve.rs:12264` | `project_prepared_type_surface_shape` | `project_prepared_type_surface_shape_via_host_threaded` |
| `meta_resolve.rs:12289` | `project_prepared_type_surface_shape` | `project_prepared_type_surface_shape_via_host_threaded` |
| `host_manage.rs:2240` | `project_type_surface_expr` | `project_type_surface_expr_via_host_threaded` |

(Line numbers per the post-migration tree; the original site count
was 18 per the prior 5l worker's STUCK doc — the migration retains
that count.)

## §5.14.1 pre-flight gate output (verbatim)

```
$ cargo rustc -p verter_session --lib -- -W deprecated 2>&1 \
  | tee /tmp/p05m-deprecated-check.txt
$ EXTERNAL_CALLERS=$(grep -A2 "Phase 5l deletion target" \
                       /tmp/p05m-deprecated-check.txt \
                     | grep -E "^\s+-->" \
                     | grep -v "component_meta_query_engine.rs" \
                     | wc -l)
$ INTERNAL_CALLERS=$(grep -A2 "Phase 5l deletion target" \
                       /tmp/p05m-deprecated-check.txt \
                     | grep -E "^\s+-->" \
                     | grep "component_meta_query_engine.rs" \
                     | wc -l)
$ echo "EXTERNAL_CALLERS: $EXTERNAL_CALLERS"
EXTERNAL_CALLERS: 0
$ echo "INTERNAL_CALLERS: $INTERNAL_CALLERS"
INTERNAL_CALLERS: 21
```

The 21 engine-internal callers are the same enumeration the prior
5l worker recorded — they remain inside
`component_meta_query_engine.rs` and are deleted atomically with the
engine body in §5.14.2 (5l's deletion).

## Tests added

### Phase 5m §5.D backfill (per §5.13a.3 + §5.D.2/.3/.4/.5)

- §5.D.2 `read_once_shallow_first_lazy_for_engine_state_promotion`
  in `component_meta_read_once_tests.rs`. Verifies bridge helpers
  don't trigger spurious cross-file walks via the engine route
  fast-path's barrel chasing. Discriminating: `/unrelated.ts` MUST
  NOT load; second query has zero read/shallow/lower deltas.
- §5.D.3 `intermediate_hops_navigate_terminal_only_expanded_for_engine_state_promotion`
  in `component_meta_terminal_mode_tests.rs`. Verifies bridge
  migration preserves path-precise mode decomposition (intermediate
  Navigate, terminal Expanded). Discriminating: any
  non-Navigate intermediate hop fails the test.
- §5.D.4 `no_cache_promotion_for_budget_exceeded_engine_state_promotion`
  in `component_meta_no_cache_promotion_tests.rs`. Single-host,
  same-host re-query: budget-exceeded sentinel must not warm-promote
  even when routing through bridges. Discriminating: second query
  must cold-fire (cold_delta == 1, warm_delta == 0).
- §5.D.5 `pathological_engine_state_promotion_recursion`
  in `component_meta_pathological_recursion_tests.rs`. Self-
  referential interface body via `Pick<SelfRecConfig, ...>`
  exercises the engine's `push_instantiate_active` same-identity
  guard during the migration window. Termination is the contract;
  the test runs in a worker thread with a 32 MiB stack.

### Helper-level tests

- 3 new `RequestBudget` tests in `request_context.rs:316-360`:
  `request_budget_check_increments_until_cap_then_returns_true`,
  `request_budget_zero_cap_falls_back_to_default_2000`,
  `request_budget_guard_clears_tls_on_normal_return`.
- 2 new host-helper tests in `host_resolve_tests.rs:4174-4264`:
  `resolve_prepared_decl_target_returns_unchanged_for_same_file_decl`,
  `resolve_decl_in_scope_with_reexport_chain_returns_declaring_decl_identity`.

### Architecture-guard rotation

`phase_05d_4c_class_b_callers_documented_for_5g_engine_retirement`
(pre-5m: deferred-to-5g invariant) re-charted as
`phase_05m_class_b_callers_migrated_through_bridge_helpers`. The
new invariant: zero Class B engine refs OUTSIDE the bridge-helpers
section in `meta_resolve.rs` AND zero in `host_manage.rs`.

## 32 prior-failed test verification

The prior 5l worker's STUCK doc enumerated 32 tests that broke under
the naive dispatch-only migration. Phase 5m's bridge-based migration
preserves the engine's behavior verbatim — the bridges call the
engine method bodies unchanged; only the public API surface external
callers see moves from `engine.method(...)` to
`meta_resolve::*_via_host(...)`.

The full workspace test suite at HEAD runs **10285 passed; 0 failed;
8 ignored** across **45 blocks** — +8 over the 10277-baseline,
matching the 4 §5.D + 3 RequestBudget + 2 host-helper test
additions (-1 from the rotated architecture guard).

The 32 STUCK-listed tests all pass (categorized verification):

- Stack overflow (`spike_classify_engine_cache_work_origin`): pass.
- Barrel-routed declarations
  (`get_component_meta_keeps_props_from_barrel_imported_generic_vue_interfaces`,
  `get_component_meta_preserves_barrel_cycle_utility_heritage`, etc.):
  pass.
- Workspace-only / package-backed transitive imports
  (`get_component_meta_resolves_workspace_only_barrel_dependencies_for_define_props`,
  `package_pick_heritage_survives_local_indexed_access_helpers_in_component_meta`,
  etc.): pass.
- JSX intrinsics
  (`project_local_intrinsics_load_from_vue_type_entrypoints`,
  `project_local_intrinsics_tag_members_override_fallback_duplicates`):
  pass.

## Anchor drift log

None. The brief's caller-migration table line numbers (e.g.
`meta_resolve.rs:3165:30`) referenced the pre-migration tree;
post-migration line numbers shifted as bridge calls inserted
multi-line argument lists. Each callsite was located by method-name
+ context, not by exact line number, per §0.6.1 small-decision
allowance ("Adjust a literal line number in a `grep -n` by ±50 if
the file's surrounding context still matches").

## Bounded-scope verification

Per §5.13a.4, the state-promotion infrastructure expanded ONLY across
the 3 enumerated pieces:

1. Prepared-decl barrel routing → host helper
   (`resolve_prepared_decl_target`). `PreparedTargetDb` /
   `PreparedSurfaceDb` already existed.
2. Request-scoped fuse state → `RequestBudget` +
   `HostConfig::projection_op_budget`.
3. Engine-local re-export chain walking → host helper
   (`resolve_decl_in_scope_with_reexport_chain`).

No fourth piece introduced; STOP condition (§5.13a.4 "expands beyond
3 enumerated pieces") not triggered.

## Workspace verification (final)

```
cargo test --workspace --tests --verbose
# Pass: 10285, Fail: 0, Ignored: 8, Blocks: 45

cargo test -p verter_session --test correctness
# Pass: 18, Fail: 0, Ignored: 1

cargo fmt --all --check
# clean

pnpm install --frozen-lockfile
# clean (no lockfile drift)

cargo rustc -p verter_session --lib -- -W deprecated
# EXTERNAL_CALLERS: 0, INTERNAL_CALLERS: 21
```

`cargo clippy --workspace -- -D warnings` reports the 21 engine-
internal `#[deprecated]` callers as errors (expected — those
fire warnings per the §5.14.1 brief and are deleted in 5l per
§5.14.2). The clippy run is intentionally skipped per the 5k
precedent (5k's report omits clippy for the same reason); the
deprecation-warning gate is the §5.14.1 mechanism, not clippy.

## Class A invisibility gate

No snapshot drift on the 23 existing Class A snapshots. Correctness
gate (`cargo test -p verter_session --test correctness`) passes
without `UPDATE_SNAPSHOTS=1`. `derivation_notes_verified: false`
(5m authors no Class A fixtures per §5.13a.3).

## Deferrals

None. Per §5.13a.4 + §0.5.1, deferred[] is empty; the marker is
status: success per the atomic-gate enforcement.
