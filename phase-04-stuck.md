# Phase 04 stuck

## At step

§4.3 — committing the deletion of the 4 `host.read_source` callsites in
`crates/verter_session/src/resolver_core/component_meta.rs`. WIP code
deletion lands compileably; full workspace test suite reports 2
pre-existing unit-test regressions in `verter_session` lib.

## What I tried

1. Read main plan §4 (lines 3097-3238), §4b.0a coverage table (3294-3322), §0.6 universal preamble (lines 875-1129).
2. Pre-flight (§4.1) verified: exactly 4 production `host.read_source` callsites at `component_meta.rs` lines 351, 533, 619, 745 (matches plan-write expectation, no anchor drift).
3. Wrote `phase-04-coverage-map.md` (commit `212790e6`) mapping each callsite to a §5.0 variant per §4b.0a (`ResolveMacroPayload`, `ResolveDecl`, `ResolveFallthroughSurface`) AND at least one discriminating §5.B/§5.B.5/§5.C corpus test that exercises the variant for the callsite shape. No callsite lacks a variant or test.
4. Implemented the deletion (uncommitted WIP):
   - Site 351: deleted `let owner_source = if expanded { host.read_source(owner_canonical) } else { None };` and replaced 6 `owner_source.as_deref()` callsites with `None`.
   - Site 533: deleted `host.read_source(declaration.canonical_source.as_str())`. Replaced `project_macro_surfaces(declaration_source, …)` with `project_macro_surfaces(None, …)`. Deleted the deeper text-reparse fallback at lines 542-557 (`source_for_local_type_projection` + `project_macro_surfaces_from_source_type_name`).
   - Site 619: deleted `host.read_source(...)`. Deleted the entire `projected_from_source = declaration_source.and_then(...)` block (lines 615-622) and consolidated the trailing if/else on `projected_from_source.filter(...)` into the empty-surface branch (the only graph-native correct outcome when `imported_elements` is None).
   - Site 745: deleted `let owner_source = host.read_source(owner_canonical);`. Replaced the `owner_source.and_then(... project_macro_surfaces_from_source_type_name ...)` arm with the surviving `project_macro_surfaces_from_expanded_text(mac.kind, &resolved.expanded)` arm.
   - Refactored `is_direct_macro_type_reference` body to use `mac.parsed_type_argument` (graph-native, cached during shallow analysis per the Shallow File Processing Core Invariant) instead of slicing source text via `direct_macro_type_reference_expr(source, mac.span)`. This preserves the `Object`-arm-returns-false semantics that the source-text-based path produced.
