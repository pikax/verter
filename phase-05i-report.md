# Phase 5i — Exclude/Extract Concrete-Literal Reduction

**Branch:** `wt/phase-05i-exclude-extract-reduction`
**Base commit:** `08d79dfa75230232b89489d2c449d4f65e148e03` (phase-05h-complete)
**Work head before marker:** `03716c0e384ed860387dd43266c30787a8ec39d8` (style commit applying rustfmt)
**Atomic gate:** REQUIRED — `status: success`, `deferred: []`, `derivation_notes_verified: true`.

## Summary

Phase 5i extends `build_builtin_utility` (the
`SemanticQueryKey::Instantiate` lowering for builtin utilities) with a
literal-type reduction for `Exclude<T,U>` and `Extract<T,U>`, plus a
mapper `name_remap` evaluator + `TemplateLiteral` literal-fold to
close the `template_literal_as_key` gap (re-homed from 5k per §5.13
r15 table).

The new arms route per-member assignability through the existing
`relate_nodes` API (which already decides literal-vs-literal equality
via `literals_equal`); survivors reconstitute through
`intern_normalized_union_or_intersection`, which canonicalises the
empty-survivor case to `Primitive(Never)` and the singleton-survivor
case to the lone arm. No new `SemanticQueryKey` variants or
`ProjectionMode` discriminators are introduced — the discipline from
§0 binding amendment + r14 is preserved.

The `mapped_types` seed (`resolver_coverage_mapped_types_exclude_distributes`)
closes; three new Class A fixtures (`mapped_exclude`,
`mapped_extract`, `template_literal_as_key`) land with rule-correct
expected values per §5.B.5 + §5.B.5.1; six §5.D tests
(.2/.3/.4/3×.5) land per the §5.D ownership tables.

## Per-commit summary

1. `6e205543 refactor(meta): close Exclude/Extract literal-type reduction via build_builtin_utility`
   - Adds `Extract` | `Exclude` arms in `build_builtin_utility`
     before the catch-all `_` arm.
   - Updates the doc-classification: `Extract` / `Exclude` are
     now Union-filter (not Opaque).
   - Updates the `lower.rs:337` comment to reflect the closure (the
     eager-resolve lowering contract is unchanged; only the body
     evaluator gains the reduction).
   - Un-ignores the seed test
     (`resolver_coverage_mapped_types_exclude_distributes`).
   - Files: `crates/verter_session/src/project_semantic_dispatch/build.rs`,
     `crates/verter_session/src/project_semantic_dispatch/lower.rs`,
     `crates/verter_session/tests/component_meta_audit/resolver_coverage_mapped_types.rs`.

2. `d81d161d refactor(meta): apply mapper name_remap + fold TemplateLiteral literals during MaterializeSurface`
   - `build_mapped_type` (build.rs): when `mapper.name_remap` is
     `Some(remap_node)`, substitute the mapper binder ->
     `Literal(name)` into `remap_node` and evaluate. If the result
     is a `Literal::String`, that string becomes the produced
     surface name; otherwise the iteration falls back to the bare
     key (regression-friendly fail mode).
   - `evaluate_deferred_semantic_node` (evaluate.rs): adds a
     `TemplateLiteral` arm that folds a template into a single
     `Literal::String` when every expression resolves to a string
     literal.
   - Files: `crates/verter_session/src/project_semantic_dispatch/build.rs`,
     `crates/verter_session/src/project_semantic_dispatch/evaluate.rs`.

3. `74a246fe test(meta): author Class A fixtures mapped_exclude + mapped_extract + template_literal_as_key`
   - Three SFC sources, three fixture entries, three expected.rs
     functions, three derivation notes citing TS spec §4.4 / §4.5,
     three .snap.json files generated via the
     `--ignored generate_class_a_snapshots_from_expected` workflow.
   - Files: `crates/verter_session/tests/correctness/fixtures.rs`,
     `crates/verter_session/tests/correctness/expected.rs`,
     `crates/verter_session/tests/correctness/derivation_notes/{mapped_exclude,mapped_extract,template_literal_as_key}.md`,
     `crates/verter_session/tests/correctness/snapshots/{mapped_exclude,mapped_extract,template_literal_as_key}.correctness.snap.json`.

