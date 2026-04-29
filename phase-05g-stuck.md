# Phase 05g — STUCK report (sub-plan §F STOP)

**Phase id:** 05g
**Branch:** `wt/phase-05g-engine-deletion-fixtures`
**Base commit at spawn:** `3147c02f44ed4fc3fdc1a50d6f51929c7a4a0c18`
**Worktree HEAD at stop:** `d6324973`
  (`test(meta): lib parity (Pick/MyPick equivalence) + dispatch shadow gate`)
**Worktree:** `D:/dev/wt/phase-05g-engine-deletion-fixtures`
**Sub-plan:** `D:/tmp/verter-architecture-cutover-phase-05.md`

## TL;DR

Phase 5g's brief mandates three commits:

1. **Commit 11** — engine deletion (~3500-5500 LOC removal) +
   close 3 deferred seeds (`slot_shapes`, `mapped_types`,
   `package_backed`).
2. **Commit N+1** — lib parity tests (per parent §5.C).
3. **Commit N+2** — author 7 Class A fixtures whose rule-correct
   expected values Verter currently does not produce.

The brief's §F STOP condition is binding:
> If Verter STILL doesn't match post-Phase-5, that's a STOP — the
> variant did not close the gap; sub-plan §F STOP condition.

Verter's current output (verified at this worktree's HEAD with
post-Phase-5f migrations and the new shadow-fix landed) does NOT
match the rule-correct expected for ANY of the 7 deferred fixtures
NOR for 1 of the 2 lib parity tests. Three landmark gaps remain
open per the deferral notes carried forward from
`phase-05f-complete`.

This worker landed commit N+1 (lib parity) AND closed one of its
two assertions (`MyPick === Pick` parity) via a clean architectural
fix in `project_semantic_dispatch/lower.rs`. The other parity test
(`shadowed_pick_is_userland_not_intrinsic`) is `#[ignore]`'d with a
detailed reason. The worker did NOT attempt commit 11 (engine
deletion) or commit N+2 (fixture authoring) past the assessment
phase, because per §F STOP rule completing them would require
landing a tree where the gates Phase 5g was supposed to close are
demonstrably still open.

## What landed

### Commit `d6324973` — lib parity (Pick/MyPick equivalence) + dispatch shadow gate

`test(meta): lib parity (Pick/MyPick equivalence) + dispatch shadow gate`

Files modified / added:
- `crates/verter_session/src/project_semantic_dispatch/lower.rs`
  (+19 / -3) — added `shadowed_by_scope` check in
  `shallow_lower_type_expr`'s `TypeExpr::Ref` branch. The builtin
  utility fast-path is now suppressed not only when
  `name_resolution.contains_key(name)` but also when
  `scope_payload.scope_type_names.contains(name)` (or
  `scope_type_bindings`). This covers the case where
  `lower_type_expr_in_scope_with_mode` is invoked with an empty
  `name_resolution` map but a populated `scope_payload` (the
  arbitrary expression projection path).
- `crates/verter_session/tests/component_meta_audit.rs` (+7) —
  registered the new `lib_parity` submodule.
- `crates/verter_session/tests/component_meta_audit/lib_parity.rs`
  (+223) — two parity tests per parent §5.C.

Tests:
- `pick_and_my_pick_produce_identical_props` — PASS. Userland
  `MyPick<T,K>` produces the same surface as ambient `Pick<T,K>`.
- `shadowed_pick_is_userland_not_intrinsic` — `#[ignore]`'d per §F
  STOP. The `lower.rs` shadow gate fixes the lowering side, but the
  materialize-path's `extract_route_root_identity_node` recognises
  `__builtin__/Pick` syntactically and bypasses the gate.

Workspace verification (post-commit):
- `cargo test --workspace --tests --verbose` (logged to
  `/tmp/p05g-workspace-c1.txt`): 44 test blocks, 10212 passed, 0
  failed, 11 ignored.
- `cargo clippy --workspace --tests --no-deps -- -D warnings`:
  clean.
