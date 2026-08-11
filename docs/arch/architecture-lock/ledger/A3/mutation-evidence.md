# A3 mutation evidence

## Scope and clean baseline

- Candidate lineage: parent `20acec177`, reviewed candidate `d1aade9ee` (tree-identical to the originally recorded `bdd1aabcf`).
- Evidence was executed in the isolated clone `A3/mutation-worktree` so the required clean-file status checks did not disturb the implementer's preserved dirty rename pass.
- The isolated baseline commit was `81d3f85044782851ad454d03e28e12edb6cc650b`. It contains the candidate, the preserved rename, and the FIX changes under test. This evidence-only commit is not in the implementation worktree.
- The inventory is nine correctness-bearing tests/assertion groups (T1-T9) and six production degradation/refusal mechanisms (G1-G6). T6 is the new optional-member false-refusal regression; T9 directly exercises the cohort anti-shrink equality.

## Reversible recipe protocol

Every table row was executed independently with the following protocol. The recorded replacement contains a row-specific `MUTATION_EVIDENCE_<ID>` marker.

1. Require `git rev-parse HEAD` to equal `81d3f85044782851ad454d03e28e12edb6cc650b` and require `git status --porcelain -- <file>` to be empty.
2. Save the original file bytes and their SHA-256. Require the exact source needle to occur once and the mutation marker to occur zero times.
3. Replace that one needle. Require the old needle to occur zero times, the marker to occur exactly once, and the planted SHA-256 to differ from the original. These checks prove the plant is present, unique, and new in the source; an editor/process exit code is not used as proof.
4. Run `cargo nextest run -p verter_session -E 'test(<named test>)' --no-fail-fast`. Require a nonzero exit and a nextest `FAIL`; a surviving mutation or an unproven plant fails the recipe.
5. Restore the saved bytes in a `finally` block. Require the restored SHA-256 to equal the starting SHA-256 and require `git status --porcelain -- <file>` to be empty.

## Test mutations

| ID | File | Reversible mutation | Named test / result | Plant SHA-256 | Restored SHA-256 / status |
|---|---|---|---|---|---|
| T1 | `crates/verter_session/src/u6_flow_gap_retraction_tests.rs` | Replace the G1 impossible-guard fixture with `return "live"` while retaining its expected `GuardNarrowing`. | `flow_gap_known_gap_results_are_typed_partial_and_never_warm` — **RED** | `CDF332DD3370DABF6C7B7892CC3DA8D6BB45AAE2E31A5113191C03DD0BB7BEFE` | `FEAC33A59E1A8878EF32DF7BCCA5D2CC8997B71694FAD61ABE45CBFFBA96A5FA`; EMPTY |
| T2 | same | Change the invoked-closure expected error from `Some(IIFE_EFFECT_REFUSAL)` to `None`. | `flow_gap_invoked_closure_effect_is_position_independent_no_value` — **RED** | `27BBA7394C4CF50489CFEB1660D411A7AC0DDC5BCA01332ABE1E0F66A5DB67CA` | `FEAC33A59E1A8878EF32DF7BCCA5D2CC8997B71694FAD61ABE45CBFFBA96A5FA`; EMPTY |
| T3 | same | Add nonexistent function `missing` to the authored-`any` complete/warm controls. | `flow_gap_authored_any_remains_complete_and_warm` — **RED** | `ED6C31B80865BE727D910F59ED8643C7F9B9622379C81C3B8C63EC4C864FC07F` | `FEAC33A59E1A8878EF32DF7BCCA5D2CC8997B71694FAD61ABE45CBFFBA96A5FA`; EMPTY |
| T4 | same | Reduce the default-parameter nesting plant from `0..65` to `0..1`, removing the intended budget failure. | `flow_gap_default_parameter_budget_failure_is_no_value_and_cold` — **RED** | `38306EB87DF024B976D8964C655BC29C11672EE271B4E65E0B4670C4F3601850` | `FEAC33A59E1A8878EF32DF7BCCA5D2CC8997B71694FAD61ABE45CBFFBA96A5FA`; EMPTY |
| T5 | same | Change the expected propagated root gap from `UnmodeledExpression` to `GuardNarrowing`. | `flow_gap_partial_propagates_through_consumer_and_scc_gates` — **RED** | `4491C2DDCBFA7D59F59562D483D4BD6E7FEFA4779125B8802D016CA3492B881A` | `FEAC33A59E1A8878EF32DF7BCCA5D2CC8997B71694FAD61ABE45CBFFBA96A5FA`; EMPTY |
| T6 | `crates/verter_session/src/flow_slice_content.rs` | Remove the semantic-`any` optional-member exception by making the unmodelled predicate unconditional. | `flow_gap_false_refusal_controls_remain_complete_and_warm` — **RED** | `0FFD265E7417FA85011165D791D87BC81F6EF46D678DB37881453AD073FB2C21` | `CD23A55C008EC01D7D43510DDD0F9EB20F917A82743AE32AA89CE54C3E999605`; EMPTY |
| T7 | `crates/verter_session/src/u6_flow_expect_tests.rs` | Invert the uniform IIFE refusal assertion from `assert_eq!` to `assert_ne!`. | `uniform_iife_effect_refusal_covers_every_position` — **RED** | `C4612DB2EEF689F333A6C48698EDC3F385EB48E5A6A7C0CDECAF51BCAC5DB815` | `9206778FEB2D91DB5AEEA8EC4A687019E5E09B801468E4D82C3C3BC5000C914D`; EMPTY |
| T8 | `crates/verter_session/src/u6_flow_shape_corpus_tests.rs` | Change the first preservation fingerprint nibble (`d...` to `0...`). | `flow_gap_retraction_preserves_clean_checker_matches` — **RED** | `4593A9E1137CF78D342E6F406F0246E86E7779B24BB790BD5A7C3C429C53AF87` | `47FD9FEF07250EC193935FDB52A5CA7539ECC8E88C08D34E15482D5A30C14FA0`; EMPTY |
| T9 | same | Drop `X05_catch_return_fallthrough` from the computed current cohort while leaving the locked cohort unchanged, directly exercising `locked_ids == current_ids`. | `flow_gap_retraction_preserves_clean_checker_matches` — **RED** | `5C7E31165B5C7068D76CCEDD8F2FBF94423811FE45ED9D71331E9A1039403FAC` | `47FD9FEF07250EC193935FDB52A5CA7539ECC8E88C08D34E15482D5A30C14FA0`; EMPTY |