4. `98785f16 test(meta): §5.B.5.1 rule-correctness gates for mapped_exclude + mapped_extract + template_literal_as_key`
   - Adds three programmatic byte-equal gate tests reusing the 5h
     helpers (`read_rule_correct_block_from_mismatches_md`,
     `run_resolver_under_audit_and_serialize`).
   - Adds three fenced ```json``` blocks to
     `phase-00-tier1-mismatches.md` rows 1, 2, 3 carrying the
     hand-authored rule-correct SnapshotView per fixture.
   - Each test PANICS on `UPDATE_SNAPSHOTS=1` (refusal-to-regenerate
     negative assertion).
   - Files: `phase-00-tier1-mismatches.md`,
     `crates/verter_session/tests/correctness/deferred_fixtures_rule_correct.rs`.

5. `cbf50182 test(meta): §5.D.2 read_once_shallow_first_lazy_for_exclude_extract_reduction`
   - Multi-file test (owner + transitively-needed dep + unrelated
     file) asserting (a) cold-path lazy expansion (unrelated NOT
     loaded), (b) warm-path zero deltas, (c) q1 == q2.
   - Files: `crates/verter_session/src/component_meta_read_once_tests.rs`.

6. `9e55ab87 test(meta): §5.D.3 intermediate_hops_navigate_terminal_only_expanded_for_exclude_extract_reduction`
   - Mirrors the 5e/5f/5h test pattern — multi-hop ProjectPath in
     Expanded mode populates intermediate sub-keys in Navigate and
     only the terminal in Expanded.
   - Files: `crates/verter_session/src/component_meta_terminal_mode_tests.rs`.

7. `ef1bcb65 test(meta): §5.D.4 no_cache_promotion_for_budget_exceeded_exclude_extract_reduction`
   - Single-host, same-host re-query test (per r18/Claude-N18).
     A budget-exceeded sentinel from the first query MUST result
     in a cold-fire on the second query (warm count must NOT
     increment).
   - Files: `crates/verter_session/src/component_meta_no_cache_promotion_tests.rs`.

8. `8d3a6e52 test(meta): §5.D.5 pathological_exclude_self_recursive + extract_through_typeof + template_literal_key_recursion`
   - Three pathological fixtures landed in
     `component_meta_pathological_recursion_tests.rs`:
     - `pathological_exclude_self_recursive` — `type R = Exclude<R, never>`.
     - `pathological_extract_through_typeof` — `Extract<typeof y, R>` chain.
     - `pathological_template_literal_key_recursion` — `type R = { [K in keyof R as `${K & string}_x`]: R[K] }`.
   - Each runs on a 32 MiB worker thread; expected: terminate with
     a Recursive / Opaque / RecursiveRef / deferred-Mapped sentinel.
   - Files: `crates/verter_session/src/component_meta_pathological_recursion_tests.rs`.

9. `03716c0e style(meta): apply rustfmt to phase 5i additions` — final fmt.

## Caller-slice enumeration (r14 mandatory)

Spawn-time greps at this worktree:

```
grep -rn "Exclude\|Extract" \
  crates/verter_session/src/resolver_core/ \
  crates/verter_session/src/project_semantic_dispatch/ \
  --include='*.rs'
```

Returns (filtered to relevant matches):

| file:line | role | resolved by 5i? |
|---|---|---|
| `crates/verter_session/src/resolver_core/scope_shadowing.rs:127` | doc-comment listing utility names | informational, no change required |
| `crates/verter_session/src/resolver_core/shallow_file_state.rs:1777, 1821` | unrelated "Extract enum members" / "Extract per-member ..." doc lines | informational |
| `crates/verter_session/src/project_semantic_dispatch/build.rs:589` | doc-comment classification of utilities | UPDATED to reflect Extract/Exclude as Union-filter |
| `crates/verter_session/src/project_semantic_dispatch/build.rs:976` | the `_` catch-all arm where Extract/Exclude previously emitted Opaque(Miss) | UPDATED — new arm added BEFORE the catch-all |
| `crates/verter_session/src/project_semantic_dispatch/lower.rs:337` | inline comment for utility lowering | UPDATED to reflect closure |
| `crates/verter_session/src/project_semantic_dispatch/mod.rs:388` | `utility_param_names` table for "Extract"/"Exclude" | unchanged (already returns `["T", "U"]`) |
| `crates/verter_session/src/project_semantic_dispatch/substitute.rs:89` | unrelated "Extract the binder's name" doc | informational |
| `crates/verter_session/src/project_semantic_dispatch/tests.rs:3681, 3699-3700, 4745, 4821, 4825-4826` | test-only references | unchanged |
| `crates/verter_session/src/project_semantic_dispatch/walk.rs:2, 89` | doc-comments only | informational |

