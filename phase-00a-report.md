# Phase 0a — Correctness baseline (mapped + structural fixtures)

**Phase id:** 00a
**Branch:** `wt/phase-00a-correctness-mapped`
**Base commit:** `3d228474d487429fe4d6942c2a4ee2ab547a63e8`
**Status:** complete (11 Class A fixtures landed; 5 utility-type
fixtures deferred to Phase 5 §5.B.5 per r9 reviewer consensus)

## Summary

Phase 0a authored the Tier-1 + Tier-2 correctness gate for Verter's
component-meta resolution. The gate is implemented as a new
integration-test crate target at
`crates/verter_session/tests/correctness.rs` plus the supporting
`tests/correctness/{snapshot_view,fixtures,expected}.rs` modules.

11 Class A fixtures (6 mapped-type + 5 structural) were authored
with hand-derived expected `SnapshotView` values per §0p.A.0
discipline (no Volar / vue-component-meta / vue-tsc consultation).
Each fixture is a minimal hermetic SFC that exercises ONE TypeScript
spec rule or Verter resolution rule. Each carries a derivation note
under `tests/correctness/derivation_notes/<id>.md` whose first
non-blank line cites a TS-spec §, Verter rule, or CLAUDE.md § (the
harness panics if any citation is missing or malformed).

The author-first generator (`generate_class_a_snapshots_from_expected`)
runs under `--include-ignored` and writes 11 `.correctness.snap.json`
files from the programmatic expected. The main test
(`correctness_snapshot_for_every_fixture`) compares Verter's actual
output against the .snap.json files byte-for-byte. The 12-row
parametric discriminating self-test
(`correctness_gate_is_discriminating_for_every_property`) skips
property-coverage rows whose fixtures live in Phase 0b's scope (per
the §0p.A.5 r5/M6 fix); the rows that map to Phase 0a fixtures all
PASS.

## 11 Class A fixtures authored (ids only)

6 mapped-type fixtures:
1. `mapped_pick_two_keys`
2. `mapped_omit_two_keys`
3. `mapped_partial`
4. `mapped_required`
5. `mapped_readonly`
6. `mapped_record`

5 structural fixtures:
7. `indexed_access_two_levels`
8. `keyof_intersection`
9. `conditional_distributive`
10. `intersection_of_objects`
11. `recursive_alias_via_typeof`

## Harness files added

- `crates/verter_session/tests/correctness.rs` — integration-test
  crate root + the citation matcher + the discriminating self-test
  + the citation-matcher self-tests.
- `crates/verter_session/tests/correctness/snapshot_view.rs` —
  test-only `SnapshotView` projection of `ComponentMetaAnalysis`
  with a deterministic `render_type_signature` over every `TypeExpr`
  variant. No production-side `Serialize` was added.
- `crates/verter_session/tests/correctness/fixtures.rs` — 11 Class A
  fixtures + the `FixtureClass` enum (Class A / B / C; B + C empty
  in Phase 0a) + the `FIXTURES` registry.
- `crates/verter_session/tests/correctness/expected.rs` — programmatic
  `pub fn <fixture_id>() -> SnapshotView` per Class A fixture +
  `lookup_class_a_expected` dispatcher.
- `crates/verter_session/tests/correctness/snapshots/*.correctness.snap.json`
  — 11 generated snapshots (committed for the Tier-2 gate).
- `crates/verter_session/tests/correctness/derivation_notes/*.md` —
  11 markdown derivation notes citing the rule each fixture
  exercises (TS spec § / Verter rule / CLAUDE.md §).

Plus `phase-00-tier1-mismatches.md` at the worktree root, documenting
the 5 deferred fixtures (rule citation, rule-correct expected,
Verter actual, root cause, owner phase = Phase 5 §5.B.5). This file
is committed in `4dccb417` and is preserved untouched — Phase 5
§5.B.5 reads it as design-input when authoring those fixtures.

## Test counts

- **Workspace** (`cargo test --workspace --tests --verbose`):
  full pass. The orchestrator-provided `.integration-tests` junction
  in this worktree exposes `.integration-tests/repos/nuxt-ui/`, so
  the previously-broken nuxt-ui-fixture-dependent test targets
  (`corpus_audit_tests`, `component_meta_audit_corpus`) now compile
  and run cleanly.

