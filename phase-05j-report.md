# Phase 5j — slot-binding-parameter type lowering migration

## Summary

Phase 5j migrates the slot-binding-parameter type lowering from the
engine's analysis path to dispatch via the `ResolveMacroPayload`
variant + `MaterializeSurface { Slots }` codepath. This closes the
`slot_shapes` seed test, the `fixture_slots_typed` deferred Class A
fixture (`phase-00b-tier1-mismatches.md` row 1), and the
`fixture_models` deferred Class A fixture (`phase-00b-tier1-mismatches.md`
row 2, re-homed from 5k to 5j per parent §5.13 r15 table).

The migration is implemented through two independent fixes in the
`expand_field_expr` closure (`host_manage.rs::compute_evaluated_types*`),
both of which compose existing `SemanticQueryKey` variants — no new
variants or `ProjectionMode` discriminators introduced (per parent §0
binding amendment + sub-plan §0 worker constraint).

## Caller-slice manifest (r14 mandatory enumeration)

Spawn-time grep:

```bash
grep -rn "slot_binding\|SlotsSurface\|defineSlots" \
    crates/verter_session/src/ \
    crates/verter_semantic/src/ \
    --include='*.rs'
```

Identified migration sites:

| File:line | Concern | Phase 5j action |
|---|---|---|
| `crates/verter_session/src/host_manage.rs:4789-5050` | `expand_field_expr` closure dispatching `ProjectPath { base, output_path, Expanded }` for all field kinds | Special-case `FieldKind::SlotBinding` to route through new helper; special-case `MacroKind::DefineModel` to route through direct lower+raise |
| `crates/verter_session/src/project_semantic_dispatch/mod.rs:735-...` | Dispatch helper module | Added `project_slot_binding_member` helper composing existing variants |
| `crates/verter_session/src/project_semantic_dispatch/walk.rs:606-625` | `Function` arm in path walker | Already returned `opaque_miss()` — Phase 5j did NOT modify; the helper sits one layer above and structurally avoids the catch-all by reading `Function.params[0].ty` and re-issuing a fresh `ProjectPath` |
| `crates/verter_session/src/meta_resolve.rs:1340-1441` | `enrich_missing_slot_bindings` (engine analysis path) | UNCHANGED — only fires when `evaluated_types.define_slots` is non-empty AND a slot's binding params are unresolved. Phase 5j's `expand_field_expr` change populates `evaluated_types.slot_bindings` correctly upstream so this enrichment never needs to run. The function remains as a fallback for edge cases (typeof, deeply-deferred Conditional shells); a follow-up phase may delete it once those edge cases also route through dispatch. |
| `crates/verter_session/src/meta_resolve.rs:8516-8602` | engine `walk_component_meta_macro_shape_member_types::DefineSlots` arm (engine rescue path) | UNCHANGED — the rescue path's `materialize_define_slots_member` only triggers when `slot_member_needs_binding_rescue(property.ty)` returns true. Post-Phase-5j the property `ty` carries the resolved binding types directly (via the dispatch round-trip in the closure), so the rescue path is structurally a no-op for typed slot fixtures. Slated for deletion alongside the engine retirement in 5l. |

## Migration design

### `project_slot_binding_member` helper

`crates/verter_session/src/project_semantic_dispatch/mod.rs`

Composes existing `SemanticQueryKey::ProjectPath` variants in three
hops:

1. `ProjectPath { base, [Member(slot_name)], Navigate }` →
   slot value's `SemanticNodeId` (Navigate per CLAUDE.md
   "Macro Type Traversal Rule" path-precise rule).
2. Read `SemanticNodeData::Function { params, .. }` from the slot
   value via `node_data_for(host, slot_node)`. Pull `params[0].ty`.
3. `ProjectPath { base: param0_ty, [Member(binding_name)], <caller_mode> }` →
   binding's lowered `TypeExpr` in caller's mode.

Returns `CacheRead<QueryResult<TypeExpr>>` with merged dep_signatures
across the three hops so any change in the intermediate (slot
Function shape) or terminal (binding Object) is observed by the
caller's local fence.