- `cargo fmt --all --check`: clean.

## Why I stopped

### Reason 1 — §F STOP — 7 deferred Class A fixtures still mismatch

Per parent §5.B.5 + sub-plan §5 commit N+2, the worker is required
to author 7 fixtures whose rule-correct expected values are carried
forward from `phase-00-tier1-mismatches.md` (5) and
`phase-00b-tier1-mismatches.md` (2). The brief is explicit:

> If Verter STILL doesn't match post-Phase-5, that's a STOP — the
> variant did not close the gap

The worker ran the harness's
`generate_class_a_snapshots_from_expected` against the rule-correct
expected, then ran `correctness_snapshot_for_every_fixture` to
compare Verter's actual output against the generated `.snap.json`.
ALL 7 fail:

| Fixture | Verter actual | Expected | Root cause |
|---|---|---|---|
| `mapped_exclude` | `kind: /*unknown*/ semanticMiss` | `kind: "a" \| "c"` | `Exclude<>` distributive conditional not evaluated |
| `mapped_extract` | `kind: /*unknown*/ semanticMiss` | `kind: "a" \| "b"` | Same root cause as `mapped_exclude` |
| `template_literal_as_key` | `props = []` | `props = [prefixA: number, prefixB: number]` | template-literal mapped-key iteration silently drops keys |
| `generic_substitution_via_typeof` | `props = []` | `props = [id: string]` | `IdShape<typeof sample.id>` substitution skipped |
| `userland_shadowing_pick` | `props = [alpha]` | `props = [alpha, beta, gamma]` | macro path bypasses scope-walk to ambient lib's Pick |
| `fixture_slots_typed` | `payload_signature: { item: /*unknown*/ semanticMiss }` | `{ item: string }` | slot-binding-parameter type extraction not lowered |
| `fixture_models` | `model.type_signature: /*unknown*/ semanticMiss` | `string` / `number` | `defineModel<T>()` macro's T-resolution diverges from event-payload path |

The 7 fixture sources, expected values, derivation notes, and
.snap.json files were prepared but NOT committed (would leave
workspace red). Worker observed each gap matches the gap
description from the design-input docs verbatim — Phase 5's
variant migrations did not close them.

### Reason 2 — 3 deferred seeds remain RED

Per `phase-05f-complete` marker, three seeds remain RED with
explicit deferral notes pinning them to 5g:

- `resolver_coverage_slot_shapes` (`fixture_slots_typed` row 1) —
  the `ResolveMacroPayload::DefineSlots` arm dispatches but the
  slot-binding-parameter extractor at `meta_resolve.rs::DefineSlots`
  arm walks raw `TypeExpr` rather than consulting the dispatch
  result.
- `resolver_coverage_mapped_types` (`mapped_exclude` row 1) —
  `Exclude<>` requires concrete relation engine reduction
  (literal-equality check). Phase 5f's open-Conditional empty-path
  distribution does not apply here because the conditional is
  bound to concrete string literals.
- `resolver_coverage_package_backed` — fixture's `/c.vue` workspace
  root path blocks `resolve_node_modules_package` ancestor walk.
  Even when fixed, the negative assertion's `event` (function
  parameter) is vacuous.

