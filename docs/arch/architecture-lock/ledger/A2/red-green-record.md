# A2 — red-then-green record

All runs on worktree `<REPO>-wt-a2`, branch `block/a2-u6-harness`,
parent `13cedd6fc1315bfb6fec0c4cacb0eacdb02c6c83`. No flow/semantic production code
was changed at any point; RED→GREEN transitions are pin-side only, so every RED below
is a measurement of the PARENT TREE's semantic behaviour.

Oracle stamp: `tsgo 7.0.0-dev.20260526.1 --noEmit --strict --ignoreConfig --pretty false` (checker only).
Profile stamp: `VerterHost standalone { analysis_level: Full, audit_enabled: true, footprint_capture: false, scheduler cpu_threads: 1 }; demand = ReturnProjectionDemand::whole_return(); rail = body-derived FlowReturn via VerterHost::get_flow_return_type_with_audit`.

## Stage discipline

1. **RED stage** — the five rows were pinned with CHECKER-DERIVED recursive expectations
   plus deliberately-unmeasurable boundary JSON (`MEASURE-ME`), and run against the
   parent tree's semantics. Raw output: `command-proofs/01-red-lane-checker-derived-pins.txt`.
2. **GREEN stage** — pins re-measured and re-pinned to the ACTUAL tree behaviour
   (divergence recorded as `KnownOwed` expected-versus-actual gap rows), then the full
   `u6_flow` filterset run green. Raw output: `command-proofs/04-green-all-u6-flow-after-repins.txt`
   (pre-commit) and `command-proofs/06-green-candidate-*.txt` (against the committed candidate).

## Per-row record

| row | checker (tsgo) | parent-tree deep measurement | RED evidence | resolution |
|---|---|---|---|---|
| `X85_nested_closure_write_updates_captured_binding` | `() => "b"` | `() => "b"` — deep-EQUAL | boundary `MEASURE-ME` JSON clause failed (01, X85 block); the checker-derived `Signature{ret:"b"}` expect MATCHED (no expect failure in 01) | pinned `Signature{params:[], ret:Literal "b"}` + exact JSON + `warm_replay:true`; verdict stays `MatchesChecker`. Discrimination proven by control `signature_expectation_rejects_a_different_return` (same shape class, XOR accept) |
| `X87_read_only_let_capture_keeps_reaching_literal` | `() => "a"` | `() => "a"` — deep-EQUAL | boundary clause failed (01, X87 block) | pinned `Signature{ret:Literal "a"}` + exact JSON + `warm_replay:true`; `MatchesChecker`. Control run on X87's own script rejects `ret:"b"` |
| `X88_nested_label_inherits_enclosing_suffix_return` | `"a" \| "b"` | `Union{"a","b"}` — deep-EQUAL (NOT `"a" \| undefined`) | boundary clause failed (01, X88 block) | pinned `Union{Literal "a", Literal "b"}` (exact set) + exact JSON + `warm_replay:true`; `MatchesChecker`. A fabricated `"a" \| undefined` now fails the set-equality pin |
| `N25_impossible_predicate_statement_omits_dead_contributor` | `{ v: "no" \| "ok" }` | **DIVERGENT**: `{ v: Union(DeclRef(A) \| DeclRef(B) \| "ok" \| "no") }` — dead contributor SURVIVES, degradation None, WARM-admitted | **checker-derived expect pin FAILED in 01 (N25 expect block) — the strengthened expectation rejects the parent's indiscriminate result** | re-pinned to the ACTUAL constituent set as a `Verdict::KnownOwed` expected-versus-actual gap (owner `U6.NARROW_LATTICE`); `warm_replay:true` records the wrong-and-warm cache behaviour; `OPEN_DEBTS` += N25; `CONFORMANCE` U6NarrowLattice (7,7,0)→(7,6,1). The owner's fix now trips both the expect pin and the boundary JSON pin |
| `N26_structurally_possible_predicate_intersection_survives` | `{ v: string \| (A & B) }` | `{ v: Union(Intersection(DeclRef(A) & DeclRef(B)) \| string) }` — deep-EQUAL (arms are `DeclRef`, not `BareRef`) | checker-derived stage-1 pin (BareRef arms) FAILED in 01 (N26 expect block) — proving the matcher discriminates the reference-identity trio on a REAL row | re-pinned with `DeclRef` arms + exact JSON + `warm_replay:true`; `MatchesChecker` |

