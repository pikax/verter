# Phase 0b — Correctness baseline (component-meta property fixtures + B/C regression)

**Phase id:** 00b
**Branch:** `wt/phase-00b-correctness-properties`
**Base commit:** `0f31dabd94deb9bb9f45dd5c4dbdc9c03233d827`
**Status:** partial-deferred (5 of 7 Class A property fixtures landed; 2
deferred to a later phase per §0p.A.4 case 2)

## Summary

Phase 0b extends the correctness gate Phase 0a authored. It adds:

- **5 Class A property fixtures** (of 7 brief-listed) exercising
  Verter's component-meta macros: `defineProps + withDefaults`,
  `defineEmits<T>`, `defineExpose({...})`, `inheritAttrs: false`,
  and single-component-root fallthrough propagation.
- **6 Class B regression baselines** mirroring the existing
  `corpus_representatives/*.rs` set: accordion, alert, app,
  auth_form, avatar, avatar_group.
- **3 Class C regression baselines** mirroring the existing
  `pathological_*.rs` set: table_loading_animation,
  editor_toolbar_array_or_nested, tabs_dynamic_helper.
- **5 derivation notes** under
  `tests/correctness/derivation_notes/` citing the rule each Class A
  property fixture exercises (Verter macros §… or CLAUDE.md
  §Fallthrough). The harness check
  `ensure_class_a_derivation_notes()` enforces these citations.

The 2 deferred Class A fixtures (`fixture_slots_typed`,
`fixture_models`) produce hand-derived rule-correct expected
SnapshotViews that Verter's current resolver does not match. Per
§0p.A.4 case 2 (and the Phase 0a precedent that deferred 5
utility-type fixtures), these fixtures are dropped from the
`FIXTURES` registry and their rule-correct expected, Verter's
actual, and the root-cause analysis are documented in
`phase-00b-tier1-mismatches.md` at the worktree root for the
follow-up phase to consume.

## 5 Class A property fixtures landed (ids only)

1. `fixture_props_with_defaults`
2. `fixture_events_typed`
3. `fixture_exposed_methods`
4. `fixture_fallthrough_inherit`
5. `fixture_fallthrough_root_inherit`

## 2 Class A property fixtures deferred (ids only)

1. `fixture_slots_typed` — slot binding type literals
   (`props: { item: string }`) lower to `Unknown { raw:
   "semanticMiss" }` instead of `Primitive(String)`.
2. `fixture_models` — `defineModel<T>()` type T captured as text but
   not lowered through the macro path; `model.type_expr` lowers to
   `Unknown { raw: "semanticMiss" }`. (Note: the corresponding
   `update:<name>` event payload IS resolved through a separate
   path, so the resolver knows T but the model's TypeExpr field is
   filled from the broken path.)

Tracking file: `phase-00b-tier1-mismatches.md` at the worktree root,
documenting both fixtures' rule-correct expected SnapshotView,
Verter actual JSON, root cause, and discriminating-test row impact.

## Class B regression baselines captured (n=6)

- `accordion.regression.snap.json`
- `alert.regression.snap.json`
- `app.regression.snap.json`
- `auth_form.regression.snap.json`
- `avatar.regression.snap.json`
- `avatar_group.regression.snap.json`

Source content reuses the exact SFC + cross-file types pairs from
the existing
`crates/verter_session/tests/component_meta_audit/corpus_representatives/*.rs`
tests so the regression baseline reflects the same component shapes
those audit tests already exercise.

## Class C regression baselines captured (n=3)

- `pathological_table_loading_animation.regression.snap.json`
- `pathological_editor_toolbar_array_or_nested.regression.snap.json`
- `pathological_tabs_dynamic_helper.regression.snap.json`

Source content reuses `test_fixtures/{table,editor_toolbar,tabs}*`
files via `include_str!` so it stays in sync with the existing
`pathological_*.rs` integration tests.

## Test counts

- **Workspace** (`cargo test --workspace --tests --verbose`):
  10116 passed, 0 failed, 1 ignored (the pre-existing scheduler
  dispatch test).
- **Correctness gate** (`cargo test -p verter_session --test correctness`):
  11 passed, 0 failed, 1 ignored (the author-first generator).

  Tests exercised by this phase:
  - `correctness_snapshot_for_every_fixture` — main Tier-1 + Tier-2
    gate; iterates all 25 fixtures (11 Phase 0a Class A + 5 Phase 0b
    Class A + 6 Class B + 3 Class C).
  - `correctness_gate_is_discriminating_for_every_property` —
    parametric 12-row discriminating self-test; under Phase 0b the
    table covers 9 rows live (the 3 referencing
    `fixture_slots_typed` and `fixture_models` skip per the
    §0p.A.5 r5/M6 fix because those fixtures are deferred).
  - `generate_class_a_snapshots_from_expected` (ignored — runs only
    under `--include-ignored`; PASSES when invoked).
  - All citation_matcher and snapshot_view::self_tests inherited
    from Phase 0a continue to PASS.

## Discriminating-case rows

Of the 12 brief-listed rows, 9 run live and PASS after Phase 0b:

| # | Fixture id                         | MutationKind                | Phase | Status |
|---|------------------------------------|-----------------------------|-------|--------|
| 1 | mapped_pick_two_keys               | PropExtraKey                | 0a    | pass   |
| 2 | mapped_omit_two_keys               | PropMissingKey              | 0a    | pass   |
| 3 | fixture_props_with_defaults        | PropDefaultDropped          | 0b    | pass   |
| 4 | mapped_required                    | PropRequiredFlipped         | 0a    | pass   |
| 5 | mapped_pick_two_keys               | PropTypeChanged             | 0a    | pass   |
| 6 | fixture_slots_typed                | SlotDropped                 | 0b    | skip   |
| 7 | fixture_slots_typed                | SlotPayloadChanged          | 0b    | skip   |
| 8 | fixture_events_typed               | EventSignatureChanged       | 0b    | pass   |
| 9 | fixture_models                     | ModelDropped                | 0b    | skip   |
|10 | fixture_exposed_methods            | ExposedDropped              | 0b    | pass   |
|11 | fixture_fallthrough_inherit        | FallthroughInheritFlipped   | 0b    | pass   |
|12 | fixture_fallthrough_root_inherit   | FallthroughSurfaceChanged   | 0b    | pass   |

The 3 skipped rows are mechanical skips from the §0p.A.5 r5/M6 fix
(`fixtures::FIXTURES.iter().find(|f| f.id == case.fixture_id)` →
`None` → `continue`). They become live when the deferred fixtures
land in a later phase.

## Newly passing / newly failing

- 5 new Class A fixtures + 9 Class B+C fixtures land. All PASS
  against their committed snapshots.
- The Phase 0b commits change the `FIXTURES` array and add 9
  regression `.snap.json` files plus 5 `.correctness.snap.json`
  files; on the pre-change tree those files don't exist, so the
  newly-added 14 fixture iterations would each trigger the
  `snapshot missing` panic. Post-change, every fixture finds its
  snapshot and asserts byte-equality successfully.
- No pre-existing tests changed status. The Phase 0a fixtures'
  iteration order changed (Class A now appears AFTER Class B+C)
  but iteration order is independent of correctness — every
  fixture still asserts against its own dedicated snapshot file.
- The discriminating self-test went from "5 of 12 cases live"
  (Phase 0a) to "9 of 12 cases live" (Phase 0b). The 4 newly-live
  cases are: `PropDefaultDropped` (props_with_defaults),
  `EventSignatureChanged` (events_typed), `ExposedDropped`
  (exposed_methods), `FallthroughInheritFlipped`
  (fallthrough_inherit), `FallthroughSurfaceChanged`
  (fallthrough_root_inherit). All 5 PASS.

## Audit metrics

Not applicable — Phase 0b is pure test/harness addition. No
production code changed, no audit counters affected.

## Tier 3 (Volar) — skipped

`packages/benchmark/benchmark-results/meta-ui/.expected-vue-component-meta/`
does not exist in this worktree (verified via `ls` returning
"No such file or directory"). Per §0p.B.1, this is NOT a STOP — the
Volar baseline is gone, so Phase 0b proceeds without §0p.B (matches
the Phase 0a posture — Phase 0a's report also recorded "skipped").

Tier 3 divergence summary: not generated (no baseline).

## Deferred items (§0.5.1)

- `fixture_slots_typed` — slot binding type literal not lowered.
  Owner phase: later phase reaching slot binding type resolution.
- `fixture_models` — `defineModel<T>()` type T not lowered. Owner
  phase: later phase reaching defineModel type resolution.

Both fixtures' rule-correct expected SnapshotViews and Verter's
actual outputs are documented in `phase-00b-tier1-mismatches.md`
(committed at the worktree root).

## Citation discipline verification

Every Class A fixture's `derivation_notes/<id>.md` first non-blank
line passes the §0p.A.4 citation matcher. The Phase 0b notes use
either `Verter rule:` or `CLAUDE.md §Fallthrough` as the leading
citation:

- `fixture_props_with_defaults.md`: "Verter rule: `withDefaults`
  populates `default_signature` and flips `required`"
- `fixture_events_typed.md`: "Verter rule: `defineEmits<T>`
  preserves T's parameter shape"
- `fixture_exposed_methods.md`: "Verter rule: `defineExpose({...})`
  surfaces every key as exposed"
- `fixture_fallthrough_inherit.md`: "CLAUDE.md §Fallthrough —
  `inheritAttrs: false` zeros the surface"
- `fixture_fallthrough_root_inherit.md`: "CLAUDE.md §Fallthrough —
  single component root propagates child surface"

The harness check `ensure_class_a_derivation_notes()` runs as part
of `correctness_snapshot_for_every_fixture` and PASSES (no
panic). The marker JSON's `derivation_notes_verified` is set to
`true` on this basis.

## Universal verification (§0.6.3)

- `cargo test --workspace --tests --verbose` — 10116 passed, 0
  failed.
- `cargo clippy --workspace -- -D warnings` — clean.
- `cargo fmt --all --check` — clean (after `cargo fmt --all` in
  commit `018666e2`).
- `pnpm install --frozen-lockfile` — clean.
- `cargo test -p verter_session --test correctness` — 11 passed,
  0 failed.

## Commit chain (Phase 0b, chronological)

| sha          | message                                                                          |
|--------------|----------------------------------------------------------------------------------|
| f41bbf2f     | test(correctness): Phase 0b — 7 Class A property fixtures + B/C registry         |
| 4f3d52d9     | test(correctness): generate Phase 0b Class A snapshots from expected.rs          |
| 3a9fcda5     | test(correctness): drop fixture_slots_typed + fixture_models — defer to later phase |
| c57bf055     | test(correctness): Phase 0b — capture Class B + C regression baselines           |
| 018666e2     | style(correctness): rustfmt fixtures.rs (cargo fmt)                              |
| (this commit)| docs(correctness): Phase 0b report                                               |