## Production guard/refusal mutations

| ID | File | Reversible mutation | Named test / result | Plant SHA-256 | Restored SHA-256 / status |
|---|---|---|---|---|---|
| G1 | `crates/verter_session/src/project_semantic_dispatch/flow_return.rs` | Make `record_degradation` discard every `FlowGap::GuardNarrowing` transfer. | `flow_gap_known_gap_results_are_typed_partial_and_never_warm` — **RED** | `AF82631EE782575644F89E6397CF86557A085DB06F552A30B6D76EF0D27F3263` | `79B922F035E4ADC99813C5CAC174C3F9253CD3FCA5BA110689E87D7146C05563`; EMPTY |
| G2 | same | Relabel the nominal-identity transfer from `NominalRelation` to `GuardNarrowing`. | `flow_gap_known_gap_results_are_typed_partial_and_never_warm` — **RED** | `45EE0F3D09DCDE08360C224BDB3410A40B72F76B1E7B27E277363102BCE85DEC` | `79B922F035E4ADC99813C5CAC174C3F9253CD3FCA5BA110689E87D7146C05563`; EMPTY |
| G3 | `crates/verter_session/src/flow_slice_content.rs` | Remove the `ClosureCapture` degradation assignment from the closure-gap guard. | `flow_gap_known_gap_results_are_typed_partial_and_never_warm` — **RED** | `BE418E51BFA83C5D89A0917F32214C337DF217E82D7FF217AE78149147EF7E5E` | `CD23A55C008EC01D7D43510DDD0F9EB20F917A82743AE32AA89CE54C3E999605`; EMPTY |
| G4 | `crates/verter_semantic/src/analysis/type_eval_build.rs` | Stop setting `used_unmodeled_fallback` in the catch-all expression lowering arm. | `flow_gap_known_gap_results_are_typed_partial_and_never_warm` — **RED** | `A1A2B1B94ACFF27BE6936DE29045FD17FDB7505C1D8F047CBD14F3C269D7F661` | `C2FC53A0757FD664E32F480B5AC8EA74CB6B6153736D9BBA10495AA21467A847`; EMPTY |
| G5 | `crates/verter_session/src/flow_slice_content.rs` | Disable the statement-level unsafe-invoked-closure refusal predicate. | `flow_gap_invoked_closure_effect_is_position_independent_no_value` — **RED** | `34667D7E4B38933B371C8874B2D0D32539845DC4FF164E7D4A70B2E02BEBC9BA` | `CD23A55C008EC01D7D43510DDD0F9EB20F917A82743AE32AA89CE54C3E999605`; EMPTY |
| G6 | `crates/verter_session/src/project_semantic_dispatch/flow_return.rs` | Bypass degraded-success cache refusal with `if false && degraded`. | `flow_gap_known_gap_results_are_typed_partial_and_never_warm` — **RED** | `97454DBBA5DFD4E9ED6105141892B324B8A00AF8A46ABAC081F530BDF119DE78` | `79B922F035E4ADC99813C5CAC174C3F9253CD3FCA5BA110689E87D7146C05563`; EMPTY |

## Restored-state control

After all fifteen recipes restored, one selector ran the eight named test groups together. Result: **8 passed, 0 failed, 8,714 skipped**. This is the shared unplanted green control; every individual planted run above was red and every restored target had byte-identical SHA-256 plus empty per-file porcelain status.

## Review-fix mutation rerun (candidate `52fc7b3f5` + uncommitted FIX tree)

The blocking-review fixes were mutation-proved in the implementation worktree and restored with `apply_patch`; no commit was created.

