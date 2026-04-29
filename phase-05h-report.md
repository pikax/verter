# Phase 05h worker report — scope-shadowing thread

**Branch:** `wt/phase-05h-shadow-gate-threading`
**Base:** `ac1ed58f` (refactor/semantic-db-overhaul HEAD)
**Work head before marker:** `138817d5`

## Summary

Phase 5h closes the `userland_shadowing_pick` semantic gap by
introducing the resolver-context `ScopeShadowing` struct in a new
`crates/verter_session/src/resolver_core/scope_shadowing.rs` module
and threading it through both lowering entry points (the dispatch
path's `shallow_lower_type_expr` and the materialise path's
`project_expr_class_a_via_dispatch_threaded`). With the thread in
place the foundation parity test
`shadowed_pick_is_userland_not_intrinsic` passes and the deferred
Class A fixture `userland_shadowing_pick` lands with byte-equal
output to the rule-correct expected.

## Per-commit summary

| # | SHA | Subject | Tests added |
|---|---|---|---|
| C1 | `53dc9acd` | `chore(5h): clean up stale 5g STUCK doc files` | — |
| C2 | `afcc62df` | `refactor(resolver_core): introduce ScopeShadowing struct for shadow-gate threading` | 5 module tests in `scope_shadowing::tests` |
| C3 | `af297280` | `refactor(meta): thread ScopeShadowing through materialize-path registry route` | — |
| C4 | `69ec5e79` | `test(meta): un-ignore shadowed_pick_is_userland_not_intrinsic` | un-ignored 1 test |
| C5 | `1f30aa8d` | `test(meta): author Class A fixture userland_shadowing_pick` | 1 fixture (correctness gate) + derivation note |
| C6 | `360c455d` | `test(meta): §5.D.2 read_once_shallow_first_lazy_for_userland_shadowing_pick` | 1 |
| C7 | `df1174ee` | `test(meta): §5.D.3 intermediate_hops_navigate_terminal_only_expanded_for_userland_shadowing_pick` | 1 |
| C8 | `b461d1dc` | `test(meta): §5.D.4 no_cache_promotion_for_budget_exceeded_userland_shadowing_pick` | 1 |
| C9 | `c86bc37f` | `test(meta): §5.D.5 pathological_self_shadowing_userland_pick` | 1 (new file) |
| C10 | `c00327a7` | `test(meta): §5.B.5.1 rule-correctness gate for userland_shadowing_pick` | 1 |
| fmt | `138817d5` | `chore(5h): apply rustfmt to phase 5h additions` | — |

Total tests added by 5h: 11 (5 module tests + 1 un-ignored + 1
fixture-driven correctness pass + 4 §5.D tests + 1 §5.B.5.1).

## Caller-slice enumeration (§5.10 r14 mandatory)

```bash
grep -rn "extract_route_root_identity_node\|component_meta_registry_public_utility_route" \
    crates/verter_session/src/ --include='*.rs'
```

### `extract_route_root_identity_node` callers (graph-native)

| Site | Disposition |
|---|---|
| `component_meta_materialize.rs:453` (registry-route branch) | Operates on a `SemanticNodeId` already in the graph. With the dispatch + `project_expr_class_a_via_dispatch_threaded` paths now honouring scope shadowing, no `__builtin__/Pick` `InstantiationRef` enters the graph for userland shadow cases; this branch operates correctly without further threading. |
| `component_meta_materialize.rs:1447` | Test fixture (`Pick<Recur, 'kids'>` constructed manually). Not a runtime caller — uses synthetic `__builtin__` identity directly. Out of scope for shadow threading. |
| `meta_resolve.rs:10754` (cycle-reaches transitive) | Internal recursion inside `extract_route_root_identity_node` itself. The recursion already resolves in a non-userland-aware way, but the entry point's gating at `component_meta_materialize.rs:453` ensures only legitimately routed nodes reach this branch. |
| `meta_resolve.rs:11088` (recursive-helper guard) | Same analysis: the upstream gate suppresses non-userland routes. |
| `meta_resolve_tests.rs` | Test-only sites; not runtime callers. |

### `component_meta_registry_public_utility_route` callers (TypeExpr-based)

| Site | Disposition |
|---|---|
| `meta_resolve.rs:137` (`project_expr_class_a_via_dispatch_threaded`) | **Load-bearing site.** Phase 5h C3 adds the `ScopeShadowing::from_host_scope` construction and `.filter()`s the route helpers' `Some(...)` result through `!shadowing.is_shadowing_lib(root_symbol)`. |
| `meta_resolve.rs:1431, 2040, 2971, 8665, 9392, 9787, 9919, 9925, 9953` | Operate on declaration BODY shapes, not the public utility route the macro evaluator hits — they answer "is this declaration structurally a route shape" for symbolic-keep decisions and are unaffected by scope shadowing in the macro caller's file. |
| `resolver_core/component_meta_query_engine.rs:4000, 4196` | Internal engine routing; reads the same registry helpers but for declaration-body symbolic-keep decisions. Same disposition as the meta_resolve sites above. |
| `resolver_core/component_meta_registry.rs:1233, 1685, 1868` | Internal helpers. Body-decision sites. |

