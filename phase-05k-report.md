# Phase 5k worker report

**Branch:** `wt/phase-05k-typeof-substitution`
**Base commit at spawn:** `6d665cb632245503e39615177889bd6d209d971d` (phase-05j-complete)
**Work head before marker:** `8178a5af`
**Marker:** `chore(orchestrator): mark phase 05k complete` (atomic-gate)

## Summary

Phase 5k §5.13 closes the deferred Class A fixture
`generic_substitution_via_typeof` — `Instantiate { TypeOf { ... } }`
chained substitution failed for value-member typeof projections
(`phase-00-tier1-mismatches.md` row 4). The substitution-layer fix
amends `shallow_lower_type_expr`'s `TypeExpr::TypeOf` arm to attempt
single-segment root resolution first, falling back to the joined-2-
segment lookup only when the single-segment root misses AND a
longer path exists.

Single-workstream change per §5.13 r14 narrow scope retention. No
new resolver layers, no new dispatch helpers, no new
`SemanticQueryKey` variants, no new `ProjectionMode` discriminators.

## Per-commit summary

| SHA       | Title                                                                                                                |
|-----------|----------------------------------------------------------------------------------------------------------------------|
| `2ea177eb` | refactor(meta): propagate substitution through TypeOf in Instantiate composition                                     |
| `96c56581` | test(meta): author Class A fixture generic_substitution_via_typeof + derivation note + mismatches.md DATA block      |
| `d299ed8a` | test(meta): §5.B.5.1 rule-correctness gate for generic_substitution_via_typeof                                       |
| `9ecb6015` | test(meta): §5.D.2 read_once_shallow_first_lazy_for_typeof_substitution                                              |
| `c301f7e2` | test(meta): §5.D.3 intermediate_hops_navigate_terminal_only_expanded_for_typeof_substitution                         |
| `663d2c13` | test(meta): §5.D.4 no_cache_promotion_for_budget_exceeded_typeof_substitution                                        |
| `8b76de1a` | test(meta): §5.D.5 pathological_typeof_substitution_cycle                                                            |
| `8178a5af` | refactor(resolver_core): add #[deprecated] attributes to engine methods 5l will delete                              |

## Substitution fix (commit 1)

**File:**
`crates/verter_session/src/project_semantic_dispatch/lower.rs:1206`
(`TypeExpr::TypeOf` arm).

**Pre-fix behaviour.** The pre-Phase-5k branch joined the first two
`value_ref.path` segments into a single `"X.Y"` name whenever the
path had length > 1, treating EVERY dotted typeof as a
namespace-member lookup. That worked for `import * as Ns from './m';
typeof Ns.Foo` (the namespace-member case `build_typeof`'s
`has_namespace_prefix` branch handles via
`resolve_namespace_member_from_facts`) but broke ordinary
value-member projection like `const sample: Sample =
...; typeof sample.id`, because no value binding named `"sample.id"`
existed in the shallow-state value-symbol table. The downstream
Miss propagated up through `Instantiate`: when an outer caller
used `IdShape<typeof sample.id>` as a generic argument, the type
argument lowered to `Opaque(Miss)`, substitution into `{ id: T }`
left T unsubstituted, and the prop surfaced as `id: T`.

**Post-fix behaviour.** The lowering attempts single-segment root
resolution first (the value-member projection case) and falls back
to the joined-2-segment lookup only when the single-segment root
misses AND a longer path exists. The fallback preserves the
namespace-member semantics for `Ns.Foo[.Bar...]` shapes; the primary
path closes the value-member gap. Both branches reuse the same
`ProjectPath { mode: Navigate }` projection for the tail segments —
terminal-mode-only expansion is the outer caller's responsibility.

## Tests added

### Class A fixture authoring (commits 2 + 3)

- `crates/verter_session/tests/correctness/fixtures.rs` — new
  fixture entry `generic_substitution_via_typeof` (target `/c.vue`,
  Class A).
- `crates/verter_session/tests/correctness/expected.rs` — new
  programmatic expected `SnapshotView` (one required prop
  `id: string`).
- `crates/verter_session/tests/correctness/derivation_notes/generic_substitution_via_typeof.md`
  — derivation note citing TS spec §3.6 + CLAUDE.md "generic
  substitutions are part of semantic meaning".
- `crates/verter_session/tests/correctness/snapshots/generic_substitution_via_typeof.correctness.snap.json`
  — generated via `--ignored
  generate_class_a_snapshots_from_expected`.
- `phase-00-tier1-mismatches.md` row 4 — added the machine-readable
  rule-correct expected JSON DATA block.
- `crates/verter_session/tests/correctness/deferred_fixtures_rule_correct.rs`
  — added `deferred_fixture_generic_substitution_via_typeof_byte_equal_to_rule_correct_expected`.

### §5.D harness tests (commits 4-7)

- §5.D.2 `read_once_shallow_first_lazy_for_typeof_substitution`
  (`crates/verter_session/src/component_meta_read_once_tests.rs`).