**Why not a new variant:** per §0 binding amendment, Phase 5
cumulatively introduced ONE new `SemanticQueryKey` variant
(`ResolveMacroPayload`); every subsequent component-meta concern is
a non-variant dispatch helper. This mirrors `materialize_surface`,
`execute_pick`, `execute_omit`, `execute_to_type_expr` — non-variant
helpers introduced by 5b/5d/5e.

### `expand_field_expr` closure routing

`crates/verter_session/src/host_manage.rs::compute_evaluated_types*`

For `FieldKind::SlotBinding` fields, the closure now:
- Reads exactly two `Member` segments from `ctx.output_path` (the
  closure's emission contract: `[Member(slot), Member(binding)]`).
- Calls `dispatch.project_slot_binding_member(base, slot, binding,
  Expanded)`.
- Maps the result to `ExpansionResult::exact_concrete` /
  `symbolic_fallback()` per the existing closure shape.

For `MacroKind::DefineModel` macros (any field kind), the closure
now:
- Lower+raises `parsed_type_argument` directly via the dispatch (the
  macro's `T` IS the field's type, not a parent shell with
  member-named children — `ProjectPath { base, [Member(model)], ... }`
  always missed).
- Maps the raised `TypeExpr` to `ExpansionResult::exact_concrete`.
- Falls back to `parsed.clone()` if lowering / raising misses.

Both fixes preserve the audit-gated `target.borrow_mut().push(node_id)`
tail (per the surface_identities length-mismatch debug_assert at
host_manage.rs:5154).

## Commits added (this branch)

1. `b00cbbbf` — `refactor(meta): migrate slot-binding-parameter lowering to dispatch via project_slot_binding_member`
2. `730d4dc8` — `test(meta): close slot_shapes seed (un-ignore typed slot-binding lower test)`
3. `b7fe9cff` — `refactor(meta): close fixture_models gap in defineModel<T> field lowering`
4. `b7c793dd` — `test(meta): author Class A fixture fixture_slots_typed + derivation note`
5. `a4ee977a` — `test(meta): author Class A fixture fixture_models + derivation note + mismatches.md DATA blocks`
6. `9b1b6640` — `test(meta): §5.B.5.1 rule-correctness gates for fixture_slots_typed + fixture_models`
7. `8cd3a73d` — `test(meta): §5.D.2 read_once_shallow_first_lazy_for_slot_binding_lowering`
8. `5e57be42` — `test(meta): §5.D.3 intermediate_hops_navigate_terminal_only_expanded_for_slot_binding_lowering`
9. `250ed340` — `test(meta): §5.D.4 no_cache_promotion_for_budget_exceeded_slot_binding_lowering`
10. `35b703dd` — `test(meta): §5.D.5 pathological_nested_slot_definitions`
11. `917cbc7d` — `test(meta): §5.D.5 pathological_self_referential_slot_payload`

(Marker commit `chore(orchestrator): mark phase 05j complete` lands as
the LAST commit; its sha is the +1 successor of `917cbc7d`.)

## Tests landed

### Closes seed (un-ignored)

- `resolver_coverage_slot_shapes_typed_bindings_lower_to_primitive`
  (`crates/verter_session/tests/component_meta_audit/resolver_coverage_slot_shapes.rs`).
  Pre-Phase-5j: ignored with deferral note. Post-Phase-5j: passes.

### Class A fixtures authored (per §5.B.5)

- `fixture_slots_typed` (`phase-00b-tier1-mismatches.md` row 1).
  Slots = `[default, named]`; payload signatures `{ item: string }` /
  `{ row: number }`.
- `fixture_models` (`phase-00b-tier1-mismatches.md` row 2, re-homed
  from 5k to 5j per parent §5.13 r15 table). Models =
  `[count: number, modelValue: string]`; matching props with
  `T | undefined` signatures and `update:<name>` events with
  `[value: T | undefined]` payload tuples.

Each fixture has a derivation note at
`crates/verter_session/tests/correctness/derivation_notes/<id>.md`
whose first non-blank line cites a rule source per §0p.A.4
`ensure_class_a_derivation_notes()` regex.

### §5.B.5.1 rule-correctness gates (programmatic byte-equal)

- `deferred_fixture_fixture_slots_typed_byte_equal_to_rule_correct_expected`
- `deferred_fixture_fixture_models_byte_equal_to_rule_correct_expected`

Both tests live in
`crates/verter_session/tests/correctness/deferred_fixtures_rule_correct.rs`
and read the rule-correct expected JSON from the fenced ` ```json ` blocks
in `phase-00b-tier1-mismatches.md` (parsed via the new
`read_rule_correct_block_from_mismatches_md_00b` helper, which mirrors
the existing `_md` helper for Phase 0a's mismatches doc).

### §5.D tests landed (5j scope)

| Test | File | Purpose |
|---|---|---|
| `read_once_shallow_first_lazy_for_slot_binding_lowering` | `component_meta_read_once_tests.rs` | §5.D.2 — owner + dep loaded; unrelated NOT loaded; warm path zero-delta |
| `intermediate_hops_navigate_terminal_only_expanded_for_slot_binding_lowering` | `component_meta_terminal_mode_tests.rs` | §5.D.3 — path-precise rule (intermediate Navigate, terminal Expanded) |
| `no_cache_promotion_for_budget_exceeded_slot_binding_lowering` | `component_meta_no_cache_promotion_tests.rs` | §5.D.4 — budget-exceeded must not warm-promote |
| `pathological_nested_slot_definitions` | `component_meta_pathological_recursion_tests.rs` | §5.D.5 — 8-level nested slot binding type terminates without stack overflow |
| `pathological_self_referential_slot_payload` | `component_meta_pathological_recursion_tests.rs` | §5.D.5 — `interface SlotsRec { default: (props: { rec: { inner: SlotsRec['default'] } }) => any; }` self-reference terminates without stack overflow |

## Test counts (final)

```
cargo test --workspace --tests --verbose
  passed=10271 failed=0 ignored=9 blocks=45

cargo test -p verter_session --test correctness
  passed=17 failed=0 ignored=1
```

Pre-Phase-5j (5i marker): `passed=10263 failed=0 ignored=9` workspace,
`passed=15 failed=0 ignored=1` correctness. Phase 5j adds:
- +1 unignored seed test (`resolver_coverage_slot_shapes_*`)
- +5 §5.D tests (.D.2, .D.3, .D.4, .D.5×2)
- +2 Class A fixtures (`fixture_slots_typed`, `fixture_models`)
- +2 §5.B.5.1 rule-correctness gates

= `+8 workspace passes` (5g-supplement instrumentation tests went up
by 2 — the discriminating gate now runs on `fixture_slots_typed` /
`fixture_models` cases that were previously skipped via the
`find(|f| f.id == case.fixture_id)` short-circuit) and `+2
correctness passes`.

Final workspace: `10271 passed`. Final correctness: `17 passed`.

## Class A invisibility gate

No drift on existing Class A snapshots — only the two NEW fixtures
(`fixture_slots_typed.correctness.snap.json`,
`fixture_models.correctness.snap.json`) added; no modifications to
the 19 pre-existing Class A snapshot files (per
`git diff --name-only 1241aa2a..HEAD --
crates/verter_session/tests/correctness/snapshots/`).

## Atomic-gate compliance (r17/Codex-P1#1)

- `status: "success"` (no STOP encountered).
- `deferred[]: []` (nothing deferred — all §5.D tests, fixtures,
  rule-correctness gates, and the seed un-ignore landed in this
  phase).
- `derivation_notes_verified: true` (both Class A fixtures have
  derivation notes citing rule sources per the
  `ensure_class_a_derivation_notes` regex; both rule-correctness gate
  tests pass).

## Verification

End-of-change checks per CLAUDE.md §0.6.3 + §0.6.4:

- `cargo test --workspace --tests --verbose` → 10271 passed, 0 failed.
- `cargo clippy --workspace -- -D warnings` → clean.
- `cargo fmt --all --check` → clean.
- `pnpm install --frozen-lockfile` → no drift.
- `cargo test -p verter_session --test correctness` → 17 passed,
  0 failed (the §5.B.5.1 rule-correctness gates included).
- Class A invisibility gate (no `correctness_snapshot_for_every_fixture`
  panics on pre-existing fixtures) → green.

No `EXPECTS_SNAPSHOT_REGEN` declared; no Class A snapshot drift
observed.
