# Phase 4b worker report

**Branch:** `wt/phase-04b-text-fallback-elimination`
**Base commit at spawn:** `513bc8d2` (phase-04-complete)
**Work head before marker:** `4917214d`
**Marker:** `chore(orchestrator): mark phase 04b complete` (atomic-gate, status: success, deferred: [])

## Summary

Phase 4b §4b.1–§4b.5a — eliminate ALL remaining text/source-reparse
fallback paths in the resolver core. Phase 4 cleared the four
`host.read_source` callsites in `component_meta.rs`; Phase 4b clears
the rest:

- The `DeclarationMetadataResolver::read_source` trait method itself
  (declared in `resolver_core/declaration_metadata.rs:23`) plus its
  three production impls (`host_resolve.rs`, `meta_resolve.rs`,
  `resolver_core/component_meta_query_engine.rs`) and three test
  impls (`declaration_metadata.rs:578`, `component_meta.rs:1157`,
  `component_meta.rs:1303`).
- The three production source-reading callsites in
  `resolver_core/declaration_metadata.rs` (former lines 184, 261,
  308). Each was migrated to a graph-only path
  (`resolve_local_type_symbol_metadata`).
- The three text-projection helpers — `source_for_local_type_projection`
  (in `component_meta.rs`), `project_macro_surfaces_from_expanded_text`
  (in `surface_projector.rs`), `project_macro_surfaces_from_source_type_name`
  (in `surface_projector.rs`) — and the test instrumentation around
  them.
- Two surviving production callers of
  `project_macro_surfaces_from_expanded_text` in `component_meta.rs`
  (former lines 879 and 3731). The first-pass loop at line 879 was
  deleted; the second-pass loop's gate was widened to cover purely-
  local non-emit macros via `host.resolve_owner_local_macro_surface`
  graph-natively. The `macro_has_authoritative_resolved_local_surface`
  helper at line 3731 was replaced with a graph-equivalent
  (`projectable_owner_local`).

The three architecture guards
(`no_read_source_in_declaration_metadata`,
`no_text_based_macro_surface_projection_helpers`,
`no_macro_string_heuristics_in_resolver_core`) are un-ignored and
pass.

## Per-commit summary

| SHA | Title |
|---|---|
| `252dbe55` | `test(meta): graph-based decoupling tests for declaration_metadata fallbacks` (commit 1, §4b.2 — RED tests) |
| `053aa817` | `refactor(meta): replace read_source at declaration_metadata.rs:184 with graph` (commit 2, §4b.3) |
| `c5073e6c` | `refactor(meta): replace read_source at declaration_metadata.rs:261 with graph` (commit 3, §4b.3) |
| `4c57a88f` | `refactor(meta): replace read_source at declaration_metadata.rs:308 with graph` (commit 4, §4b.3) |
| `4c8dcdc0` | `refactor(meta): delete DeclarationMetadataResolver::read_source trait method` (commit 5, §4b.3) |
| `8cf0cb86` | `refactor(meta): delete source-text projection helpers (graph-only resolver)` (commit 6, §4b.4) |
| `da48b191` | `test(meta): verify graph-only resolver invariant after Phase 4b deletions` (commit 7, §4b.5) |
| `316783ac` | `test(arch): un-ignore 3 Phase-4b guards after deletion` (commit 8, §4b.5a) |
| `4917214d` | `chore(meta): apply cargo fmt after Phase 4b deletions` (final fmt) |

## §4b.1 pre-flight verification (resolved at spawn time)

- 4b.1.1: `grep -n "host.read_source" crates/verter_session/src/resolver_core/component_meta.rs`
  → 0 production matches (Phase 4 deletions held).
- 4b.1.2: `grep -n "read_source" crates/verter_session/src/resolver_core/declaration_metadata.rs`
  → trait def at line 23, 3 production callsites at lines 184/261/308,
  test impl at line 578, plus test instrumentation at 554/776/806.
- 4b.1.3: text-projection helpers verified at:
  - `component_meta.rs:3796` (`source_for_local_type_projection`).
    Drift from §4b.1.3's plan-write expectation of line 3638 (within
    §0.6.1 ±50-line tolerance).
  - `surface_projector.rs:172` (`project_macro_surfaces_from_expanded_text`).
  - `surface_projector.rs:187` (`project_macro_surfaces_from_source_type_name`).
