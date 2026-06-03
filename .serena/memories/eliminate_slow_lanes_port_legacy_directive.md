# Eliminate Slow Lanes — Port Legacy to Dispatch (Owner Directive, 2026-05-31, STRENGTHENS forbid_slow_lane_tests)

Owner directive (escalation): **"I don't want slow lanes, please port any legacy or unnecessary functions/code."**

## Meaning
The goal is NOT to guard slow lanes — it is to **ELIMINATE** them. For every legacy/slow-lane
fallback that "shadows" the fast dispatch path: **port what it does onto dispatch (make dispatch
complete), then DELETE the legacy function.** The forbid-slow-lane TEST guards (see
`forbid_slow_lane_tests_standing_directive`) are the SAFETY HARNESS: once dispatch handles all
cases, the armed forbid-guard passes WITHOUT the slow lane being taken → it is safe to delete the
slow lane → the guard then becomes an absence assertion.

## Concrete live slow lane to eliminate first (the prepared-structural-substitution lane)
`instantiate_local_generic_ref_via_engine` (component_meta_query_engine/mod.rs:1095-1135) →
`apply_type_param_substitutions` (surface.rs:383) → `substitute_type_expr` (surface.rs:396).
Reached as `.or_else` fallback at route_keys.rs:215/465/599. Fast counterpart:
`instantiate_local_generic_ref_via_dispatch` (dispatch_helpers.rs:673, pure dispatch via
ProjectSemanticDispatch/raise_node_to_type_expr). codex flagged its deletion needs a
"dispatch-completeness cutover" (port the 3 fallback cases to dispatch, then delete the 3 slow-lane
fns). The landed forbid-guard (frontier @ 72e71883a) is the harness proving completeness.

## Standing application (this effort)
- For every remaining slow-lane / legacy / `.or_else(legacy)` fallback in the resolution path:
  scout the gap → codex BINDING consult on porting it to dispatch → port → delete the legacy fn →
  the forbid-guard / absence guard proves it's gone.
- Stages 5-7 (delete OXC rail, delete verter_parser resolve_type/, runtime inference) are
  legacy-elimination — apply the same aggressive port-then-delete discipline throughout, beyond the
  specifically-scoped deletions. NO legacy left alive; NO slow-lane fallback retained behind a guard
  when it can be ported + deleted.
- Don't ship a guard as the END state where elimination is feasible — the guard is a step toward
  deletion, not a substitute for it.
