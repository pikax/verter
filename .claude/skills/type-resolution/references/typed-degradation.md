## Typed Degradation And Completeness Contract (CRITICAL)

Semantic degraded states are part of the type system contract. They must be typed, propagated, and observable.

- `TypeExpr::Unknown(UnknownValue)` is not a carrier for semantic control flow. Misses, unsupported intrinsics/operators, alias cycles, recursion sentinels, budget exits, unstable-state exits, and bridge-depth exits must use typed variants or typed sidecar state. Do not encode them as strings such as `"budgetExceeded(...)"` / `"aliasCycle(...)"` and do not recover them with `starts_with` / regex checks.
- `Unknown` is allowed only for a genuine unknown type value with provenance explaining why the producer could not represent it. If the producer knows this is `Unsupported`, `BudgetExceeded`, `Recursive`, `Miss`, or `Unstable`, use that state instead.
- Public query envelopes must preserve completeness. `Complete` means the required inputs were available, current, and no budget/unsupported/unstable branch affected the answer. A query may return `Complete(None)` only when absence itself was proven under the current facts; missing analysis, stale cache data, unavailable providers, unsupported operators, and budget exits must surface as `Unavailable`, `Partial`, or a typed degraded result.
- Degraded results may be displayed and returned to callers, but must not be promoted into warm shared caches as complete answers.

Guards: `macro_impacting_constructs_fail_lowering_not_silent_skip` (`crates/verter_session/tests/cases/architecture_guards.rs` + `crates/verter_session/src/owned_artifacts/eval_program_tests.rs`) pins fail-loud lowering over silent skip; `audit_publishes_member_edge_with_published_field_provenance_at_macro_boundaries` (`crates/verter_session/src/component_meta_audit/mod_tests.rs`) pins published-field provenance at the macro boundaries. Typed degraded-state propagation beyond those two boundaries is not yet guarded end to end.


### Producer fact guarantees

Producers state their answer's quality as a `FactResult` (`semantic_query/fact_result.rs`) derived from their own typed outcome; the arm decides, the causes explain.

- **Flow return** (`project_semantic_dispatch/flow_return_fact.rs`): an undegraded evaluated return over a read that observed no partiality is `Complete`. A degraded success (`FlowReturnDegradation`), or an undegraded value whose read observed partiality, is `Approximate` — the documented usable interpretation is the evaluated return itself, with exact siblings around a positional marker (`FlowReturnUninferred`), a complete member set whose types may be unverified (`FlowReturnUnverified`, which is where an unapplied write lands), or the checker's budget recovery (`OperationBudget`). A typed `FlowReturnFailure` or a surfacing hold is `Unavailable` with `FlowReturnNoSurface`. A read that observed cancellation or a superseded/torn view is an `ExecutionAbort`, not a fact.
- **Scoped flow probes**: a flow read whose consumer may decline it runs under `ProjectSemanticDispatch::observe_flow_read`, and the consumer states `adopt` (its observations join the enclosing build) or `discard` (a fall-through route owns the answer). A declined probe never marks its enclosing build partial; an adopted one never hides its partiality.
- **Conditional reading** (`project_semantic_dispatch/conditional_decision.rs`): `NotConditional`, `Reduced` and a conditional the checker keeps (`Deferred`) are all `Complete`. A conditional the query could not finish is `Unavailable` with `UndecidedConditional` plus the read's own classes, its error's classification, or same-path recursion; a partial read's value is never offered as an approximate reading. Consumers relate only exact readings.
- **Surfaces**: `SurfaceResolution::into_fact` maps a closed surface, an open presence-only domain and `NoSurface` to `Complete`; an incomplete claim with a usable subset is an approximate presence-only domain, without one it is unavailable.
