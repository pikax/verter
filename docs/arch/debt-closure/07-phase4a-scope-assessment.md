# Phase 4A — Scope Assessment (architectural-debt-closure rev 11.3)

**Status:** Scope analysis. Phase 4A's broader walker-family deletion was assessed
during the same session that landed Phase 4B (`20d85e15`); the deletion was
determined to be a substantively larger refactor than the plan's "mechanical
migration of ~50 callers" framing acknowledges, and is deferred to a dedicated
follow-up session.

This document captures what was assessed, why the in-session deletion attempt
was paused, and the concrete entry-point inventory that a follow-up agent
should use.

## Discriminator-test status (sub-task 4A.0)

All four Phase 4A discriminator fixtures **already pass on the
post-Phase-4B tree**:

| Test | Status |
|---|---|
| `evaluate_types_cross_file_recursive_alias_through_reexport_preserves_recursive_transport` | ok |
| `get_component_meta_uses_default_type_parameters_when_generic_args_are_omitted` | ok |
| `resolve_component_meta_keeps_deep_imported_registry_branches_shallow` | ok |
| `resolve_component_meta_does_not_publish_package_helpers_from_imported_local_registry_entries` | ok (Phase 4B Rule 1 covers package-backed) |

The legacy walker family — the very functions the plan's sub-task 4A.5 calls
to delete — is what makes them pass. Sub-task 4A.0's "FAIL-FIRST" framing
holds in the contrapositive: removing the walker without replacement
re-introduces the failures.

## Walker-family scope (sub-task 4A.5)

The tombstone targets and their function-definition + caller counts (from
`rg`) are:

| Function | Defined in | Caller mentions in `crates/` |
|---|---|---|
| `solve_expr_type_expr` | resolver_core/component_meta_query_engine.rs:4018 | 16 |
| `expand_local_generic_ref_expr` | resolver_core/component_meta_query_engine.rs:4045 | 20 |
| `prepared_type_param_substitutions` | resolver_core/component_meta_query_engine.rs:6459 | 10 |
| `projected_member_surface_keys` | resolver_core/component_meta_query_engine.rs:3529 | 14 |
| `projected_string_literal_keys[_inner]` | resolver_core/component_meta_query_engine.rs:3360 / :3374 | 5 |
| `materialize_component_meta_member_surface_expr` | meta_resolve.rs:7688 | 23 |
| `materialize_component_meta_macro_shape_member_types` | meta_resolve.rs:7704 | 6 |
| `materialize_member_route_from_alias_body_in_owner_scope` | meta_resolve.rs:1566 | 5 |
| `imported_component_meta_materialization_scope` | meta_resolve.rs:916 | 14 |
| `expr_has_transitively_recursive_generic_root` + `named_decl_body_reaches_cycle` | meta_resolve.rs:955 / :992 | 9 |
| `type_expr_needs_projection_rescue` | meta_resolve.rs:4406 | 14 |
| `component_meta_type_expr_improves` | meta_resolve.rs:8816 | 21 |

Total: ~150 caller-site mentions across two files (`meta_resolve.rs` and
`component_meta_query_engine.rs`), each ~11 KLOC. The walker family itself
spans roughly 3000–5000 LOC of intricate, mutually-recursive logic.

The plan's "mechanical migration of ~50 callers" framing collapses two
separable concerns:

1. **External entry points** in the resolve pipeline — the pipeline's
   field-rescue functions invoke walker entry points to fix up TypeExprs.
   These are the genuinely-distinct caller sites; estimated count
   10–20 from inspection.
2. **Internal recursion** — the walker family is mutually recursive, so
   most of the 150 mentions are walker-into-walker calls that go away
   when the walker is replaced as a unit.

A real deletion lands in two coordinated steps:

1. Implement caller-side iteration helpers (Gaps 1, 2, 3) that close the
   four discriminator fixtures using dispatch alone.
2. Migrate every external entry point to call the new helpers, then delete
   the walker family.

Without both halves landing in the same change, the discriminator tests
fail. The plan's rev 11.3 decision was that 4A.1–4A.3 add the helpers and
4A.5 deletes — but each sub-task's payload is non-trivial, and 4A.5 in
particular requires verifying every external entry-point migration against
the discriminator fixtures plus the broader 1698-test suite.