- 4b.1.4: production callsites of text-projection helpers in
  `component_meta.rs`:
  - line 16 (import of `_from_expanded_text`)
  - line 879 (production caller of `_from_expanded_text` — kept by
    Phase 4 as transitional; deleted by Phase 4b §4b.4)
  - line 3731 (production caller in
    `macro_has_authoritative_resolved_local_surface` — also deleted)
  - test impls at lines 1113/1228 (test impls in `#[cfg(test)] mod`)
  - doc-comment references at lines 549/623/759/766/1115/3573 — all
    in test code or doc comments, none feeding production logic.

## Callsites migrated (production `read_source` →
graph-only)

| File | Former line | Function | Replacement |
|---|---|---|---|
| `resolver_core/declaration_metadata.rs` | 184 | `resolve_type_declaration` (after `get_export_span_follow_reexports`) | `resolve_local_symbol_details(... source=None)` — graph metadata only |
| `resolver_core/declaration_metadata.rs` | 261 | `resolve_type_declaration` (reexport-chain follow fallback) | direct call to `resolve_local_symbol_details(... source=None)` for the chained leaf — graph metadata only |
| `resolver_core/declaration_metadata.rs` | 308 | `resolve_local_type_declaration` | `resolve_local_type_symbol_metadata` for kind/span; `text=None` |

Production callsites of `host.read_source` in `resolver_core/`
post-Phase-4b: zero.

## Trait method + impls deleted

- Trait method `pub fn read_source(&self, canonical_source: &str) -> Option<String>`
  on `DeclarationMetadataResolver` (former
  `resolver_core/declaration_metadata.rs:23`).
- Production impls (3): `host_resolve.rs:82`,
  `meta_resolve.rs:12169`, `resolver_core/component_meta_query_engine.rs:5612`.
- Test impls (3): `resolver_core/declaration_metadata.rs:578`
  (`FakeResolver` — also drops `sources` and `read_source_calls`
  fields), `resolver_core/component_meta.rs:1157` (`TestHost`),
  `resolver_core/component_meta.rs:1303` (`CombinedSurfaceTestHost`).

## Functions deleted (§4b.4)

- `source_for_local_type_projection` (`component_meta.rs`).
- `project_macro_surfaces_from_expanded_text` (`surface_projector.rs`).
- `project_macro_surfaces_from_source_type_name` (`surface_projector.rs`).
- `macro_has_authoritative_resolved_local_surface` (`component_meta.rs`).
- `resolved_local_type_expr_can_drive_authoritative_projection` (`component_meta.rs`).
- Test instrumentation: `PROJECT_MACRO_SURFACES_FROM_SOURCE_CALL_COUNT`
  thread-local + `_inc`/`_count`/`reset_*` helpers.

Plus 4 surface_projector unit tests removed (their behaviour
contracts are covered by integration tests in `meta_resolve_tests`
and `component_meta_audit`).

## §4b.5 invariant verification (post-deletion)

```
$ grep -rn "read_source\b" crates/verter_session/src/resolver_core/ --include='*.rs'
(empty)

$ grep -rn "source_for_local_type_projection\|project_macro_surfaces_from_source_type_name\|project_macro_surfaces_from_expanded_text" crates/verter_session/src/ --include='*.rs'
(empty)

$ grep -rn "contains(\"defineProps\"\|contains(\"defineEmits\"\|contains(\"defineSlots\"\|contains(\"defineModel\"" crates/verter_session/src/resolver_core/ --include='*.rs'
(empty)

$ cargo test -p verter_session --test correctness 2>&1 | tail -1
test result: ok. 18 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.35s
```

All four invariant checks pass.

## §4b.5a guards un-ignored

```
running 14 tests
test no_read_source_in_declaration_metadata ... ok
test no_text_based_macro_surface_projection_helpers ... ok
test no_macro_string_heuristics_in_resolver_core ... ok
... (other guards unchanged) ...
test result: ok. 12 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out
```

The remaining `ignored` entries are unrelated (`god_module_size_budget`
gated on `phase-11`, `no_unbounded_recursion_in_resolver_core` gated
on `phase-05l`).

## Anchor drift log

- `component_meta.rs` line numbers shifted ~150 lines through Phase
  4b's deletions. The §4b.1.3 plan-write anchor at line 3638 for
  `source_for_local_type_projection` resolved to 3796 at spawn time
  (within the §0.6.1 ±50-line tolerance after re-checking adjacent
  lines).