## Negative controls (all in `u6_flow_expect_tests::expectation_controls`; raw: 03/06)

Each control measures a REAL graph node/boundary trace, asserts the truthful pin
accepts, and asserts a differing pin is REJECTED (non-empty failure list):

| control | proves the assertion can fail when |
|---|---|
| `literal_expectation_rejects_a_different_value` | literal value differs (`"a"` vs `"b"`, vs number, vs widened `Primitive(String)`) |
| `signature_expectation_rejects_a_different_return` | signature return differs (`() => "a"` vs `() => "b"`, XOR-exclusive) |
| `union_set_equality_rejects_subset_and_superset` | union constituents are a subset, superset, swapped member, or duplicate-claimed; also proves order-insensitivity |
| `intersection_expectation_rejects_a_wrong_arm` | an intersection arm is wrong or missing |
| `reference_identity_trio_is_distinct` | TypeParam/DeclRef/BareRef pins cross-match (exactly ONE matches a real reference node) |
| `type_param_identity_is_distinct_from_references` | same, from a generic signature |
| `cache_replay_assertion_fails_in_both_directions` | a warm-replaying clean result is pinned cold; a ReturnOnly degraded result is pinned warm (the admitted-warm-when-it-should-not-be case); wrong exact JSON; wrong degradation |
| `stamps_match_the_pinned_oracle_and_profile` | oracle stamp drifts from the pinned tsgo version; profile stamp drops the demand point |

## Matrix red/green

The matrix cells were first written with `PLACEHOLDER-MEASURE-ME` outcomes (structurally
RED — `check_cell_outcome` fails on any rendering mismatch), measured via
`U6_CORPUS_DUMP=1` (raw: `command-proofs/02-matrix-cell-dump-measurements.txt`), then
pinned to the measured outcomes. Divergences from the checker column are recorded
per-cell in the `gap` field / `PositionDependent` note; see `matrix-coverage.md`.

---

# Round 2 — comparator characterization (fix round for the adversarial-mandate FAIL)

Candidate: `80a7d9c328842f1457e866fb8588687e9f1d3118` (tree `eaffd3997f140c2c881179e8089ef6bd05b9bc8d`),
ONE squashed commit parented on `13cedd6fc1315bfb6fec0c4cacb0eacdb02c6c83` (replaces `2744b4081`).

The adversarial finding: the COMPARATORS themselves were uncharacterized — 12 of the
reviewer's 25 comparator mutations survived with the whole suite green (4 clauses of
`check_cell_outcome`, 3 expectation forms in `node_matches`, 3 `check_boundary`
clauses + the warm pair only jointly discriminated). Round 2 adds a control per
clause and proves each by mutation.

## Comparator mutation matrix — 28 mutations, 28 caught

Raw logs + per-mutation plant proofs: `command-proofs/20-comparator-mutation-matrix/`
(`summary.json` binds the runs to file blob `d9aac889c11f312d7476bdda4b0a0a793c32374a`,
byte-equal to the round-2 version of that file (superseded; the current candidate blob is `44f572eb7fe61411a2734ab3d013125ebdb1ee63`)).
Per mutation the runner PROVED the plant applied (original string present exactly once
before, absent after; mutated marker present exactly once; `git diff` non-empty) and
proved restoration (worktree clean, blob identity re-checked) — a silently-no-op plant
is impossible to record as a pass. Every run used
`cargo nextest run -p verter_session -E 'test(u6_flow)' --no-fail-fast` (40 tests).

