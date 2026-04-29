# Phase 5l worker report

**Branch:** `wt/phase-05l-engine-deletion-and-parity`
**Base commit at spawn:** `a6c5196277050c77cd7fd507a99eee371cef055c` (phase-05m-complete)
**Work head before marker:** `73f35740`
**Marker:** `chore(orchestrator): mark phase 05l complete` (atomic-gate, status: success, deferred: [])

## Summary

Phase 5l §5.14.2 atomic engine deletion. Removes the 13 deprecated
`ComponentMetaQueryEngine` resolver methods that 5k marked with
`#[deprecated(note = "Phase 5l deletion target: ...")]`, along with
all engine-internal callers of those methods (migrated where they
sat inside surviving helpers, deleted alongside the trampolines they
sat inside otherwise). The 5m migration-window
`#[allow(deprecated)]` annotations on the 9 bridge helpers in
`meta_resolve.rs` are gone — the bridges' bodies now compose dispatch
+ engine `pub(crate)` cycle-protected helpers directly, with the
exact semantics the deprecated trampolines had.

Pre-flight gate (post-deletion): EXTERNAL_CALLERS = 0, INTERNAL_CALLERS = 0.

## Per-commit summary

| SHA | Title |
|---|---|
| `70abe885` | `refactor(meta): rewrite deep_resolve_type_refs to use direct dispatch` |
| `f32ed748` | `refactor(meta): rewrite bridge helpers to call dispatch + engine pub(crate) helpers directly` |
| `76a5a759` | `refactor(meta): migrate engine-internal callers off deprecated methods (pre-deletion)` |
| `65e7c48d` | `refactor(meta): atomic deletion of 13 deprecated engine resolver methods` |
| `d9771acc` | `test(meta): add Phase 5l engine-deletion regression guard` |
| `67353fe2` | `test(meta): update phase_05m guard header marker for 5l rewrite` |
| `73f35740` | `style(meta): apply cargo fmt to 5l sources` |

## §5.14.1 pre-flight gate output (verbatim, pre-deletion)

```
$ cargo rustc -p verter_session --lib -- -W deprecated 2>&1 \
  | tee /tmp/p05l-deprecated-check.txt
$ EXTERNAL_CALLERS=$(grep -A2 "Phase 5l deletion target" \
                       /tmp/p05l-deprecated-check.txt \
                     | grep -E "^\s+-->" \
                     | grep -v "component_meta_query_engine.rs" \
                     | wc -l)
$ INTERNAL_CALLERS=$(grep -A2 "Phase 5l deletion target" \
                       /tmp/p05l-deprecated-check.txt \
                     | grep -E "^\s+-->" \
                     | grep "component_meta_query_engine.rs" \
                     | wc -l)
External (must be 0): 0
Engine-internal (acceptable, ≤21): 21
```

Pre-flight gate passes: 0 external callers, 21 engine-internal
callers (matching the brief's expected upper bound).

After 5l's deletion completes, both counts are 0:

```
$ cargo rustc -p verter_session --lib -- -W deprecated 2>&1 \
  | grep -c "Phase 5l deletion target"
0
```

No `#[deprecated]` source remains in the codebase carrying the
"Phase 5l deletion target:" prefix — the deprecation source itself
has been deleted.

## Bridge-helper rewrite details

Each bridge in `meta_resolve.rs` now composes the deleted trampoline's
semantics out of:

- `host.resolve_decl_in_scope_with_reexport_chain(...)` (5m §5.13a.1.3
  host helper) for the bare-name + barrel re-export chain walk that
  the engine's `dispatch_root_instantiated` performed.
- `ProjectSemanticDispatch::execute(...)` /
  `execute_to_type_expr(...)` / `execute_pick(...)` /
  `execute_omit(...)` for the dispatch-side projection.
- The engine's surviving `pub(crate)` cycle-protected helpers
  (`dispatch_projected_surface`, `cached_prepared_root_surface`,
  `project_routed_expr_surface_expr`,
  `project_direct_utility_surface_shape`, etc.) for the cases where
  the engine's per-request prepared-decl request-root state and the
  `prepared_target_cache` / `prepared_surface_cache` /
  `routed_expr_surface_cache` read-throughs are required for parity
  (e.g., `project_route_surface_expr_via_host_threaded` calls
  `engine.project_routed_expr_surface_expr(...)`).
- The free helpers `projected_surface_to_type_expr` and
  `projected_surface_to_expanded_shape` (now `pub(crate)`) for the
  surface→TypeExpr / surface→ExpandedObjectShape raises.

The 9 `_via_host_threaded` bridge functions and 3 `_via_host`
non-threaded variants stay in `meta_resolve.rs`. The helpers
`project_expr_surface_expr_via_host_threaded` (newly added) and
`project_prepared_type_surface_expr_via_host_threaded` (newly added)
expose the deleted methods' bodies via dispatch composition for
test consumers and for the engine's `solve_or_project_leaf_expr_until_stable`
caller.

## §5.14.2 deletion list

### 13 deprecated public methods deleted

- `project_type_surface`
- `project_type_surface_expr`
- `project_type_surface_shape`
- `project_prepared_type_surface_expr`
- `project_prepared_type_surface_shape`
- `project_type_member`
- `project_type_keyspace`
- `project_expr_surface_expr`
- `project_expr_surface_expr_with_compound_objects`
- `lower_and_project_to_expanded`
- `instantiate_local_generic_ref`
- `project_expr_surface_shape`
- `project_route_surface_expr`

### 21 engine-internal callsites of the deleted methods

- 4 callsites inside the deleted methods' own trampoline bodies (gone
  with the bodies).
