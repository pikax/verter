# Phase 11b — `component_meta_query_engine.rs` folder split

## Summary

Phase 11b is a REORG-ONLY phase that converts the 11250-LOC
`component_meta_query_engine.rs` god module into a 9-file folder
module under `crates/verter_session/src/resolver_core/component_meta_query_engine/`.
Each commit moves a thematic cluster of free helpers, engine impl
methods, or the inline test module into a dedicated sibling. Per
§11b.0.6 char-reading boundary contract, public-surface symbols
remain re-exported from the parent `mod.rs` so the 8 cross-callsite
anchors documented in §11b.0.4 continue to resolve at the same path
without any caller-side edit.

The 11b.1 commit (renaming the file to a folder + atomically updating
the `phase_05l_engine_resolver_methods_deleted` arch-guard string
literal) was already in place at recovery time. Commits 11b.2 through
11b.10 were authored by this worker session.

Recovery taken: **CASE A** — the previous worker's WIP (1267-LOC
`surface.rs` + 9711→9984-LOC `mod.rs` diff) was sound apart from a
missing `prepared_substitution_key` import in the engine impl's
`use surface::{...}` block plus an unused `Hasher` carryover.
Surgical fix landed cleanly, full workspace tests passed, then the
WIP was committed as 11b.2 with no further structural changes.

## Files added / renamed