| # | clause neutered | expected control | result |
|---|---|---|---|
| M1 | `check_cell_outcome` NoValue-pin class (`if !no_value`) | `matrix_outcome_controls::cell_outcome_class_clauses_reject_the_opposite_class` | CAUGHT, only that control failed |
| M2 | Value-pin vs measured refusal (`if no_value`) | same | CAUGHT, only that control failed |
| M3 | rendering comparison | `matrix_outcome_controls::cell_value_clauses_each_reject_a_wrong_pin` | CAUGHT, only that control failed |
| M4 | degradation comparison | same | CAUGHT, only that control failed |
| M5 | warm-replay comparison | same | CAUGHT, only that control failed |
| B1 | `check_boundary` first-call-cold (from_cache) | `boundary_first_call_cold_clauses_fail_against_a_warm_first_trace` | CAUGHT, only that control failed |
| B2 | first-call cold_computes>=1 | same | CAUGHT, only that control failed |
| B3 | degradation guard | `cache_replay_assertion_fails_in_both_directions` (round-1 control) | CAUGHT |
| B4 | exact-JSON guard | same | CAUGHT |
| B5 | warm-pair from_cache clause ALONE | `boundary_warm_pair_clauses_fail_individually` | CAUGHT, only that control failed |
| B6 | warm-pair cold_computes clause ALONE | same | CAUGHT, only that control failed |
| B7 | no-poison clause (warm ReturnOnly) | `cache_replay_assertion_fails_in_both_directions` | CAUGHT |
| B8 | replay-drift comparison | `boundary_replay_drift_and_no_value_clauses_fail` | CAUGHT, only that control failed |
| B9 | Err-carrier clause | same | CAUGHT, only that control failed |
| L1 | literal string equality | `literal_expectation_rejects_a_different_value` | CAUGHT (+ signature & union controls, all value-bearing) |
| U1 | union set size | `union_set_equality_rejects_subset_and_superset` | CAUGHT, only that control failed |
| U2 | union member match | same | CAUGHT, only that control failed |
| I1 | intersection arm count | `intersection_expectation_rejects_a_wrong_arm` | CAUGHT, only that control failed |
| I2 | intersection arm match | same | CAUGHT, only that control failed |
| S2 | signature return match | `signature_expectation_rejects_a_different_return` | CAUGHT, only that control failed |
| O1 | object member count | `object_expectation_rejects_wrong_missing_extra_and_duplicate_members` | CAUGHT, only that control failed |
| O2 | object member NAME | same | CAUGHT, only that control failed |
| O3 | object member VALUE (the `&& true` survivor) | same | CAUGHT (+ opaque-marker control) |
| O4 | object INJECTIVITY (duplicate-key claim) | same | CAUGHT, only that control failed |
| T1 | TypeParam NAME (the `true` survivor) | `type_param_expectation_rejects_a_wrong_name` | CAUGHT, only that control failed |
| R1 | DeclRef NAME | `intersection_expectation_rejects_a_wrong_arm` (WRONG_DECL arm) | CAUGHT, only that control failed |
| BR1 | BareRef NAME (the `true` survivor) | `bare_ref_expectation_rejects_a_wrong_name` | CAUGHT, only that control failed |
| OP1 | Opaque pattern broadened to `Opaque(_)` | `opaque_unmodeled_position_marker_is_discriminating` (different-error program) | CAUGHT, only that control failed |

The reviewer's exact 25-mutation list was not recoverable; this 28-mutation matrix is a
reconstruction covering EVERY comparison clause in the three comparators
(`check_cell_outcome`, `check_boundary`, `node_matches`/`lit_matches`/`set_matches`),
a superset of the named 12 survivors. Post-fix score: 28/28 caught, each by its
designated control (three mutations additionally tripped sibling value-bearing
controls, recorded in `summary.json`).

## Named residual (recorded, not hidden)

`node_matches` Signature arm's `*kind == SignatureKind::Call` strictness check has NO
mutation control: no program reachable through the body-derived FlowReturn rail
measures a `Construct`-kind signature (probed live: `class Box { }; return Box`
renders `{ }`, not a construct signature), so no real measurement exists that the
neutered check would mis-accept. The clause is currently value-indistinguishable
through this rail; it becomes controllable the moment the rail can measure a
construct signature.