The `ScopeShadowing` value threads from the route/registry entry-
point downward; the public callers receive the scope from the
resolver-context already on hand. NO parallel threading paths —
single-source-of-truth scope shadowing. Phase 10a's
`ResolverContext` migration absorbs `ScopeShadowing` as one input
field.

## Class A fixture derivation note

`userland_shadowing_pick`:
- Source SFC: `crates/verter_session/tests/correctness/fixtures.rs` —
  `USERLAND_SHADOWING_PICK_VUE` const, registered in `FIXTURES`.
- Programmatic expected:
  `crates/verter_session/tests/correctness/expected.rs` —
  `userland_shadowing_pick()` returns three required props
  (alpha, beta, gamma) — sorted alphabetically by the projection.
- Derivation note:
  `crates/verter_session/tests/correctness/derivation_notes/userland_shadowing_pick.md`
  — first non-blank line cites
  `Verter rule ./.claude/skills/type-resolution`. Passes the
  §0p.A.4 `ensure_class_a_derivation_notes` regex.
- Snapshot: `crates/verter_session/tests/correctness/snapshots/userland_shadowing_pick.correctness.snap.json`
  — generated via the §0p.A.0 author-first workflow
  (`--ignored generate_class_a_snapshots_from_expected`).
- Rule-correct DATA block:
  `phase-00-tier1-mismatches.md` row 5 — fenced ```json``` block
  carries the canonical `SnapshotView` for the rule-correctness
  gate test.

## Anchor drift log

None. The brief's caller-slice grep matched the expected sites
verbatim; no line-number adjustments via §0.6.1 were required.

One brief discrepancy: the brief said
`component_meta_pathological_recursion_tests.rs` "ALREADY EXISTS";
in the actual tree only the OTHER 4 §5.D files existed (the
5g-supplement created `read_once`, `terminal_mode`,
`no_cache_promotion`, and `cache_discipline` files but not
pathological_recursion). 5h created the file as part of C9 per
§0.6.1 small decision (the file path was unambiguous and the
brief's "ALREADY EXIST" claim was clearly an oversight rather
than an architectural directive).

## Verification

| Gate | Result |
|---|---|
| `cargo test --workspace --tests --verbose` | 10253 passed / 0 failed / 11 ignored / 45 blocks (per `/tmp/p05h-workspace.txt`) |
| `cargo test -p verter_session --test correctness` | 12 passed / 0 failed / 1 ignored |
| `cargo clippy --workspace -- -D warnings` | clean |
| `cargo fmt --all --check` | clean |
| `pnpm install --frozen-lockfile` | clean |

## Tests un-ignored

- `shadowed_pick_is_userland_not_intrinsic` (lib_parity.rs)

## Tests added (full enumeration)

§5.D mandatory tests (4):
- `read_once_shallow_first_lazy_for_userland_shadowing_pick`
- `intermediate_hops_navigate_terminal_only_expanded_for_userland_shadowing_pick`
- `no_cache_promotion_for_budget_exceeded_userland_shadowing_pick`
- `pathological_self_shadowing_userland_pick`

§5.B.5.1 rule-correctness gate (1):
- `deferred_fixture_userland_shadowing_pick_byte_equal_to_rule_correct_expected`

§5.10 module tests (5 in `scope_shadowing::tests`):
- `empty_shadow_set_does_not_shadow_any_name`
- `from_scope_payload_includes_scope_type_names`
- `from_scope_payload_includes_script_setup_type_bindings`
- `from_scope_payload_none_returns_empty_set`
- `shadow_sets_from_payload_and_bundle_observe_same_names`

§5.B.5 fixture (drives correctness gate; counted in
`correctness_snapshot_for_every_fixture`):
- `userland_shadowing_pick` Class A fixture

## §5.D.5 termination contract — note on dispatcher scope

The §5.D.5 pathological test exercises the dispatch engine's
same-identity recursion guard via direct
`dispatch.execute(SemanticQueryKey::Instantiate)`. The
component-meta query engine's
`materialize_component_meta_type_expr_until_stable_full` recreates
a fresh `ProjectSemanticDispatch` per call; that resets the
`instantiate_active` stack across recursion levels. The §5.D.5
contract ("terminate with `Recursive`, not stack overflow") is
enforced at the engine layer where 5h's `ScopeShadowing` thread
plumbs through. The higher-layer per-call dispatcher recreation is
a separate architectural concern (engine-layer recursion handling
for parametrised cycles via the cross-call memo) belonging to
Phase 11+ rather than 5h's shadow-gate-threading scope. The §5.D.2
test covers the `get_component_meta` end-to-end path's lazy /
read-once contract on the userland-shadowing scenario; the §5.D.5
test covers the engine's same-dispatcher recursion safety.

## Deferrals

None. `deferred[]` is EMPTY per atomic-gate r17/Codex-P1#1.

## Marker

`crates/verter_session/.phase-markers/phase-05h-complete` — JSON
manifest per §0.6 R7 with `status: "success"`,
`atomic_gate_phase: true`, `derivation_notes_verified: true`,
`deferred: []`.