## Why the in-session attempt paused

Phase 4B landed cleanly — the user-facing regression closure is shipped.
Attempting Phase 4A's deletion in the remaining session budget would
require either:

- **Renaming** the tombstoned functions to non-tombstoned names. This
  satisfies `rg` checks literally but moves logic around without removing
  it, which is the "stub-satisfies-gate" pattern that
  `~/.claude/CLAUDE.md` explicitly forbids.
- **A real deletion + caller-side iteration migration** of ~10–20 entry
  points + recursion-removal verification across 1698 tests. This is
  realistically a dedicated session-length task; rushing it risks
  committing a half-broken codebase.

A WIP commit in the middle of a real deletion would also leave the tree in
a broken state where the discriminator fixtures fail. The plan's directive
"commit a WIP if you run out of context" presumes the WIP is internally
coherent (e.g. Step N partially done but compiling); a WIP that breaks the
discriminator tests is not coherent.

## Entry-point inventory (for the follow-up session)

The walker family is invoked from the following resolve pipelines in
`meta_resolve.rs`. A follow-up should treat each as a discrete migration:

1. **Field rescue: `solve_evaluated_field_inline_imported_meta`** (line
   ~1900–2900). Calls `imported_component_meta_materialization_scope`,
   `materialize_member_route_from_alias_body_in_owner_scope`,
   `materialize_component_meta_member_surface_expr`,
   `component_meta_type_expr_improves`,
   `type_expr_needs_projection_rescue`. This is the largest single entry
   point.
2. **Macro-shape member rescue: `materialize_component_meta_macro_shape_member_types`**
   (line ~7704). Used for slot/prop binding rescue from macro shapes.
3. **Public-prop materialization** (line ~3000–3500). Calls
   `materialize_component_meta_member_surface_expr` for prop-type
   refinement.
4. **Route rescue: `materialize_member_route_from_alias_body_in_owner_scope`**
   (line ~1566). Used by field rescue.
5. **Frontier expansion in resolver_core** — `solve_expr_type_expr`,
   `expand_local_generic_ref_expr` are wrappers over
   `dispatch.lower_type_expr_in_scope_with_mode` plus filtering. Their
   ~6 external call sites (after deduping internal recursion) can take
   the dispatch composition inline.

## Required helpers for the deletion (sub-tasks 4A.1/4A.2/4A.3)

**Gap 1 — multi-scope iteration helper.** Replaces
`imported_component_meta_materialization_scope` callers. Iterates `[owner,
preferred_scope, declaration_source]` feeding
`dispatch.lower_type_expr_in_scope_with_mode` until a non-Unknown result.

**Gap 2 — default-type-parameter rewriter.** Replaces
`expand_local_generic_ref_expr` for the default-args case. When caller
observes `Ref { name, type_arguments: [] }` and
`prepared_for(name).type_parameters[i].default.is_some()`, rewrites to
`Ref { name, type_arguments: [defaults] }` BEFORE calling dispatch.

**Gap 3 — visited-set worklist with cycle detection.** Replaces the
chained `materialize_member_route_from_alias_body_in_owner_scope` +
`materialize_component_meta_member_surface_expr` rescue loop. Uses
`HashSet<SemanticNodeId>` of resolved nodes; cycle detected via re-encounter;
defensive fuse ≥ 4096 hops emits structured `component_meta_trace_custom!`
diagnostic.

## Concrete state after this session

- `20d85e15 refactor(meta): Phase 4B — apply_component_meta_resolution_policy`
  closes B1.
- `fa073650 refactor(meta): Step 3 closure — migrate 10 engine-local
  caches to ProjectTypeStore` closes Debt 3 + the Step 3 final 10 DBs.
- 1698/1698 verter_session lib tests pass.
- 231/231 `@verter/component-meta` vitest tests pass.
- All four Phase-4A discriminator fixtures pass.
- Workspace cargo clippy + fmt clean.

Phase 4A's broader walker-family deletion (sub-tasks 4A.1/4A.2/4A.3 helper
implementation + 4A.4 verification + 4A.5 deletion) remains as future
work. The follow-up session should consume this scope assessment as its
starting input.