## Checker cross-validation (round-2)

`corpus_suite::checker_column_cross_validates_against_live_rendering` drives X85 and
X87 live and asserts `rendered == checker` BYTE-EQUAL (the reviewer's
`checker: "() => \"ZZZ\""` mutation on X87 now fails this lane). X88/N25/N26 are
named RENDER_INCOMPARABLE with per-row reasons (renderer print syntax differs /
KnownOwed divergence is the row's content); both lists are exhaustive over the
deep-pinned population and stale-failing.
`matrix_suite::agreeing_fixed_cells_bind_checker_to_their_pin` binds every agreeing
(gap-less) fixed cell's checker text to its pinned rendering (2 named
render-divergent union-print exceptions must genuinely differ).

## Anti-recurrence floor (round-2)

`verdict_consistency::value_indistinct_rows_carry_deep_pins_or_are_named_shallow`:
every row whose checker names a value the root NodeShape buckets away (Other/Union/
Literal at root or member) must carry an `Expect::Node` pin or be named in the exact
72-row `SHALLOW_PINNED_ROWS` ledger (the round-2 prose miscounted this as 74; corrected in round 3); a deep-pinned row must also carry its
`Boundary::Audit` companion (both directions asserted). Deleting any of the five
repaired rows' pins now fails this floor instead of silently skipping the row green.

## Dump-mode false-green closed (round-2)

`U6_CORPUS_DUMP=1` now FAILS `matrix_cells_hold_their_pins` and
`same_capture_write_cell_is_position_independent` loudly after dumping
(`command-proofs/22-dump-mode-fails-loud.txt`) — a dump run can no longer report an
assertion-free `ok`.

## Round-2 green

`command-proofs/21-green-candidate-eb441c9bf-u6-flow.txt`: 40 tests run, 40 passed
(was 28 in round 1; +12: 9 new negative controls, the cross-validation lane, the
checker-to-pin cell binding, the floor).

## Round 3 — the remaining uncontrolled predicates, the two-call boundary, and the semantic checker rail

Red-first form for this round is the PLANT: each retained comparison clause was
neutered in place (plant proven applied: find unique+present before, replacement
absent before / present exactly once after; file byte-restored and re-verified
after every run) and the suite re-run — the designated control, and only a
designated control, went RED for all 54 mutations
(command-proofs/30-round3-mutation-matrix/mutation-results.log; 54/54
CAUGHT-BY-NAMED-CONTROL, 0 SURVIVED). The two corpus-level hatches the round-3
review proved are demonstrated red: moving X85 into RENDER_INCOMPARABLE with a
bogus reason fails checker_column_cross_validates_against_live_rendering (the
byte-divergence claim is now verified live), and editing N26's checker column to
the wrong intersection arm fails
deep_pinned_rows_semantic_equality_follows_their_verdict (the checker column is
now compared SEMANTICALLY for every deep-pinned row, verdict-directed).
Unexercised comparison vocabulary was DELETED rather than controlled:
SignatureKind::Call — RESTORED. The earlier deletion rested on a sample-probe
argument that no Construct signature is reachable; that argument is FALSE (the disproving form is `function makeProps(x: new () => Box) { return x }`, which measures a genuine `SignatureKind::Construct` returning `DeclRef(Box)`; the three probes below merely fail to reach one — 
`return class C {}` renders `any`, `{ new (): Box }` renders `{  }`,
`new () => Box` aliases to `DeclRef(C)`), Lit::Bool / Lit::BigInt, and alias
transparency (neuter probe left the whole suite green — unexercised; matcher now
fail-closed on Alias). The driver itself was caught once returning
SURVIVED-everywhere on a broken filterset invocation (zero tests ran) and now
hard-fails any run that cannot prove >= 40 executed tests — a non-run is never a
survival.