- **Correctness gate** (`cargo test -p verter_session --test correctness`):
  11 passed, 0 failed, 1 ignored (the author-first generator).

  Tests added by this phase (all PASSING in the post-change tree):
  - `generate_class_a_snapshots_from_expected` (ignored — runs only
    under `--include-ignored`; PASSES when invoked).
  - `correctness_snapshot_for_every_fixture` — main Tier-1 + Tier-2
    gate; iterates all 11 Class A fixtures.
  - `correctness_gate_is_discriminating_for_every_property` — 12
    discriminating-mutation rows (5 PASS for Phase 0a fixtures, 7
    SKIP per the r5/M6 fix because their fixtures live in Phase 0b).
  - `citation_matcher_accepts_ts_spec_section`
  - `citation_matcher_accepts_claude_md_section`
  - `citation_matcher_accepts_skill_path_and_verter_rule`
  - `citation_matcher_rejects_unrelated_text`
  - `snapshot_view::self_tests::renders_primitives_and_literals`
  - `snapshot_view::self_tests::renders_union_and_object_in_canonical_order`
  - `snapshot_view::self_tests::renders_array_with_parens_for_unions`
  - `snapshot_view::self_tests::renders_function_signature_with_void_default`
  - `snapshot_view::self_tests::object_field_order_is_irrelevant`

  Each citation_matcher test includes both a positive and a
  negative assertion (rejects unrelated prose, rejects missing
  whitespace/section sign).

## Newly passing / newly failing

- All 11 visible tests in the correctness target are NEW (the test
  crate did not exist on the pre-change tree). They fail trivially
  on the pre-change tree (the file does not compile to a test
  binary). Each PASSES on the post-change tree.
- No pre-existing tests changed status because of this phase.

## Audit metrics

Not applicable — Phase 0a is pure test/harness addition. No
production code changed, no audit counters affected.

## Tier 3 (Volar) — skipped

`packages/benchmark/benchmark-results/meta-ui/.expected-vue-component-meta/`
does not exist in this worktree. Per §0p.B.1, "If B.1.1 directory is
empty, the Volar baseline is gone — Phase 0 proceeds without §0p.B
(note in `phase-00-report.md` and skip). This is NOT a STOP — Tier 3
is optional." Skipped.

## Deferred items (§0p.A.2 r9 / §5.B.5)

Per r9 reviewer consensus, 5 utility-type fixtures whose rule-correct
expected outputs Verter does not currently produce are deferred to
Phase 5 §5.B.5. They are NOT acceptable as Class A regression
baselines (would lock in the wrong expected output) and NOT
acceptable as Class B (Class B captures Verter's output as the
intended behaviour). Phase 5 will author them with rule-correct
expected once the resolver variants close the gaps.

The 5 deferred ids:

1. **`mapped_exclude`** — `Exclude<>` not evaluated through macro
   path. Rule: TS spec §4.4.
2. **`mapped_extract`** — same root cause. Rule: TS spec §4.4.
3. **`template_literal_as_key`** — template-literal key iteration
   loses every prop. Rule: TS spec §4.5.
4. **`generic_substitution_via_typeof`** — typeof substitution
   skipped. Rule: TS spec §3.6.
5. **`userland_shadowing_pick`** — TS-first / userland-shadow
   precedence not honoured. Rule: Verter rule
   (`./.claude/skills/type-resolution`).

Tracking file: `phase-00-tier1-mismatches.md` at the worktree root
(committed in `4dccb417`). Phase 5 §5.B.5 reads this file as
design-input.

Phase 0b (separate worker): 7 component-meta property fixtures plus
Class B + C regression baselines.

## Citation discipline verification

Every Class A fixture's `derivation_notes/<id>.md` first non-blank
line passes the §0p.A.4 citation regex implemented as a plain-string
prefix matcher in `correctness.rs::citation_line_is_well_formed`
(matches `(?i)^TS spec\s+§|^[.]/[.]claude/skills/|^Verter rule\b|^CLAUDE[.]md\s+§`).
The harness check `ensure_class_a_derivation_notes()` passes after
the 5 deferred fixtures' notes are removed.

The marker JSON's `derivation_notes_verified` field is set to `true`
on this basis.

## Commit chain (Phase 0a)

| sha          | message                                                                          |
|--------------|----------------------------------------------------------------------------------|
| 0cb60d1f     | test(correctness): tier-1 hand-curated fixtures with exact expected results       |
| 165304e3     | test(correctness): tier-1+tier-2 harness asserting against hand-authored snapshots |
| 9f19ac93     | test(correctness): generate Class A snapshots from expected.rs                    |
| 4dccb417     | docs(correctness): document Phase 0a tier-1 known defects                         |
| 0f040d51     | docs(correctness): Phase 0a report                                                |
| b29fe547     | chore(orchestrator): mark phase 00a complete (initial — superseded by review)     |
| 309dbe71     | fix(phase-00a-review): drop 5 utility-type fixtures deferred to Phase 5 §5.B.5    |
| (next)       | fix(phase-00a-review): refresh phase-00a-report for 11-fixture scope (r9)         |
| (next)       | fix(phase-00a-review): rewrite marker for r9 R7 schema                            |