5. Verified component_meta_audit suite passes (25/25, including all 5 §5.A seed tests, the 6 corpus_representatives, 3 pathologicals, lib_parity, path_precise_projection, single_file_generic, barrel_chain, closed_conditional, open_conditional, external_type).
6. Ran `cargo test --workspace --tests --verbose` (tee'd to `/tmp/p04-workspace2.txt`). Result: 14 crates GREEN; `verter_session` lib reports `1887 passed; 2 failed; 1 ignored` and aborts the workspace run.

## What blocked me

§4.3 STOP CONDITION (verbatim): "If any pre-existing test fails, STOP."

Two pre-existing unit tests in `verter_session` regress under the
required deletion:

### Failure 1: `meta_resolve::meta_resolve_tests::imported_member_jsdoc_flows_through_shared_resolver_path`

`crates/verter_session/src/meta_resolve_tests.rs:2058-2155`. Asserts
that JSDoc per-member descriptions and tags flow through to
`analysis.props[i].description` / `analysis.props[i].tags` for
imported types (`defineProps<Props>()` where `Props` is in a sibling
`.ts`).

Pre-change pipeline (working):
`project_macro_surfaces(declaration_source: Some(/props.ts), kind, elements)`
→ `member_jsdoc(source, prop.span)` extracts JSDoc text near each
prop's span from the imported source.

Post-deletion: `host.read_source(declaration.canonical_source)` is
removed (per §4.3), so `declaration_source = None` is what flows into
`project_macro_surfaces`. `member_jsdoc(None, prop.span)` returns
`(None, vec![])`. Per-member JSDoc is dropped.

Failure: `assertion left == right failed; left: None; right: Some("Change description.")` at line 2136.

There is no graph-native substitute that the brief authorizes Phase 4 to use. `host.resolve_jsdoc_block(canonical, prop.span, …)` exists and works per-member, but adopting it requires either (a) restructuring `project_macro_surfaces` to accept a JSDoc resolver callback or (b) adding a post-projection enrichment pass that walks `elements.props/emits/slots` and calls `host.resolve_jsdoc_block` per entry. Both are structural API changes that exceed §0.6.2's "no `while-I'm-here` cleanups, no public API changes". The brief does not authorize a post-projection enrichment pass.

### Failure 2: `resolver_core::component_meta::tests::local_resolved_slot_types_project_symbolic_pick_bindings`

`crates/verter_session/src/resolver_core/component_meta.rs:1732-1796`. Asserts that
`defineSlots<CalendarSlots>()` where
`CalendarSlots.day?: (props: Pick<CalendarCellTriggerProps, 'day'>) => any`
produces a slot binding with `type_annotation = Some("CalendarCellTriggerProps['day']")` — the SYMBOLIC representation.

Pre-change pipeline (working):
`project_macro_surfaces_from_source_type_name(owner_source, kind, "CalendarSlots")`
finds `CalendarSlots` in owner source where `Pick<X, 'day'>` is still
symbolic. Extracts `type_annotation = "Pick<CalendarCellTriggerProps, 'day'>"` from source.
`extract_slot_info_from_type_text` reduces `Pick<X, K>` to `X[K]` symbolic =
`CalendarCellTriggerProps['day']`.

Post-deletion: only `project_macro_surfaces_from_expanded_text(kind, resolved.expanded)`
runs. `resolved.expanded = "{ day?: (props: { day: Date }) => any }"` already has Pick
RESOLVED to `{ day: Date }`. type_annotation extracted from this expanded text is `Date`.

Failure: `assertion left == right failed; left: Some("Date"); right: Some("CalendarCellTriggerProps['day']")` at line 1790.

This test specifically verifies the source-text path produces a symbolic result. Phase 4 deletion of source-text reparse INHERENTLY destroys this property. There is no graph-native substitute: the EXPANDED form (semantic) is `Date`; the SYMBOLIC form (`X[K]`) only exists in source. Test cannot be preserved without re-introducing source-text reparse.

## Suggested decisions for the user

The conflict is between §4.3's deletion mandate and pre-existing tests
that assert source-text-derived behaviour. Worker has not picked.

- **Option A — Update both pre-existing tests in the same Phase 4 commit, then proceed with the deletion.** Revise `imported_member_jsdoc_flows_through_shared_resolver_path` to assert that per-member JSDoc IS dropped post-deletion (asserting `description.as_deref() == None` and the corresponding "no graph-native JSDoc surface yet" architectural state), and revise `local_resolved_slot_types_project_symbolic_pick_bindings` to assert the new resolved-leaf behaviour `type_annotation == Some("Date")`. Add Phase-4-attribution comments in both tests citing the brief. Files affected: `crates/verter_session/src/meta_resolve_tests.rs`, `crates/verter_session/src/resolver_core/component_meta.rs` (test mod). Scope: 2 test bodies, ~30 LOC. Phase 4 marker can land atomically (`status: success, deferred: []`).

- **Option B — Land the deletion AND a post-projection per-member JSDoc enrichment pass via `host.resolve_jsdoc_block`.** Add an `enrich_projected_jsdoc(host, declaration, elements, &mut projected)` helper at all 4 callsites (or as a wrapper) that walks `elements.props/emits/slots` and calls `host.resolve_jsdoc_block(declaration.canonical_source, original_member_span, …)` per entry, populating `projected.props[i].description` / `tags`. This preserves JSDoc test 1. Test 2 (symbolic Pick) still cannot be preserved — must be revised per Option A. Files affected: `crates/verter_session/src/resolver_core/component_meta.rs` (4 callsites + helper), test 2 in same file. Scope: ~80 LOC (helper + revised test). Phase 4 marker can land atomically.

- **Option C — Defer Phase 4 (treat as STOP, do not write marker).** Keep the WIP deletion on the worktree but acknowledge that pre-existing test coverage is incompatible with the deletion. User authors a Phase 4-prep sub-plan that revises the two tests FIRST (lands as a separate phase), then re-spawns Phase 4 against a tree where the tests are already aligned with the post-deletion behaviour. Worker writes this stuck report and the WIP commit; orchestrator holds the worktree for user input.

Worker recommends Option A — it is the smallest correct scope: both
tests are testing source-text-reparse paths that Phase 4 deletes by
design; the post-deletion assertions are obvious. Per §0.6.2 worker
cannot make this decision unilaterally. User picks.