- §5.D.3
  `intermediate_hops_navigate_terminal_only_expanded_for_typeof_substitution`
  (`crates/verter_session/src/component_meta_terminal_mode_tests.rs`).
- §5.D.4 `no_cache_promotion_for_budget_exceeded_typeof_substitution`
  (`crates/verter_session/src/component_meta_no_cache_promotion_tests.rs`).
- §5.D.5 `pathological_typeof_substitution_cycle`
  (`crates/verter_session/src/component_meta_pathological_recursion_tests.rs`).

Total new tests: 1 (rule-correctness gate) + 4 (§5.D) + 1 (Class A
correctness via the existing
`correctness_snapshot_for_every_fixture` umbrella) = 6 net new test
identities.

## §5.14.0 prerequisite — engine method deprecation (commit 8)

Added `#[deprecated(note = "Phase 5l deletion target: <method>")]`
to every engine resolver method 5l will delete (per sub-plan §8
deletion list):

1. `project_type_surface`
2. `project_type_surface_expr`
3. `project_type_surface_shape`
4. `project_prepared_type_surface_expr`
5. `project_prepared_type_surface_shape`
6. `project_type_member`
7. `project_type_keyspace`
8. `project_expr_surface_expr`
9. `project_expr_surface_expr_with_compound_objects`
10. `lower_and_project_to_expanded`
11. `instantiate_local_generic_ref`
12. `project_expr_surface_shape`
13. `project_route_surface_expr`

Each note carries the "Phase 5l deletion target: <method-name>"
prefix so the §5.14.1 r16 grep filter
(`grep -c "Phase 5l deletion target" /tmp/p05l-deprecated-check.txt`)
discriminates 5l's targets from any unrelated upstream deprecation
warnings.

Per r16/Claude-N1: NO `#[allow(deprecated)]` mod is added (the
§5.14.1 r16 warning-mode gate uses
`cargo rustc -p verter_session --lib -- -W deprecated` which counts
matching warnings only; the workspace BUILD does NOT fail on
deprecation warnings).

`cargo build --workspace --tests` produces 38 unique deprecation
warnings (the surviving caller list 5l's pre-flight gate
enumerates).

Plus: amended `no_deprecated_attributes_on_retired_symbols`
characterization test to remove three D-cutover-era retired symbol
names (`lower_and_project_to_expanded`, `project_expr_surface_shape`,
`instantiate_local_generic_ref`) that overlap with 5l's deletion
list. The exception is scoped to the 5k-5l window only — every
other D-cutover-retired symbol stays in the list.

## Verification

- `cargo test --workspace --tests --verbose 2>&1 | tee
  /tmp/p05k-workspace.txt`: **10276 passed; 0 failed** across **45
  blocks**.
- `cargo test -p verter_session --test correctness`: **18 passed;
  0 failed; 1 ignored** (the ignored
  `generate_class_a_snapshots_from_expected` test runs only via
  `--include-ignored`).
- `cargo fmt --all --check`: clean.
- `pnpm install --frozen-lockfile`: clean.
- The §5.B.5.1 rule-correctness gate for
  `generic_substitution_via_typeof` PASSES post-fix (would FAIL
  pre-fix — discriminating).
- The §5.D.2/.3/.4 tests exercise the read-once / terminal-mode /
  no-cache-promotion contracts on the typeof-substitution path.
- The §5.D.5 pathological fixture terminates without
  stack-overflow.

## Anchor drift log

None. All anchors used (`shallow_lower_type_expr`, `build_typeof`,
`resolve_bare_name_in_scope`, `phase-00-tier1-mismatches.md` row 4,
the 5h/5i/5j harness helpers in `deferred_fixtures_rule_correct.rs`)
matched the integration-HEAD layout at spawn. No file moves or
signature changes encountered.

## Deferrals

**EMPTY** (atomic-gate phase per §0.3 ATOMIC_GATE_PHASES, r17/Codex-P1#1).

The Class A fixture authoring (`§5.B.5`), the §5.B.5.1
rule-correctness gate, the four §5.D tests, and the §5.14.0
prerequisite landed within the bounded scope (1-3 commits for the
fix + ~6 test commits + 1 deprecation commit). No work was
deferred.

## STOP-condition compliance

- Substitution-layer fix: 1 commit (`2ea177eb`), single workstream
  (`shallow_lower_type_expr`'s `TypeExpr::TypeOf` arm) — within the
  §5.13 r14 narrow scope (1-3 commits, no new resolver layers, no
  new dispatch helpers, no new `SemanticQueryKey` variants, no new
  `ProjectionMode` discriminators).
- §5.B.5.1 rule-correctness gate: passes post-fix.
- Class A invisibility gate: no snapshot drift on pre-existing 22
  Class A snapshots (verified via
  `correctness_snapshot_for_every_fixture` +
  `ensure_class_a_expected_matches_snapshot`).
- Workspace green after every commit: verified.
- Atomic-gate marker: `status: "success"`, `deferred: []`,
  `derivation_notes_verified: true`.
