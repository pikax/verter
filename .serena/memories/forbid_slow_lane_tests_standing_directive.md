# Forbid-Slow-Lane Tests — Standing Owner Directive (2026-05-31)

Owner directive: **"add forbid slow-lane tests whenever you can."**

## What it means
The `verter_session` component-meta query engine has a dispatch FAST lane (the
post-walker-deletion one-engine path) and a legacy SLOW lane (the walker/prepared
fallback being eliminated). A "forbid slow-lane test" arms a thread-local RAII guard
so that IF resolution falls to the slow lane it PANICS — proving the dispatch fast
lane handles the scenario on its own.

## Infrastructure (crates/verter_session/src/resolver_core/component_meta_query_engine/mod.rs ~539-634)
3 forbid-guards (RAII, thread-local `Cell<usize>` depth counters):
- `forbid_structural_slow_lane_for_tests() -> StructuralSlowLaneGuard`
- `forbid_direct_pick_routed_expr_slow_lane_for_tests()`
- `forbid_prepared_structural_substitution_slow_lane_for_tests()`
Checkers: `structural_slow_lane_forbidden_for_current_thread()` /
`direct_pick_routed_expr_slow_lane_forbidden_for_current_thread()` /
`prepared_structural_substitution_slow_lane_forbidden_for_current_thread()`.
Panics: `assert_*_slow_lane_allowed()` panic if the slow lane is entered while forbidden.

## Pattern
```rust
#[test]
fn scenario_uses_fast_lane_not_slow() {
    let _guard = forbid_direct_pick_routed_expr_slow_lane_for_tests(); // arm
    // run Pick/Omit/member-route/structural-subst resolution via dispatch
    // (panics if it falls to the slow lane)
    assert!(/* concrete correct result: present keys + absent keys */);
}
```
Existing examples: tests.rs:688/866/996/1129/1514/2418, meta_tests.rs:8900/9002/9107.

## Standing application (Stage 4b F12 + Stage 5-7)
- New Pick/Omit/member-route/structural-substitution dispatch tests should ARM the
  matching forbid-guard so they prove fast-lane sufficiency.
- Replace any vacuous "solver-free / shallow-route" assertion (e.g. `assert_eq!(0u32,0)`)
  with arming the forbid-guard (the real signal) + a concrete result assertion.
- Do it in-batch (not deferred); it complements codex's coverage-loss findings.