| Path | LOC | Purpose |
|---|---|---|
| `crates/verter_session/src/resolver_core/component_meta_query_engine/mod.rs` | 705 | Parent shell — struct definition, slow-lane guards, `new()`, `scope_payload_for_scope`, free fns (`local_type_symbol_metadata_for_known_source`, `empty_semantic_args`, `dispatch_member_for_root_symbol`, `instantiate_local_generic_ref_via_engine`), constants (`SEMANTIC_*`), DepSignature builders, `DirectPreparedDeclarationResolver`, `PreparedProjectionContext`, cache-key value types, and `mod` declarations + re-exports |
| `crates/verter_session/src/resolver_core/component_meta_query_engine/surface.rs` | 1265 | Free surface-projection helpers, prepared-substitution machinery, arc cache-key constructors |
| `crates/verter_session/src/resolver_core/component_meta_query_engine/helpers.rs` | 308 | Predicate/utility helpers (`is_package_canonical`, `strip_parens_expr`, `is_builtin_name`, `prepared_decl_keeps_raw_symbolic_non_object_alias`, etc.) |
| `crates/verter_session/src/resolver_core/component_meta_query_engine/shallow_preserve.rs` | 1252 | Shallow-preservation predicates, imported-route fast paths, deep slot/type ref resolution |
| `crates/verter_session/src/resolver_core/component_meta_query_engine/registry_decl.rs` | 656 | Imported registry symbol resolution, direct prepared declaration access, fuse/state/debug accessors, host/dispatch entry helpers |
| `crates/verter_session/src/resolver_core/component_meta_query_engine/prepared_surface.rs` | 1003 | Prepared root/member surface projection + host-cache publication |
| `crates/verter_session/src/resolver_core/component_meta_query_engine/route_keys.rs` | 758 | Route literal-key enumeration + direct utility shape projection |
| `crates/verter_session/src/resolver_core/component_meta_query_engine/routed_expr.rs` | 1577 | Routed expression projection, request-local caches, pick/member route projection, inherited-member projection, `type_surface_facts` |
| `crates/verter_session/src/resolver_core/component_meta_query_engine/tests.rs` | 3957 | Inline tests (47 `#[test]` functions, gated `#[cfg(test)]` via parent's `mod tests;`) |

Total: 11481 LOC across 9 files (was 11250 LOC in a single file pre-split).

`crates/verter_session/tests/architecture_guards.rs` was edited
once: the `phase_05l_engine_resolver_methods_deleted` discriminator
method was changed from `pub fn should_preserve_shallow_field_expr(`
(moved to `shallow_preserve.rs` in 11b.4) to
`pub fn new(host: &'a VerterHost)` — the engine constructor stays in
`mod.rs` per §11b.0.6 and is the only public engine method that does
not move into a sibling file.

`crates/verter_session/src/d_cutover_characterization_tests.rs` was
edited once: the `migrate_engine_lower_and_project_to_expanded_preserves_env`
test was rewritten to scan all sibling files in the
`component_meta_query_engine/` folder rather than just `mod.rs`,
because `materialize_member_surface_expr` (which carries the
`dispatch.lower_type_expr_in_scope` + `ProjectPath` literals checked
by the test) moved into `registry_decl.rs` in 11b.5.

## Final per-file LOC budget

All 8 production siblings stay under the 4000-LOC `god_module_size_budget`
(currently `#[ignore]` per parent §11.4 — phase 11e flips). No
allow-list entry is added. The largest production sibling is
`routed_expr.rs` at 1577 LOC.

`tests.rs` at 3957 LOC remains under the 4000-LOC threshold so the
budget guard does not require a test-fixture exemption either.

## Verification commands run + results

```
cargo test --workspace --tests --verbose
  → 10283 passed, 0 failed, 4 ignored, 45 result blocks

cargo test -p verter_session --test correctness
  → 18 passed, 0 failed, 1 ignored

cargo test phase_05l_engine_resolver_methods_deleted -p verter_session --test architecture_guards
  → 1 passed (green through every commit, including 11b.4 where the
    discriminator was updated atomically and 11b.5/11b.7/11b.8/11b.9
    where method moves continued)

cargo clippy --fix --allow-dirty --allow-staged --workspace -- -D warnings
  → finished cleanly (auto-fixes applied to baseline tech-debt
    warnings in unrelated files such as meta_resolve.rs; reverted
    out-of-scope to keep the 11b commits REORG-only)

cargo fmt --all
  → clean (reformatting limited to the 11b-touched files; unrelated
    fmt drift in meta_resolve.rs / d_cutover_characterization_tests.rs
    reverted as out-of-scope per §11b.7)

pnpm install --frozen-lockfile
  → clean (Done in 20.6s using pnpm v10.22.0)
```

## Recovery procedure

The previous worker died with a partially-staged 11b.2:
- `surface.rs` (1267 LOC, untracked)
- `mod.rs` (-1233/+28 lines from baseline)

Step R1: `cargo build -p verter_session --tests` failed with 2
errors (`prepared_substitution_key` not found at 2 callsites in
mod.rs). Step R2 case analysis: the WIP was structurally sound — the
2 callsites referred to a `pub(super)` symbol in `surface.rs` that
the WIP had moved but not added to mod.rs's `use surface::{...}`
block.

Surgical fix:
1. Added `prepared_substitution_key` to mod.rs's `use surface::{...}`
2. Removed the now-unused `Hasher` from `use std::hash::{Hash, Hasher}`
3. Removed two now-unused imports (`type_expr_references_substitutions`
   from the runtime path; `node_data_for`, `QueryError`, `SemanticNodeData`,
   `SurfaceView` from `crate::semantic_query`)
4. Re-added `type_expr_references_substitutions` under `#[cfg(test)]`
   so the inline test that calls `super::type_expr_references_substitutions`
   continues to resolve

After the surgical fix, `cargo test --workspace --tests --verbose`
passed (10283/0 failed/4 ignored), confirming the WIP was sound.
Committed as 11b.2 (`6b170b49`).

## Commit list (11b.2 through 11b.10)

| sha | subject |
|---|---|
| `6b170b49` | refactor(meta): extract surface-projection helpers to component_meta_query_engine/surface.rs |
| `ca31e359` | refactor(meta): extract miscellaneous predicates + utilities to component_meta_query_engine/helpers.rs |
| `9e1f8cbf` | refactor(meta): move shallow-preservation engine methods |
| `e66647b4` | refactor(meta): move registry/declaration engine methods |
| `2f1a97c2` | refactor(meta): move prepared-surface projection methods |
| `87e801c8` | refactor(meta): move route-key projection methods |
| `96dfcd0f` | refactor(meta): move routed-expression projection methods |
| `ce2c1cab` | refactor(meta): extract inline test module to component_meta_query_engine/tests.rs |
| (this commit) | chore(orchestrator): mark phase 11b complete |

## STOP encountered and resolved

**STOP B (architecture-guard string mismatch)** triggered during
11b.4: after moving `should_preserve_shallow_field_expr` into
`shallow_preserve.rs`, the `phase_05l_engine_resolver_methods_deleted`
guard's discriminator check failed because the surviving sentinel
method was no longer in `mod.rs`. Per §11b.0.5 the guard's string
literal was atomically updated in the same commit (11b.4) to
`pub fn new(host: &'a VerterHost)` — the engine constructor that
stays in `mod.rs` after the folder split. Workspace tests passed
post-update; arch guard verified green. STOP resolved.

**Sub-issue resolved during 11b.5**: the
`migrate_engine_lower_and_project_to_expanded_preserves_env`
characterization test (in `d_cutover_characterization_tests.rs`)
failed because `materialize_member_surface_expr` (which carries the
`dispatch.lower_type_expr_in_scope` + `ProjectPath` literal checks)
moved out of mod.rs. Test was updated to scan all sibling files in
the folder per §0.6.1 small-decision allowance.

## Deferred

Nothing. Phase 11b is in the orchestrator's `ATOMIC_GATE_PHASES`
allowlist; per r17 atomic-gate phases must have `deferred == []` and
`status == "success"`. Both invariants hold for this marker.

## Worktree HEAD at completion

`wt/phase-11b-engine-split` HEAD will be the marker commit after R7
lands (1 commit after `ce2c1cab`).
