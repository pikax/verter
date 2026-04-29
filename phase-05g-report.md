# Phase 05g — Worker report

**Phase id:** 05g
**Branch:** `wt/phase-05g-engine-deletion-fixtures`
**Base commit at spawn:** `3147c02f44ed4fc3fdc1a50d6f51929c7a4a0c18`
**Worktree HEAD at end:** `33614b41`
**Status:** STOPPED per sub-plan §F (see `phase-05g-stuck.md`)

## Summary

Phase 5g's brief mandates three commits:

1. **Commit 11** — engine deletion (~3500-5500 LOC) + close 3
   deferred seeds (`slot_shapes`, `mapped_types`, `package_backed`).
2. **Commit N+1** — lib parity tests (parent §5.C).
3. **Commit N+2** — author 7 Class A fixtures whose rule-correct
   expected values Verter currently does not produce.

Per the brief's §F STOP condition:
> If Verter STILL doesn't match post-Phase-5, that's a STOP — the
> variant did not close the gap.

This condition fires on multiple workstreams. The worker:

- Landed commit N+1 (lib parity) with one of two assertions PASSING
  via a clean architectural fix (`d6324973`).
- Did NOT land commit 11 (engine deletion) — §4.3 deletion gate
  fails with 15+ production callsites in `meta_resolve.rs`.
- Did NOT activate commit N+2 (7 fixtures) — Verter currently
  outputs wrong values for ALL 7 deferred Class A fixtures (per the
  carried-forward deferral notes).

The stuck-file commit (`33614b41`) is the worker's STOP escalation
per the brief's STOP path.

## Commits made

| SHA | Message |
|---|---|
| `d6324973` | `test(meta): lib parity (Pick/MyPick equivalence) + dispatch shadow gate` |
| `33614b41` | `docs(orchestrator): phase 05g STUCK report (sub-plan §F STOP)` |

### Commit `d6324973` — Lib parity tests + dispatch shadow gate

`crates/verter_session/src/project_semantic_dispatch/lower.rs`
(+19 / -3): added `shadowed_by_scope` gate to the builtin-utility
fast-path. Previously `lower_type_expr_in_scope_with_mode` (called
on arbitrary expression projection) used an empty `name_resolution`
map, so userland aliases declared in the same-file scope were
silently overridden by ambient lib builtins. The gate now also
checks `scope_payload.scope_type_names` — covering the same-file
shadow case the plan's "user shadowing wins" rule mandates.

`crates/verter_session/tests/component_meta_audit.rs` (+7) +
`crates/verter_session/tests/component_meta_audit/lib_parity.rs`
(+223): two parity tests per parent §5.C.

- `pick_and_my_pick_produce_identical_props` — PASS. Userland
  `MyPick<T,K extends keyof T> = { [P in K]: T[P] }` produces the
  same surface as ambient `Pick<T,K>` over `Cfg`. Discriminating
  positive (both produce `[alpha, beta]`) and negative
  (neither contains `gamma`) assertions.
- `shadowed_pick_is_userland_not_intrinsic` — `#[ignore]`'d per §F.
  Userland `Pick<T,_K> = T` should shadow ambient lib's Pick (rule:
  ALL three Cfg members surface). Currently Verter still routes to
  lib's mapped Pick because `extract_route_root_identity_node`
  (materialize-path) syntactically recognises `Pick<X, Y>` based on
  name without consulting the dispatch shadow gate. Closing this
  requires propagating the gate through the route extraction.

### Commit `33614b41` — STUCK report

`phase-05g-stuck.md` (+324). Documents per-fixture diff, deletion
gate failure log, per-seed deferral carry-forward, and four
recommended continuation workstreams.

## Engine LOC reduction

**0 lines.** No engine code was deleted in this phase. Sub-plan §4.3
deletion gate failed at this worktree's HEAD — `meta_resolve.rs`
alone has 15+ production callsites of methods slated for retirement
(verified via `rg`, logged to `/tmp/p05g-deletion-gate.txt`).