```
grep -rn "RelationResult\|relate_nodes\b" \
  crates/verter_session/src/ --include='*.rs'
```

Returns (high level): the relation engine entry points
(`crates/verter_session/src/project_semantic_dispatch/relation.rs`)
and the `build_conditional` callers
(`crates/verter_session/src/project_semantic_dispatch/build.rs:1743+`).
The new Extract/Exclude arm uses `relate_nodes` as its per-member
authority — the new call sites are colocated with the new arm in
`build.rs`. No relation-engine surface change was required;
literal-vs-literal equality (`literals_equal` at `relation.rs:1019`)
already discriminates correctly.

The d_cutover_characterization_tests.rs callsites are existing
tests that exercise `relate_nodes` directly; they continue to pass
unchanged.

## Derivation notes

All three new Class A fixtures have derivation notes whose first
non-blank line cites a rule source per the §0p.A.4 `citation_re`
gate:

- `mapped_exclude.md` — `TS spec §4.4 — Predefined `Exclude<T,U>` utility (distributive conditional)`
- `mapped_extract.md` — `TS spec §4.4 — Predefined `Extract<T,U>` utility (distributive conditional)`
- `template_literal_as_key.md` — `TS spec §4.5 — Template literal types in mapped key positions`

Each note ties the rule to the specific `build_builtin_utility` /
`build_mapped_type` change that closes the gap.

## Anchor drift log

None. The brief's anchor citations
(`build.rs:589`, `build.rs:976`, `lower.rs:337`,
`mod.rs:388`) all matched current HEAD before the changes.

## Tests landed

- 1 seed un-ignore: `resolver_coverage_mapped_types_exclude_distributes`
- 3 Class A fixture authoring tests (registered in `FIXTURES`,
  picked up automatically by `correctness_snapshot_for_every_fixture`).
- 3 §5.B.5.1 rule-correctness gate tests
  (`deferred_fixture_{mapped_exclude,mapped_extract,template_literal_as_key}_byte_equal_to_rule_correct_expected`).
- 1 §5.D.2 (`read_once_shallow_first_lazy_for_exclude_extract_reduction`).
- 1 §5.D.3 (`intermediate_hops_navigate_terminal_only_expanded_for_exclude_extract_reduction`).
- 1 §5.D.4 (`no_cache_promotion_for_budget_exceeded_exclude_extract_reduction`).
- 3 §5.D.5 pathological tests
  (`pathological_exclude_self_recursive`,
  `pathological_extract_through_typeof`,
  `pathological_template_literal_key_recursion`).

Total **12 net-new tests** + 1 seed un-ignore = 13 newly-passing
tests against the prior tree.

## Verification (per §0.6.3)

```
cargo test --workspace --tests --verbose 2>&1 | tee /tmp/p05i-workspace.txt
  -> 10263 passed, 0 failed across 45 test result blocks.

cargo clippy --workspace --tests -- -D warnings
  -> Clean (only unrelated ts-rs `serde` macro parse warnings, pre-existing).

cargo fmt --all --check
  -> Clean (after the 03716c0e style commit).

pnpm install --frozen-lockfile
  -> Lockfile in sync; husky prepare runs cleanly.

cargo test -p verter_session --test correctness 2>&1 | tee /tmp/p05i-correctness.txt
  -> 15 passed, 0 failed, 1 ignored (the `--ignored
     generate_class_a_snapshots_from_expected` is intentionally
     ignored except when run explicitly to regenerate snapshots).
```

Class A invisibility gate: green. No snapshot drift on existing
fixtures (including 5h's `userland_shadowing_pick`).

## Deferred items

NONE. Atomic gate honoured — `deferred[]` is empty.

## Marker

`crates/verter_session/.phase-markers/phase-05i-complete` written
in the next commit per §0.6 R7.