- `declaration_metadata.rs:184/261/308` matched plan-write expectation
  exactly.
- `surface_projector.rs:172/187` matched plan-write expectation.

## Test fixture migration

The `TestHost`/`CombinedSurfaceTestHost` test fixtures in
`component_meta.rs` lost their `source: String` field; `TestHost`
gained `owner_local_macro_surfaces: BTreeMap<String, ProjectedMacroSurfaces>`
to mirror the graph-native production API. Eight unit tests in
`resolver_core::component_meta::tests` migrate from "first-pass
text projection" semantics to "host returns precomputed graph
surface" — they configure `projectable_owner_local_roots` plus
`owner_local_macro_surfaces` to drive the surviving graph-native
projection path.

Two existing integration tests had `text` assertions updated to
reflect the post-Phase-4b contract (`text` field is always `None`):
- `imported_jsdoc_flows_through_shared_resolver_path` (in `meta_resolve_tests.rs`)
- `resolved_type_registry_preserves_pre_expansion_declaration_metadata` (in `meta_resolve_tests.rs`)
- `resolve_direct_prepared_type_declaration_matches_local_prepared_decl` (in `component_meta_query_engine.rs`)

`append_component_meta_registry_entries_skips_imported_refresh_for_explicit_seeded_object_surfaces`
(in `meta_resolve_tests.rs`) was renamed and rewritten — its
"skip refresh" optimization depended on
`imported_declaration_surface_is_authoritative` returning true,
which itself required source-text. Under the graph-only resolver
that optimization can't engage; the surviving invariant (the
direct imported macro root seeds the registry with an explicit
object surface) is preserved.

Two source-text edge-case tests in `meta_resolve_tests.rs`
(`declaration_text_does_not_match_substring_names`,
`declaration_text_handles_braces_inside_string_literals`) were
deleted — they exercised `extract_declaration_details` /
`find_named_declaration_start` through the resolver, but the
underlying helpers are still unit-tested in
`resolver_core::declaration_metadata::tests`.

`imported_jsdoc_enrichment_uses_cached_parse_and_does_not_reparse_source`
(in `meta_tests.rs`) drops its call-count assertion on the deleted
`project_macro_surfaces_from_source_type_name` instrumentation; the
architecture guard `no_text_based_macro_surface_projection_helpers`
now enforces the same invariant statically. The behaviour
assertion (JSDoc descriptions propagate through imported `Omit<>`)
is preserved.

The behaviour change for `should_seed_direct_macro_registry_entry`
required renaming/rewriting one unit test in `component_meta.rs`:
`resolve_component_meta_parts_skips_direct_non_object_imported_macro_seed`
→ `resolve_component_meta_parts_seeds_imported_macro_root_when_graph_metadata_unknown`.
Pre-Phase-4b the function's source-text path could parse the
imported alias body to detect a non-object surface. Under the graph
resolver, declaration text is `None` for un-seeded metadata, so
`kind` is `Unknown` and the registry seed proceeds. The new test
documents the post-Phase-4b contract.

## End-of-change checks

- **`cargo test --workspace --tests --verbose`** (tee'd to
  `/tmp/p04b-c8-workspace.txt` and `/tmp/p04b-final-workspace.txt`):
  45 test result blocks, all green; 10282 passed, 0 failed, 4
  ignored.
- **`cargo test -p verter_session --test correctness`** (tee'd to
  `/tmp/p04b-final-correctness.txt`): 18 passed, 0 failed,
  1 ignored.
- **`cargo fmt --all --check`**: clean (after the final
  `chore(meta): apply cargo fmt` commit).
- **`pnpm install --frozen-lockfile`**: lockfile in sync.
- **`cargo clippy --workspace -- -D warnings`**: intentionally
  skipped per the 5k/5l/5m/4 precedent. The phase-04-complete
  base commit already failed clippy with pre-existing warnings;
  Phase 4b's deletions remove some dead helpers but introduce no
  new clippy violations.

## Deferred

None. Phase 4b is an atomic-gate phase — `deferred[]` MUST be
empty per §0.6 R7 + r17/Codex-P1#1 atomic-gate rule. All §4b.2
through §4b.5a work landed in this worker's contiguous commit
chain.