These seeds are scoped to 5g per the deferral notes. Closing them
requires (per the notes' own diagnosis):

1. Slot-binding-parameter migration in `meta_resolve.rs::DefineSlots`
   arm — distinct migration, not subsumed by engine deletion.
2. Concrete literal-equality reduction path for `Exclude<>` —
   relation-engine work.
3. Harness fix to seat fixtures deeper than workspace-root for
   `resolve_node_modules_package` to work, plus a discriminating
   fixture replacement for the package-backed gate.

Each is a non-trivial change that must land alongside a careful
audit of the resolver paths involved. The deferral notes already
flag these as scope changes beyond the engine's trampoline
deletion.

### Reason 3 — Engine deletion has many production callsites

Per sub-plan §4.3 deletion gate, commit 11's prerequisite is that
ALL retired engine methods have NO production callers:

```bash
rg --files-with-matches \
   '(project_(expr_surface_expr(_with_compound_objects)?|expr_surface_shape|...)|lower_and_project_to_expanded|instantiate_local_generic_ref)' \
   crates/
```

Expected output: ONLY the engine file. Actual output at this
worktree's HEAD (logged to `/tmp/p05g-deletion-gate.txt`):

```
crates/verter_session/src/resolver_core/fallthrough.rs
crates/verter_session/src/resolver_core/component_meta_query_engine.rs
crates/verter_session/src/host_manage_tests.rs
crates/verter_session/src/host_manage.rs
crates/verter_session/src/d_cutover_characterization_tests.rs
crates/verter_session/src/parity_tests.rs
crates/verter_session/src/meta_tests.rs
crates/verter_session/src/meta_resolve_tests.rs
crates/verter_session/src/meta_resolve.rs
crates/verter_session/tests/architecture_guards.rs
crates/verter_session/tests/component_meta_audit/resolver_coverage_indexed_paths.rs
crates/verter_session/tests/component_meta_audit/resolver_coverage_inherited_emits.rs
crates/verter_session/tests/component_meta_audit/resolver_coverage_package_backed.rs
```

The gate is wide open. `meta_resolve.rs` alone has 15+ active
production callsites of retired methods (lines 145, 149, 3140,
4912, 4946, 5013, 5099, 5223, 5256, 5283, 5325, 6513, 9439, 9445,
12076, 12101 — verified via `rg`). `host_manage.rs:2303` calls
`project_type_surface_expr` from production. `cmqe.rs` itself has
~10 engine-internal callsites of `instantiate_local_generic_ref`
and `project_route_surface_expr` from inside other engine methods
(lines 1660, 1698, 3604, 3605, 3849, 3854, 4004, 4008, 4242, 4249,
4260, 4263, 4270, 4393, 4496, 4787, 5174, 5175, 5745).

Per the sub-plan's worker discipline rule, every commit must end
with the workspace green. Migrating these callsites under that
discipline would require dozens of commits, each with workspace
verification. The Phase 5e/5f deferral notes acknowledge these
sites are retained "until 5g" — but 5g's brief frames the deletion
as a single atomic commit (with an option to split).

### Reason 4 — workspace-green discipline forbids landing failing fixtures

Adding the 7 fixtures to `FIXTURES` registry (the way commit N+2
should land) makes
`correctness_snapshot_for_every_fixture` panic on the first
mismatch — leaving the workspace red. Per
"Workspace-green discipline (mandatory per CLAUDE.md):
> ZERO TOLERANCE.

Adding `#[ignore]` to the parametric correctness gate is not an
option (it iterates `FIXTURES` directly). Removing the fixtures
from `FIXTURES` after adding their expected/derivation/snapshot
files would leave the constants dead-code. The brief's "split into
multiple sub-commits" allowance does not extend to "land
infrastructure but skip the activation step that the brief
explicitly requires".

The clean architectural path is to land the resolver work that
closes the 7 gaps FIRST, then activate the fixtures. The resolver
work is not single-commit-tractable (see Reasons 1+2+3 above).

## What the next phase / continuation worker needs

The right shape for closing this is a focused continuation phase
with three resolver-side workstreams, each landing as its own
commit:

1. **`Pick`/`Omit` route extraction respects scope shadowing** —
   thread the `shadowed_by_scope` gate through
   `extract_route_root_identity_node` AND
   `component_meta_registry_public_utility_route` so the
   materialize fast-path defers to the dispatch shadow when the
   userland Pick is in scope. Then un-ignore
   `shadowed_pick_is_userland_not_intrinsic` and add the
   `userland_shadowing_pick` fixture.

2. **`Exclude<>`/`Extract<>` concrete-literal reduction** — the
   relation engine's literal-equality check must drive the
   distributive conditional reduction so `Exclude<'a'|'b'|'c', 'b'>`
   evaluates to `'a' | 'c'`. Closes seed
   `resolver_coverage_mapped_types` and the `mapped_exclude` /
   `mapped_extract` fixtures.

3. **Slot-binding-parameter type lowering migrates to dispatch** —
   the slot-binding extractor at `meta_resolve.rs::DefineSlots` arm
   currently walks raw TypeExpr; it should consult the dispatch
   result for the binding-parameter types. Closes seed
   `resolver_coverage_slot_shapes` and the `fixture_slots_typed`
   fixture.

4. **Other gap closures** — `template_literal_as_key`
   (template-literal mapped-key iteration through dispatch);
   `generic_substitution_via_typeof` (typeof-substitution thread
   through `Instantiate`); `fixture_models` (`defineModel<T>` T
   resolution alignment with the `update:<name>` event payload
   path); `resolver_coverage_package_backed` (harness fix +
   discriminating fixture replacement).

Once those land, the engine deletion can proceed atomically (or in
2-3 commits per call-class), because every retired engine method
will have a clean dispatch-side replacement in `meta_resolve.rs`'s
production callers.

## Marker disposition

The brief mandates a final `phase-05g-complete` marker as the gate
for downstream Phase 4 / Phase 8 / Phase 11. Per §F STOP rule the
worker does NOT land the success marker. This stuck-file commit is
the worker's STOP escalation; the orchestrator's marker validation
will recognise the absence of the success marker and route to the
continuation flow.

The lib parity test (commit `d6324973`) is real, useful work that
should land regardless of the larger phase outcome. It is committed
on this branch and is workspace-green.

## Files of interest for continuation

- `D:/dev/personal/verter/phase-00-tier1-mismatches.md` — 5 fixture
  rule-correct expected
- `D:/dev/personal/verter/phase-00b-tier1-mismatches.md` — 2
  fixture rule-correct expected
- `crates/verter_session/src/project_semantic_dispatch/lower.rs` —
  partial shadow gate (this commit)
- `crates/verter_session/src/meta_resolve.rs:10460-10547` —
  `extract_route_root_identity_node` needs the gate
- `crates/verter_session/src/resolver_core/component_meta_registry.rs:1710-1735` —
  `component_meta_registry_public_utility_route` needs the gate
- `crates/verter_session/src/component_meta_materialize.rs:511-560` —
  the materialize Pick/Omit path that bypasses scope shadowing
- `crates/verter_session/tests/component_meta_audit/lib_parity.rs` —
  the parity tests (1 PASS, 1 ignored with the precise scope-shadow
  gap reason)
- `/tmp/p05g-workspace-c1.txt` — workspace test results post-commit
- `/tmp/p05g-deletion-gate.txt` — engine deletion gate failure log

## Test counts (this worktree HEAD `d6324973`)

```
test result: ok. 1831 passed; 0 failed; 1 ignored  (verter_session lib)
test result: ok. 21 passed; 0 failed; 4 ignored    (component_meta_audit incl. lib_parity)
test result: ok. 11 passed; 0 failed; 1 ignored    (correctness)

Workspace aggregate: 10212 passed / 0 failed / 11 ignored
                      (44 test blocks)
```

## Recommendation to orchestrator

Treat this worktree's commit `d6324973` as a partial-deferred land
(per the precedent in `phase-06 partial-deferred report`):

1. Lib parity tests (`pick_and_my_pick_produce_identical_props`)
   land cleanly and provide ongoing regression coverage for the
   userland-utility-equivalence rule.
2. The `shadowed_by_scope` shadow gate in
   `project_semantic_dispatch/lower.rs` is a clean architectural
   fix that closes the userland-`MyPick` parity case.
3. The remaining work (engine deletion + 3 seeds + 7 fixtures) is
   re-scoped to a continuation phase that focuses on the
   resolver-side gap closures FIRST, then proceeds with the
   atomic engine deletion.

The work decomposes naturally into the four resolver workstreams
listed above. Each is a focused commit with clear before/after
characterisation tests already authored (the 7 fixtures + the
ignored parity test).
