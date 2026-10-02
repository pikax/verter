## Typed Degradation And Completeness Contract (CRITICAL)

Semantic degraded states are part of the type system contract. They must be typed, propagated, and observable.

- `TypeExpr::Unknown(UnknownValue)` is not a carrier for semantic control flow. Misses, unsupported intrinsics/operators, alias cycles, recursion sentinels, budget exits, unstable-state exits, and bridge-depth exits must use typed variants or typed sidecar state. Do not encode them as strings such as `"budgetExceeded(...)"` / `"aliasCycle(...)"` and do not recover them with `starts_with` / regex checks.
- `Unknown` is allowed only for a genuine unknown type value with provenance explaining why the producer could not represent it. If the producer knows this is `Unsupported`, `BudgetExceeded`, `Recursive`, `Miss`, or `Unstable`, use that state instead.
- Public query envelopes must preserve completeness. `Complete` means the required inputs were available, current, and no budget/unsupported/unstable branch affected the answer. A query may return `Complete(None)` only when absence itself was proven under the current facts; missing analysis, stale cache data, unavailable providers, unsupported operators, and budget exits must surface as `Unavailable`, `Partial`, or a typed degraded result.
- Degraded results may be displayed and returned to callers, but must not be promoted into warm shared caches as complete answers.

Guards: `macro_impacting_constructs_fail_lowering_not_silent_skip` (`crates/verter_session/tests/cases/architecture_guards.rs` + `crates/verter_session/src/owned_artifacts/eval_program_tests.rs`) pins fail-loud lowering over silent skip; `audit_publishes_member_edge_with_published_field_provenance_at_macro_boundaries` (`crates/verter_session/src/component_meta_audit/mod_tests.rs`) pins published-field provenance at the macro boundaries. Typed degraded-state propagation beyond those two boundaries is not yet guarded end to end.