## §5.A seed test status

3 seeds remain RED, deferred to a continuation phase per
`phase-05g-stuck.md`:

| Seed | Status | Reason |
|---|---|---|
| `resolver_coverage_inherited_emits` | GREEN (closed in 5f c7) | — |
| `resolver_coverage_indexed_paths` | GREEN (closed in 5f c8) | — |
| `resolver_coverage_slot_shapes` | RED (`#[ignore]`'d) | slot-binding-parameter extractor still walks raw TypeExpr |
| `resolver_coverage_mapped_types` | RED (`#[ignore]`'d) | `Exclude<>` requires concrete relation engine reduction |
| `resolver_coverage_package_backed` | RED (`#[ignore]`'d) | harness fix needed (workspace-root fixture path) |

## 7 deferred Class A fixtures status

Authoring scaffolding (sources, expected.rs functions, derivation
notes, snapshots) was prepared and verified to compile against the
rule-correct expected. NOT committed because activating them in
`FIXTURES` makes `correctness_snapshot_for_every_fixture` panic on
the first mismatch — leaving workspace red, violating the brief's
ZERO TOLERANCE workspace-green discipline.

| Fixture | Verter actual | Rule-correct expected |
|---|---|---|
| `mapped_exclude` | `kind: /*unknown*/ semanticMiss` | `kind: "a" \| "c"` |
| `mapped_extract` | `kind: /*unknown*/ semanticMiss` | `kind: "a" \| "b"` |
| `template_literal_as_key` | `props = []` | `props = [prefixA: number, prefixB: number]` |
| `generic_substitution_via_typeof` | `props = []` | `props = [id: string]` |
| `userland_shadowing_pick` | `props = [alpha]` | `props = [alpha, beta, gamma]` |
| `fixture_slots_typed` | `payload: { item: /*unknown*/ semanticMiss }` | `{ item: string }` |
| `fixture_models` | `model.type: /*unknown*/ semanticMiss` | `string` / `number` |

ALL 7 fail. The gap-description matches the carry-forward deferral
notes verbatim.

## Class A parity gate status (the 16 pre-existing snapshots)

**No drift.** Class A invisibility holds. The pre-existing 16 Class
A snapshots (11 from Phase 0a + 5 from Phase 0b) are byte-equal
between spawn HEAD and this worktree's HEAD. Verified by:

- `git status crates/verter_session/tests/correctness/snapshots/`
  shows no modified files (only untracked candidate files for the 7
  new fixtures, which were reverted).
- `cargo test -p verter_session --test correctness` PASSES (all
  pre-existing Class A snapshots still byte-equal).

## Test pass counts (cited from `/tmp/p05g-workspace-c1.txt`)

`cargo test --workspace --tests --verbose` post-commit `33614b41`:

```
44 test blocks
10212 passed
0 failed
11 ignored (1 added by this phase: shadowed_pick_is_userland_not_intrinsic)
```

Block listing (test result lines from cargo output):

```
1831 passed; 0 failed; 1 ignored   (verter_session lib)
2947 passed; 0 failed; 0 ignored   (verter_workspace)
1165 passed; 0 failed; 0 ignored   (verter_compiler)
 990 passed; 0 failed; 0 ignored   (verter_protocol)
 789 passed; 0 failed; 0 ignored   (verter_semantic)
 431 passed; 0 failed; 0 ignored   (verter_actions)
 177 passed; 0 failed; 0 ignored   (component_meta_audit_corpus)
 149 passed; 0 failed; 0 ignored   (verter_oxc_parser)
 141 passed; 0 failed; 0 ignored   (verter_lsp lib)
 100 passed; 0 failed; 0 ignored   (host_tests)
  89 passed; 0 failed; 0 ignored   (verter_diagnostics)
  77 passed; 0 failed; 0 ignored   (verter_actions integ)
  41 passed; 0 failed; 0 ignored   (verter_napi)
  35 passed; 0 failed; 0 ignored   (relate_disambiguation)
  27 passed; 0 failed; 0 ignored   (verter_lsp_macros)
  21 passed; 0 failed; 4 ignored   (component_meta_audit incl. lib_parity)
  17 passed; 0 failed; 0 ignored   (verter_diagnostics integ)
  11 passed; 0 failed; 1 ignored   (correctness)
  10 passed; 0 failed; 0 ignored   (ts_bindings)
   9 passed; 0 failed; 0 ignored   (verter_session host integ)
   8 passed; 0 failed; 0 ignored   (verter_session integ)
   7 passed; 0 failed; 5 ignored   (verter_session ext)
   7 passed; 0 failed; 0 ignored   (corpus_audit_tests)
   6 passed; 0 failed; 0 ignored   (verter_lsp_macros integ)
   5 passed; 0 failed; 0 ignored   (scheduler_worker_tls_propagation)
   4 passed; 0 failed; 0 ignored   (origin_graph_audit_contract)
   4 passed; 0 failed; 0 ignored   (audit_phase_1_e2e)
   4 passed; 0 failed; 0 ignored   (audit_synthetic_fixtures)
   3 passed; 0 failed; 0 ignored   (corpus_generator_parity)
   2 passed; 0 failed; 0 ignored   (audit_docs)
   2 passed; 0 failed; 0 ignored   (legacy_walker_parity_discrimination)
   2 passed; 0 failed; 0 ignored   (resolved_no_residual_operator_leaves)
   1 passed; 0 failed; 0 ignored   (architecture_guards)
   1 passed; 0 failed; 0 ignored   (audited_request_e2e)
   1 passed; 0 failed; 0 ignored   (baseline_trace_alloc_count)
   1 passed; 0 failed; 0 ignored   (legacy_trace_cutover)
   1 passed; 0 failed; 0 ignored   (legacy_walker_parity_baseline)
   1 passed; 0 failed; 0 ignored   (no_legacy_walker)
   1 passed; 0 failed; 0 ignored   (origin_graph_consumer_contract)
   1 passed; 0 failed; 0 ignored   (relative_path_session_parity)
   0 passed; 0 failed; 0 ignored   (verter_oxc_parser dummy)
   0 passed; 0 failed; 0 ignored   (verter_compiler dummy)
   0 passed; 0 failed; 0 ignored   (verter_diagnostics dummy)
```

(The 4 ignored in component_meta_audit incl. 1 from this phase
+ 3 pre-existing seed defers; the 1 ignored in correctness is the
`generate_class_a_snapshots_from_expected` runner ignored by
design.)

Block count = 44, ≥ 40 (§0.4 r11 floor satisfied).

## §0p.A.5 discriminating-test row status

Per `phase-05g-stuck.md` Reason 4, the 7 fixtures were NOT
activated in `FIXTURES`, so the §0p.A.5 parametric self-test
remains at the same coverage as Phase 0b: 9 of 12 rows live, 3 SKIP
mechanically because the fixtures they reference are not in the
registry. Status unchanged from Phase 0b.

## Total Class A fixture count post-5g

**16** (11 from Phase 0a + 5 from Phase 0b), unchanged from Phase
0b. The 7 brief-target fixtures could not be activated due to
Verter's resolver gaps (per §F STOP). Post-continuation phase the
target count is **23** as specified.

## Marker JSON path

**No success marker landed.** Per the brief's §F STOP rule and the
precedent of `phase-06 partial-deferred report` (`fbc93401`), the
worker did NOT create
`crates/verter_session/.phase-markers/phase-05g-complete`. The
absence of this marker signals to the orchestrator that the phase
is in STUCK state and a continuation is required.

## work_head_before_marker SHA

The brief's R7 marker schema is not applicable here (no marker
landed). For orchestrator visibility, the work HEAD is `33614b41`
(the stuck-file commit).