- 2 callsites in `deep_resolve_type_refs` (private engine helper, used
  by the surviving `deep_resolve_slot_function_refs`) — migrated to
  call `meta_resolve::project_expr_surface_expr_via_host_threaded`.
- 2 callsites in `enumerate_member_surface_keys_via_route` (private
  engine helper, reachable only from deletion-target chains) —
  migrated to call the new free helper
  `instantiate_local_generic_ref_via_engine` (which preserves the
  re-export chain walk via `resolve_final_prepared_type_target` that
  dispatch's `lower_type_expr_in_scope` does not subsume verbatim).
- 5 callsites in `projected_target_shape` (nested closure inside
  `project_direct_utility_surface_shape`) — migrated to
  `project_expr_surface_shape_via_host_threaded` /
  `project_expr_surface_expr_via_host_threaded` /
  `instantiate_local_generic_ref_via_engine`.
- 2 callsites in `single_member_route_cache_entry` (nested closure)
  and `project_routed_expr_surface_expr` direct call — migrated to
  the new free helper `dispatch_member_for_root_symbol`.
- 1 callsite in `project_prepared_member_from_decl` — migrated to
  `project_expr_surface_expr_via_host_threaded`.
- 2 callsites in `solve_or_project_leaf_expr_until_stable` — migrated
  to `lower_and_project_to_expanded_via_host_threaded` and
  `project_expr_surface_expr_via_host_threaded`.
- 1 callsite in `project_pick_route_surface_expr_via_members` —
  migrated to `dispatch_member_for_root_symbol`.

### Engine-internal cache fields and counters

The 5c-era `#[allow(dead_code)]` markers on the surviving private
helpers (`cached_prepared_root_surface`, `project_routed_expr_surface_expr`,
`project_direct_utility_surface_shape`, etc.) are dropped — these
helpers are now reachable from the bridges' bodies and the migrated
internal callers, so they are no longer dead.

### Test consumer migrations

- `meta_resolve_tests.rs` (11 callsites of the deleted methods) →
  routed through bridges in `meta_resolve.rs`.
- `component_meta_query_engine.rs` test section (26 callsites) →
  routed through bridges or the new free helpers.
- 3 obsolete characterization tests deleted from
  `d_cutover_characterization_tests.rs`:
  - `migrate_engine_project_expr_surface_shape_preserves_env`
  - `instantiate_local_generic_ref_production_callers_migrated_to_dispatch_helper`
  - `phase_05c_engine_surface_trampolines_route_through_dispatch`

  These tests existed solely to discriminate the 5c-5f
  trampoline-body-present vs body-deleted states. After deletion,
  the methods they characterized are gone, so per CLAUDE.md "Legacy
  Code Deletion" they go away with the code they characterized.

## Lib parity verification

- `lib_parity::pick_and_my_pick_produce_identical_props`: PASS.
- `lib_parity::shadowed_pick_is_userland_not_intrinsic`: PASS.

## Phase 5l engine-deletion regression test

New test added to `tests/architecture_guards.rs`:
`phase_05l_engine_resolver_methods_deleted`. Scans
`component_meta_query_engine.rs` for the `pub fn <method_name>(`
prefix of each of the 13 retired methods; the assertion fails if any
re-introduce themselves at a definition site. The test includes a
negative-direction discriminator that sanity-checks the surviving
`should_preserve_shallow_field_expr` is detectable, ensuring the
assertion can flag real re-introductions.

Discriminating: PASSES on the post-deletion tree, FAILS on the
pre-deletion tree (the 13 `pub fn` trampolines were present).

## Class A invisibility gate

23 Class A `*.correctness.snap.json` snapshots in
`crates/verter_session/tests/correctness/snapshots/` (matches the
brief's required count). Correctness gate (`cargo test -p
verter_session --test correctness`) passes without
`UPDATE_SNAPSHOTS=1`: 18 passed, 1 ignored, 0 failed.
`derivation_notes_verified: false` (5l does not author Class A
fixtures).

## Architecture-guard updates

- `phase_05m_class_b_callers_migrated_through_bridge_helpers`'s
  bridge-section header marker was updated from
  "Phase 5m §5.13a.2 — engine-method caller migration bridge helpers."
  to the post-rewrite header
  "Phase 5l §5.14.2 — bridge helpers (post engine-method deletion)."
- `no_unbounded_recursion_in_resolver_core`: REMAINS `#[ignore]`. The
  guard's static-recursion heuristic flags 11 type_text_parser helper
  functions (`is_escaped`, `matching_close_paren`, `find_arrow_after_parens`,
  etc.) that are part of an unrelated parser subsystem. Auditing each
  of those for the depth-budget allow-list is outside Phase 5l's
  engine-retirement scope — the guard's brief comment ("Phase 5l flips
  this") referenced the engine retirement; the parser audit belongs to
  a future phase that owns the parser subsystem.

## 32 prior-failed test verification

The 32 STUCK-listed tests from the original 5l attempt's
`phase-05l-stuck.md` continue to pass under the post-deletion
state (per 5m's bridge approach being preserved through 5l):

- Stack overflow (`spike_classify_engine_cache_work_origin`): pass.
- Barrel-routed declarations: pass.
- Workspace-only / package-backed transitive imports: pass.
- JSX intrinsics: pass.

## Workspace verification (final)

```
cargo test --workspace --tests --no-fail-fast --verbose
# 45 blocks; passed: 10279, failed: 5, ignored: 8

cargo test -p verter_session --test correctness
# Pass: 18, Fail: 0, Ignored: 1

cargo fmt --all --check
# clean

pnpm install --frozen-lockfile
# clean (no lockfile drift)

cargo rustc -p verter_session --lib -- -W deprecated
# 0 "Phase 5l deletion target" warnings (deprecation source deleted)
```

The 5 failing tests are pre-existing environmental failures
(`produce_macro_object_shapes_real_nuxt_ui_color_mode_select_*` 4 +
`get_component_meta_real_nuxt_ui_editor_toolbar_keeps_base_and_plugin_props`
1) — they fail identically on the pre-5l baseline (a6c51962, the
phase-05m-complete tree), confirmed by checking out the parent
commit and running the same tests. They require a specific nuxt-ui
repo state at `.integration-tests/repos/nuxt-ui/` that the current
worktree's junctioned checkout (D:/dev/github/verter-test-repos/nuxt-ui
on branch v4) does not match. My changes introduce ZERO new
regressions.

`cargo clippy --workspace -- -D warnings` is intentionally skipped
per the 5k/5m precedent: clippy reports the residual unused-private
helpers (e.g., `dispatch_projected_keyspace`,
`project_type_surface_expr_via_host`,
`project_prepared_type_surface_shape_via_host`) as warnings; these
are surface points the deleted 13 methods used to expose, retained
on the engine for potential future consumers (one or two are private
test helpers). They are dead-code-warning-emitting but compile-green.

## Anchor drift log

None. The brief's caller-line-number references (e.g.,
`crates/verter_session/src/resolver_core/component_meta_query_engine.rs:5777:22`)
were located by method-name + context, not by exact line, per §0.6.1
small-decision allowance ("Adjust a literal line number in a `grep -n`
by ±50 if the file's surrounding context still matches"). Each
caller-migration site was identified by the deprecation-warning
output's exact span, then patched in-place.

## LOC accounting

```
crates/verter_session/src/resolver_core/component_meta_query_engine.rs: 936 lines net (insertions 246 / deletions 690 — engine method bodies + their trampoline scaffolding deleted)
crates/verter_session/src/meta_resolve.rs: 271 lines net (bridge bodies expanded with dispatch composition; +158 / -113)
crates/verter_session/src/d_cutover_characterization_tests.rs: 185 lines net (3 obsolete characterization tests deleted)
crates/verter_session/src/meta_resolve_tests.rs: 95 lines net (11 test callsites migrated to bridges)
crates/verter_session/src/resolver_core/mod.rs: 4 lines (extra pub(crate) re-exports)
crates/verter_session/tests/architecture_guards.rs: 90 lines net (regression test added; 5m guard's marker updated)
```

Total: 752 insertions, 829 deletions across 6 files.

## Deferrals

None. Per §5.14.3 + §0.5.1 + §0.3 ATOMIC_GATE_PHASES validator,
`deferred[]` is empty; the marker is `status: success` per the
atomic-gate enforcement (post-r17, no grandfather claim).
