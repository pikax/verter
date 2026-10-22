# Retired request-work fuses

No field, store, hook, counter or trace was added. The frontier visit
counter (`ResolutionCounters::frontier_symbol_visits`), now read only by tests, is
OPTIONAL and compiled under `cfg(test)` only; the unread
`builder_expansion_steps` budget and counter were removed. The rows below
classify the state this change kept or rerouted.

Removed outright, with no successor state: `FuseBudgets::projection_op_count`
and `FuseState::projection_ops_executed` (the component-meta engine reads the
request's `RequestBudget` instead); `FuseBudgets::member_surface_recursion_depth`
with `FuseState::current_member_recursion_depth`, and
`FuseBudgets::union_member_explosion` with `FuseState::union_members_processed`
(no production caller consulted them); `HostConfig::external_resolution_step_budget`
(no production path read it); `MAX_EXTERNAL_TYPE_RESOLVE_STEPS`,
`ResolutionBudgets::frontier_symbol_visits` and
`ExternalTypeFrontier::budget_failure` (the frontier walks each
`(canonical_id, exported_name)` pair once and has no visit cap); and
`COMPONENT_META_MAX_SYMBOLIC_STEPS` (quoted in an error message, bounding
nothing).

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `FuseBudgets` wildcard-route, imported-root and registry-deepening fan-out rails and their `FuseState` counters | Component-meta route and registry fan-out refusal | REQUIRED-budget | One engine (one request) | `resolver_core/fuses` | always |
| `FuseState::trips` | The `fuse_tripped` component-meta trace and `has_fuse_tripped` (observation only; refusals come from the fan-out checks, which mark the request result budget-partial on a trip) | OPTIONAL | One engine (one request) | `resolver_core/fuses` | `semantic-observe` |

## verter_session

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `ComponentMetaQueryEngine::projection_op_budget_exhausted` | Heritage, route-scoped, intrinsic and expanded-node projections skip once the request's operation account is exhausted, marking the request result `BUDGET_EXCEEDED`-partial so the surface is refused warm admission | REQUIRED-budget | One request | `resolver_core/component_meta_query_engine/engine_accessors` | always |