| ID | Reversible production mutation | Named test / planted result | Restored result |
|---|---|---|---|
| RF1 | Disable the refined impossible-consequent statement transfer with `false && !consequent_possible`. | `flow_gap_known_gap_results_are_typed_partial_and_never_warm` — **RED**: G1 became complete/warm, returned only `"live"`, and admitted one candidate. | **GREEN**; together with `flow_gap_false_refusal_controls_remain_complete_and_warm` and `corpus_expect_and_boundary_lane`: 3 passed, 0 failed. |
| RF2 | Remove `active_guard_bindings.contains(binding) ||` from `optional_chain_root_has_prior_flow_change`. | `optional_chain_after_active_type_predicate_lowers_to_gap` — **RED**: the guarded user-predicate chain lowered as `OptionalAnyChain`. | **GREEN**; direct lowerer discriminator plus public refusal control: 2 passed, 0 failed. |
| RF3 | Remove the call-argument and computed-key triviality checks from `pure_optional_chain_root_identifier`. | `optional_chain_with_effectful_call_argument_lowers_to_gap` and `optional_chain_with_effectful_computed_key_lowers_to_gap` — **RED**: 0 passed, 2 failed. | **GREEN**; both direct discriminators plus the public refusal cohort: 3 passed, 0 failed. |

RF1 restored values: G1 is `FlowGap::GuardNarrowing`, cold on both calls with zero candidates; the exact-subject-read statement control is complete/warm with `"live"`; N25 remains `FlowGap::GuardNarrowing`, cold with zero candidates.

## Adversarial review fix rerun (candidate `29a8c8879` + uncommitted FIX tree)

Each production mutation below was planted with `apply_patch`, its named nextest selector was required to fail, and the inverse patch restored the implementation before the green control ran. The implementation worktree remained uncommitted.

| ID | Reversible production mutation | Named test / planted result | Restored result |
|---|---|---|---|
| RF4 | Reintroduce a spelling allowlist ahead of `optional_chain_discarded_expr_has_no_syntactic_effect`. | `optional_any_admits_the_complete_pure_member_and_terminal_call_class` - **RED**: 0 passed, 1 failed; `a?.[-1]` degraded as `UnmodeledExpression`, stayed cold, and admitted zero candidates. | **GREEN** with `optional_chain_with_nested_syntactic_effect_lowers_to_gap`: 2 passed, 0 failed. |
| RF5 | **SUPERSEDED by the A3 scope retraction.** The ancestor-suffix propagation and its `impossible_assertion_in_nested_block_degrades_an_enclosing_return_suffix` pin were removed; this recipe no longer describes live behavior. | Not rerun: the planted mechanism and named test no longer exist. | `conditional_impossible_assertion_preserves_enclosing_return_suffix` is **GREEN** and pins the ruled conditional-subregion repro as clean/warm `Literal(String("live"))` with one candidate. |
| RF6 | Rename `flow_gap_retraction_tests.rs` and its module wiring back to the candidate's `u6_` spelling. | `flow_gap_retraction_test_module_uses_final_state_filename` - **RED**: 0 passed, 1 failed. | **GREEN**: 1 passed, 0 failed. |

RF4's opposite-polarity control recursively plants assignment, update, `delete`, `await`, and `yield` below discarded operands and requires every form to lower to `Gap(UnmodeledExpression)`. RF5 is superseded by the ruled scope retraction; no ancestor-suffix mutation recipe remains live. The optional-any reaching-value test now pins both user-defined-predicate and `typeof` narrowing spellings.

## A3 scope-retraction follow-up mutation recipes

Each depth recipe changed only the named fixture, ran its single named test to a required RED, restored the exact authored effect, and then reused the unplanted six-test control (`6 passed, 0 failed`, including all four depth refusals and both enclosing-suffix controls).

| ID | Reversible fixture mutation | Named test / planted result | Restored result |
|---|---|---|---|
| RD1 | Change ``a?.b(`${x = 1}`)`` to ``a?.b(`${x}`)`` so the template interpolation no longer contains a nested assignment. | `optional_any_refuses_effect_in_nested_template_interpolation` — **RED**: 0 passed, 1 failed; the result became clean/warm `Primitive(Any)` with one candidate. | **GREEN** in the six-test restored control. |
| RD2 | Change `a?.b({ k: x++ })` to `a?.b({ k: x })` so the object property no longer contains a nested update. | `optional_any_refuses_effect_in_nested_object_property` — **RED**: 0 passed, 1 failed; the result became clean/warm `Primitive(Any)` with one candidate. | **GREEN** in the six-test restored control. |
| RD3 | Change `a?.b([await p])` to `a?.b([p])` so the array element no longer contains a nested await. | `optional_any_refuses_effect_in_nested_array_element` — **RED**: 0 passed, 1 failed; the result became clean/warm `Primitive(Any)` with one candidate. | **GREEN** in the six-test restored control. |
| RD4 | Change `a?.b(g(x = 1))` to `a?.b(g(x))` so the nested call argument no longer contains an assignment. | `optional_any_refuses_effect_in_nested_call_argument` — **RED**: 0 passed, 1 failed; the result became clean/warm `Primitive(Any)` with one candidate. | **GREEN** in the six-test restored control. |
