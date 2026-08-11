## Verdict

**IMPLEMENTABLE AS SPECIFIED; no Revision 11 contradiction or physical impossibility was found. No architecture deviation is required.**

Verified basis:

- Current checkout is `main` at `7f224ef7f1a0d25f1a7f563022ceb35b810a6fcb`, tree `10f3e860fa24b034a4a2a2e0350a99841e06bc58`.
- Accepted A2 implementation commit is its parent, `80a7d9c328842f1457e866fb8588687e9f1d3118`.
- The extra HEAD commit records the R-3 precedence ruling; it does not alter the A2 production implementation.
- The three existing admission gates are sufficient. A3 must feed them typed degradation; it must not create another cache policy or final flow owner.

The strongest counter-argument is that adding `Partial`/`FlowGap` now could prematurely install D1/D2’s final obligation-ledger typestate. That would be wrong. A3 must add only the narrow `FlowGap` reason carrier described below and map `Partial` onto the existing degraded-result rail. It must not add `FlowSolveOutcome`, `CompleteFlowResult`, `PartialFlowResult`, `FlowCoverageProof`, an obligation ledger, or a second solver.

## Mechanical implementation specification

### 1. Realize the ratified vocabulary

1. In [semantic_query.rs](<REPO>/crates/verter_session/src/semantic_query.rs:1547), immediately before `FlowReturnDegradation`, add:

```rust
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FlowGap {
    GuardNarrowing,
    NominalRelation,
    ClosureCapture,
    AbruptCompletion,
    UnmodeledExpression,
}
```

2. Add this variant to `FlowReturnDegradation`:

```rust
FlowGap(FlowGap),
```

3. Rewrite the documentation at lines 1547–1552. A degraded result no longer necessarily means “substituted `any`”; it means a usable but incomplete result. Preserve “first-observed reason wins” and explicitly state that every such result is return-only and warm-inadmissible.

4. Do **not** add a concrete `Partial` type. For A3, the plan term maps exactly as follows:

   - `Partial` = `FlowReturnStep::Complete(FlowReturnResult)` where `FlowReturnResult::degradation() == Some(...)`.
   - `FlowGap` = the new typed reason carried as `FlowReturnDegradation::FlowGap(FlowGap)`.
   - `NoValue` = existing `FlowReturnStep::NoValue(FlowReturnFailure)`.

   The legacy name `FlowReturnStep::Complete` is misleading for degraded payloads, but renaming or replacing that outcome type belongs to D1/D2’s final typestate cutover.

5. Do not add a stored `site` field to `FlowGap`. The older target document’s illustrative `FlowGap { site, reason }` depends on the final obligation identity/ledger, which A3 is forbidden to select. In A3, the construction site is the typed producer location; D1 later supplies obligation identity.

6. Update the A2 test mirror in [u6_flow_shape_corpus_tests.rs](<REPO>/crates/verter_session/src/u6_flow_shape_corpus_tests.rs:246):

```rust
FlowGap(FlowGap),
```

and add the exhaustive mapping at line 842:

```rust
Some(FlowReturnDegradation::FlowGap(gap)) => Degr::FlowGap(gap),
```

7. The new variant must reach the existing gates without gate changes:

   - Root gate: [flow_return.rs](<REPO>/crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1096), where `degradation().is_some()` sets `cache_suppress`.
   - SCC batch gate: [scc_publish.rs](<REPO>/crates/verter_session/src/semantic_query_memo/scc_publish.rs:229), where any degraded result makes the batch inadmissible.
   - Consumer-fold gate: [flow_return.rs](<REPO>/crates/verter_session/src/project_semantic_dispatch/flow_return.rs:653), where a degraded dependency keeps the enclosing family off the clean rail.
   - Retain the warm-read assertion at [flow_return_memo.rs](<REPO>/crates/verter_session/src/semantic_query_memo/flow_return_memo.rs:114).

No fourth gate, special-case key, or A3-specific cache flag is permitted.

### 2. Add IR carriers that cannot fabricate `any`

8. In [flow_slice_content.rs](<REPO>/crates/verter_session/src/flow_slice_content.rs), change:

```rust
SliceExpr::Any
```

to:

```rust
SliceExpr::SemanticAny
SliceExpr::Gap(FlowGap)
```

`SemanticAny` is only for a complete shallow inference that semantically produced TypeScript `any`. `Gap` is for “Verter has no model.”

9. In `FlowEvaluator::eval_expr`, at [flow_return.rs](<REPO>/crates/verter_session/src/project_semantic_dispatch/flow_return.rs:7307):

```rust
SliceExpr::SemanticAny => Positional::Value(
    graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Any)),
),
SliceExpr::Gap(gap) => {
    self.record_degradation(FlowReturnDegradation::FlowGap(*gap));
    Positional::Unmodeled
}
```

The existing positional marker composes into enclosing values. Because the precise `FlowGap` degradation is recorded first, later generic `UnmodeledPosition` recording must not replace it.

10. Add a value-neutral statement marker:

```rust
SliceStatement::Gap(FlowGap)
```

At the start of its evaluator arm near [flow_return.rs](<REPO>/crates/verter_session/src/project_semantic_dispatch/flow_return.rs:5585):

```rust
SliceStatement::Gap(gap) => {
    self.record_degradation(FlowReturnDegradation::FlowGap(*gap));
}
```

It records incompleteness and continues evaluating the current approximation. It must not change `path_alive`, contributors, locals, narrowing state, or completion.

This preserves deterministic source-order “first reason wins” instead of seeding a whole-slice degradation before evaluation.

### 3. Separate shallow inference completeness from semantic `any`

11. At [type_eval_build.rs](<REPO>/crates/verter_semantic/src/analysis/type_eval_build.rs:3954), add a flow-facing result:

```rust
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpressionInferenceCompleteness {
    Complete,
    Unmodeled,
}

pub struct DeclarationExpressionInference {
    pub ty: TypeExpr,
    pub completeness: ExpressionInferenceCompleteness,
}
```

12. Add:

```rust
pub fn infer_declaration_expression_type_with_completeness(
    ...
) -> Result<DeclarationExpressionInference, InferenceUnavailableReason>
```

The inference builder receives a boolean `used_unmodeled_fallback`, initially false.

13. At the wildcard arm currently returning `Any` at [type_eval_build.rs](<REPO>/crates/verter_semantic/src/analysis/type_eval_build.rs:4359), retain the current `TypeExpr::Primitive(Any)` only as the internal value, but set `used_unmodeled_fallback = true`.

14. The existing `infer_declaration_expression_type` remains as a wrapper returning only `.ty`. This prevents A3 from changing non-flow behavior before A6.

15. In flow’s `leaf_type`/`lower_leaf` boundary at [flow_slice_content.rs](<REPO>/crates/verter_session/src/flow_slice_content.rs:4530), call the completeness-returning function:

   - `Complete` plus `is_any(&ty)` → `SliceExpr::SemanticAny`.
   - `Unmodeled` → `SliceExpr::Gap(FlowGap::UnmodeledExpression)`.
   - Complete non-`any` → existing `Type`/`FrameShadowedType` paths.
   - Preserve the earlier unreduced-call classifier and `UnreducedCallValue`.

This distinguishes authored or legitimately inferred `any` from wildcard fallback without adding `This`, `Capture`, or sequence semantics.

### 4. Remove the other fabricated-`any` paths

16. At [flow_slice_content.rs](<REPO>/crates/verter_session/src/flow_slice_content.rs:4493), replace the impossible expression-body fallback:

```rust
SliceExpr::Any
```

with:

```rust
SliceExpr::Gap(FlowGap::UnmodeledExpression)
```

17. At [flow_return.rs](<REPO>/crates/verter_session/src/project_semantic_dispatch/flow_return.rs:7250), change an absent local read to:

```rust
match self.read_local(name.as_ref()) {
    Some(node) => Positional::Value(node),
    None if *captured => Positional::Unmodeled,
    None => match param.and_then(|ordinal| self.params.get(ordinal as usize).copied()) {
        Some(node) => Positional::Value(node),
        None => {
            self.record_degradation(FlowReturnDegradation::FlowGap(
                FlowGap::UnmodeledExpression,
            ));
            Positional::Unmodeled
        }
    },
}
```

Delete the `PrimitiveKind::Any` fallback at lines 7260–7262.

18. At [flow_slice_content.rs](<REPO>/crates/verter_session/src/flow_slice_content.rs:1812), make `lower_params` return:

```rust
Result<Vec<SliceParam>, InferenceUnavailableReason>
```

At lines 1880–1889, replace `unwrap_or(Any)` with `?`.

19. Root parameter failure:

   - In `build_flow_slice_content` around line 1484, on `Err(reason)`, return a `SliceContent` with `budget_failure: Some(reason)`, empty parameters/body, and no semantic placeholder.
   - Evaluation already converts this at [flow_return.rs](<REPO>/crates/verter_session/src/project_semantic_dispatch/flow_return.rs:2112) to `FlowReturnFailure::Budget`.
   - The empty content is never semantically evaluated because the budget check precedes evaluator construction.

20. Nested parameter failure at line 4418:

   - Set `self.budget_failure` to the first reason.
   - Use an empty nested parameter list solely to finish constructing the cold artifact.
   - The outer evaluation must terminate at the existing budget check; do not manufacture an `any`, `unknown`, or partial signature.

21. The following remain semantic `any`:

   - Line 1895: unannotated ordinary parameter.
   - Line 1929: unannotated rest parameter.
   - A complete inference result whose actual TS type is `any`.
   - The `SemanticAny` evaluator arm.
   - [flow_return_callee.rs](<REPO>/crates/verter_session/src/project_semantic_dispatch/flow_return_callee.rs:592) `CallValue::modeled_any`.
   - The undegraded callable-`any` arm at [flow_return.rs](<REPO>/crates/verter_session/src/project_semantic_dispatch/flow_return.rs:8023).

22. The budget sentinel at `flow_slice_content.rs:4690` is not a publication defect: line 2114 preempts it with `FlowReturnFailure::Budget`. Do not change it in A3 unless it becomes reachable after step 18; add a reachability assertion rather than widening scope.

### 5. Per-gap implementation

| Gap | A3 detector and exact outcome | Semantic owner code changed? |
|---|---|---|
| G1 | At `flow_return.rs:5688`, `5729`, `5768`, `5808`, `5810`, `5884`, and `6580`, when final `GuardNarrowing::Impossible` suppresses a return-bearing region, record `FlowGap::GuardNarrowing`; return the current approximate value as `Partial`. | **No.** Do not alter `GuardNarrowing`, its boolean result, arm contribution, or narrowing algebra. |
| G2 | At ternary evaluation `flow_return.rs:7338`, when an impossible guard suppresses an arm not proven to be exactly the narrowed subject read, record `FlowGap::GuardNarrowing`. For nested statement predicates over an already-narrowed subject, record the same gap before evaluating the nested `SliceStatement::If`. | **No.** No structural-return repair or `FlowFrame`. |
| G3 | Refactor `nodes_provably_disjoint` at `flow_return.rs:4710` to return `{ provably_disjoint: bool, nominal_identity_missing: bool }`. When either compared leaf is `Primitive(Symbol)` and nominal identity is unavailable, set the second bit. At use `:5402`, preserve the old boolean decision but record `FlowGap::NominalRelation` when the bit is set. | **No.** Do not add unique-symbol identity, shared relation calls, or tri-state comparability. |
| G4 | Uniform direct-IIFE preflight finds the capture-write in declarator initializers. Emit `SliceStatement::Unsupported(InvokedClosureEffect)`, yielding `NoValue(Unsupported(InvokedClosureEffect))`. | **No.** No effect transfer. |
| G5 | Same preflight for `if` tests. Same `NoValue`. | **No.** |
| G6 | At nested-function lowering around `flow_slice_content.rs:4406`, if an outer lexical `let` captured for reading has an outer write after the function-value creation span, attach `FlowGap::ClosureCapture` to that nested value. Current approximate signature is returned cold. | **No.** No temporal capture state or join. |
| G7 | If a sibling or deeper nested skeleton contains a free write resolving to the captured outer `let`, attach `FlowGap::ClosureCapture`. | **No.** Leave `push_nested_capture_reads` untouched. |
| G8 | If an unannotated mutable outer `let` is both freely read and freely written by the same nested function, attach `FlowGap::ClosureCapture`. Annotated same-closure writes remain on the current supported rail. | **No.** No capture summary. |
| G9 | Maintain an active-guard binding stack only while lowering an `if` arm. If a nested function freely reads a binding present in that active guard stack, attach `FlowGap::ClosureCapture`. | **No.** Do not transfer the narrowing into the closure. |
| G10 | Before dropping a suffix at `flow_slice_content.rs:3045`, detect a remaining current-function return under a labeled/try/catch/finally/switch container that `statement_guarantees_current_function_return` does not recognize. Insert `SliceStatement::Gap(FlowGap::AbruptCompletion)` immediately before the controlling statement. Preserve the current `"a" \| undefined` approximation, cold. | **No.** Do not alter the completion boolean or implement `CompletionKind`. |
| G11 `this` | The completeness flag from `type_eval_build.rs:4359` makes the current wildcard `Any` become `SliceExpr::Gap(UnmodeledExpression)`, producing a partial opaque position, never `() => any`. | **No.** No `This`/`Capture` carrier. |
| G11 sequence | The same completeness flag catches the unmodelled sequence operand. Return the composable positional marker with `FlowGap::UnmodeledExpression`. | **No.** No sequence pass-through disposition. |

Thus A3 leaves the semantics of **all G1–G11 gaps** to their assigned owners. It touches only detection, typed retraction, and admission eligibility.

### 6. Guard detector constraints

23. Do not record a gap for every intermediate `GuardNarrowing::Impossible` inside `apply_guard_union` at lines 4838–4853. That would falsely retract N23.

24. Record at a consumer only when the collapsed result controls one of:

   - whether a return-bearing statement region is evaluated;
   - whether a ternary value arm contributes;
   - whether assertion handling kills the remainder;
   - whether a nested predicate statement operates over an already-narrowed subject.

25. Add `slice_expr_is_exact_subject_read(expr, subject)`. It returns true only for the exact `Local`/`Param` root and exact property path encoded by `SliceNarrowSubject`; it must return false for calls, objects, literals, aliases, composites, and unknown paths.

This permits N24’s `never`-neutral `x` branch to remain complete while G2’s unrelated dead value becomes partial.

### 7. Position-independent IIFE refusal

26. Remove the expression-statement-only hook at [flow_slice_content.rs](<REPO>/crates/verter_session/src/flow_slice_content.rs:3828).

27. Do not call the recursive IIFE scanner independently from every expression position. That would multiply AST walks and regress cold-path complexity.

28. Add to `Lowerer`:

```rust
unsafe_invoked_closure_effects: FxHashSet<FrameSpan>,
```

29. Immediately after constructing the root lowerer at line 1510, perform one body walk over `body.statements`. For every call:

   - inspect only a directly invoked function/arrow in callee position;
   - exclude closures merely passed as arguments;
   - reuse `nested_function_transfers_downstream_slot`;
   - exclude literal-dead branches using the existing filter;
   - insert the direct call span when its capture write/control read can affect a later selected slot.

30. Perform the equivalent one-time indexing for each nested lowerer after line 4458.

31. At the start of each `lower_region` statement iteration at line 3045, if its span contains an indexed unsafe call, emit:

```rust
SliceStatement::Unsupported(SliceUnsupported::InvokedClosureEffect)
```

set `hit_unsupported = true`, set `can_fall_through = false`, and stop that region.

32. For root or nested expression-bodied arrows, run the same indexed-span check before `lower_expr` at lines 1537 and 4489.

This yields the same `NoValue` for statement, declarator initializer, `if` test, sequence operand, call argument, template, short-circuit, and object-literal positions without implementing a closure summary.

### 8. Capture detector constraints

33. Build a transient `nested_free_writes: FxHashSet<SkeletonBindingId>` once per lowered function. Populate it by recursively building existing nested skeletons and resolving each nested free `SkeletonWriteTarget::Named` back through the enclosing skeleton at the nested function’s creation region.

34. Do not modify [analysis/flow/mod.rs](<REPO>/crates/verter_semantic/src/analysis/flow/mod.rs:1228), do not retain `nested.writes` in the graph, and do not add a persisted capture summary. Those are `U6.LOOP_CLOSURE` work.

35. Add an optional `gap: Option<FlowGap>` field to `SliceExpr::NestedFunctionValue`. At evaluation of that expression, record the gap before evaluating or materializing the nested signature.

36. Apply exactly these exclusions:

   - `var` and parameters with authored declared authority are not G6 false positives.
   - `const` is not mutable.
   - A `let` with no relevant write and no active guard is not a gap.
   - A same-closure write to an annotated `let` is not G8.
   - A callback value not invoked is not G4/G5, although its free write may still be G7 for another captured reader.

### 9. A2 pin updates

37. In [u6_flow_shape_corpus_rows_tests.rs](<REPO>/crates/verter_session/src/u6_flow_shape_corpus_rows_tests.rs:641), re-pin N25:

   - Keep the existing recursive wrong-value expectation and exact JSON.
   - Change `degradation` to `Degr::FlowGap(FlowGap::GuardNarrowing)`.
   - Change `candidates: 1` to `candidates: 0`.
   - Change `warm_replay: true` to `warm_replay: false`.
   - Keep `Verdict::KnownOwed`.
   - Rewrite its note to say the current approximation remains wrong, but is now honestly partial and recomputes cold.

38. In [u6_flow_expect_tests.rs](<REPO>/crates/verter_session/src/u6_flow_expect_tests.rs:1829), replace `IIFE_POSITION_CELL`’s position-dependent outcome with:

```rust
CellExpectation::Uniform(CellOutcome::NoValue {
    error: IIFE_EFFECT_REFUSAL,
})
```

All eight covered positions become exact typed refusal, cold on both calls.

39. Replace `position_dependent_pins_record_a_real_divergence` at line 2423 with `uniform_iife_effect_refusal_covers_every_position`. Assert:

   - the cell is `Uniform`;
   - the position set equals `COVERED_POSITIONS`;
   - every position returns exact `IIFE_EFFECT_REFUSAL`;
   - both calls report `from_cache == false`;
   - both calls have `cold_computes >= 1`.

40. Re-pin these `FIXED_CELLS` from clean/warm to partial/cold while retaining their current rendered approximation:

   - `let_write_after_creation` → `FlowGap::ClosureCapture`, `warm_replay: false`.
   - `let_sibling_closure_write` → same.
   - `let_deeper_closure_write` → same.
   - `typeof_guard_before_creation` → same.

41. Add an explicit G8 cell:

```ts
function makeProps() {
  let x = "a";
  return () => { x = "b"; return x }
}
```

Pin the current approximate signature, `FlowGap::ClosureCapture`, candidate count zero, and cold replay.

42. Leave these existing pins unchanged:

   - `let_write_before_creation`;
   - `var_write_after_creation`;
   - `param_write_after_creation`;
   - `const_capture_never_written`;
   - the already-honest IIFE try/finally, label, `if`, and switch refusals.

### 10. Required public tests

43. Add [u6_flow_a3_retraction_tests.rs](<REPO>/crates/verter_session/src/u6_flow_a3_retraction_tests.rs) and register it under `#[cfg(test)]` in [lib.rs](<REPO>/crates/verter_session/src/lib.rs:855).

44. Add `a3_known_gap_results_are_typed_partial_and_never_warm`. Its table must contain separate fixtures for:

   - G1 impossible `typeof` return.
   - G2 nested predicate ternary with an unrelated dead value.
   - G3 distinct unique-symbol discriminants.
   - G6 write after creation.
   - G7 sibling and deeper writes.
   - G8 unannotated mutable same-closure write.
   - G9 guard before creation.
   - G10:

```ts
function makeProps() {
  try { throw 0 }
  catch { LABEL: { return "a" as const } }
  return "b" as const
}
```

   - G11 sequence:

```ts
function makeProps() { return (0, () => "a" as const) }
```

   - G11 typed `this`:

```ts
function makeProps(this: { value: "a" }) {
  return () => this.value
}
```

For each fixture, perform two public `get_flow_return_type_with_audit(...whole_return())` calls and assert:

   - both are usable values;
   - exact `FlowReturnDegradation::FlowGap(expected)`;
   - both `from_cache == false`;
   - both `cold_computes >= 1`;
   - memo candidate count is zero;
   - no returned node introduced `Primitive(Any)` unless the source authored `any`.

45. Add `a3_invoked_closure_effect_is_position_independent_no_value`. Drive all eight A2 positions twice and assert exact:

```rust
FlowReturnError::Failure(
    FlowReturnFailure::Unsupported(
        FlowReturnUnsupported::InvokedClosureEffect,
    ),
)
```

with cold computation on both calls and zero candidates.

46. Add `a3_authored_any_remains_complete_and_warm`, covering:

```ts
function explicit(x: any) { return x }
function implicit(x) { return x }
function rest(...x) { return x }
function callAny(callable: any) { return callable() }
function asserted(x: unknown) { return x as any }
```

Each must have `degradation == None`; its second call must be `from_cache == true` with `cold_computes == 0`.

47. Add `a3_default_parameter_budget_failure_is_no_value_and_cold`. Generate 65 nested array/object expressions in an unannotated default parameter, exceeding `MAX_SEMANTIC_INFERENCE_DEPTH == 64`. Assert exact `FlowReturnFailure::Budget(DepthBudgetExceeded)` on both calls, no warm hit, positive cold work, and zero candidates.

48. Add `a3_partial_propagates_through_consumer_and_scc_gates`:

   - Direct G11 fixture exercises the root gate.
   - A clean outer function calling a G11-partial function must itself be degraded and have no candidate, exercising consumer fold.
   - A recursive two-function SCC with one G11-partial contributor must publish no candidate for either member, exercising batch admission.

49. Add `a3_false_refusal_controls_remain_complete_and_warm` with exact recursive value pins and two-call warm assertions for:

   - N23 impossible conjunction.
   - X70 callback argument.
   - X87 read-only mutable capture.
   - X68 abrupt-completion approximation.
   - X80 wrapped labeled try/finally.
   - Also include N24, N26, X85, and X88 because they are adjacent discriminators.

### 11. Verification commands

Run in this order:

```powershell
cargo fmt --all --check
cargo test -p verter_session u6_flow_a3_retraction_tests -- --nocapture
cargo test -p verter_session u6_flow_shape_corpus_tests::corpus_suite::corpus_expect_and_boundary_lane -- --exact --nocapture
cargo test -p verter_session u6_flow_shape_corpus_tests::u6_flow_expect_tests::matrix_suite::matrix_cells_hold_their_pins -- --exact --nocapture
cargo nextest run --workspace
cargo test -p verter_session --tests
node scripts/gate.mjs
```

The evidence packet must record the exact candidate SHA/tree and, for every new public case:

- first and second result class;
- exact degradation/failure;
- projected JSON or recursive graph expectation;
- first and second `from_cache`;
- first and second `cold_computes`;
- candidate count after each call.

Required mutation evidence:

1. Change the G11 wildcard status back to `Complete`: G11 must publish `any`, and its test must fail.
2. Remove the root `degradation().is_some()` suppression: the second partial call must warm and fail.
3. Empty the unsafe-IIFE index: the five formerly stale positions must fail.
4. Treat callback arguments as direct callees: X70 must fail.
5. Mark every capture read as a gap: X87 must fail.
6. Mark every `try`/label as completion-unknown: X68/X80 must fail.
7. Record every intermediate guard `Impossible`: N23 must fail.
8. Restore `unwrap_or(Any)` for default parameters: the budget test must fail.

### 12. Risks and anti-overreach findings

1. **Final typestate ownership:** Adding `FlowSolveOutcome`, proof tokens, or an obligation ledger in A3 is prohibited scope expansion.

2. **Narrowing ownership:** A3 may observe `Impossible`; it may not repair structural return collection or create `FlowFrame`.

3. **Relation ownership:** A3 may flag missing nominal identity; it may not add unique-symbol identity, delete the private classifier, or define tri-state relation semantics.

4. **Closure ownership:** The transient free-write scan exists only to refuse. Persisting it, transferring effects, or changing capture values is `U6.LOOP_CLOSURE`.

5. **Completion ownership:** The detector may identify reliance on the incomplete boolean. It must not introduce completion algebra.

6. **G11 ownership:** Completeness provenance is allowed; `This`, `Capture`, and sequence pass-through carriers are not.

7. **Authored-any regression:** Converting every inferred `Any` to a gap would be wrong. The completeness bit, not the resulting `TypeExpr`, is authoritative.

8. **Warm-path performance:** `FlowGap` is allocation-free and does not enter memo keys. IIFE and nested-write indexes are built once per cold lowered function, never once per expression position. Clean warm reads remain unchanged.

9. **N23:** Must remain clean/warm because intermediate impossible disjuncts do not themselves control an omitted return-bearing region.

10. **X70:** Must remain clean/warm because only closures in direct callee position count as invoked.

11. **X87:** Must remain clean/warm because a mutable declaration alone is insufficient; a relevant write or active guard is required.

12. **X68/X80:** Must remain clean/warm because the completion detector is triggered by an unclassified return-bearing suffix, not by the mere presence of `try`, `finally`, labels, or abrupt completion.

13. **N24:** Must remain clean/warm through the exact-subject-read exemption; its skipped `x` is `never`-neutral.

14. **X85:** Must remain clean/warm because an annotated capture written and read within the same closure is already correctly modeled.

15. **N26:** Must remain clean/warm because it contains no missing unique-symbol identity and structural overlap remains possible.

16. **X88:** Must remain clean/warm because its direct suffix `return` is already recognized; the detector must not fire merely because labels are present.

With these constraints, A3 retracts every verified wrong-and-warm answer without repairing any assigned semantics, selecting the final solver owner, conflating authored `any`, or creating a parallel admission authority.

__DONE__

---

## Specification correction (architecture authority)

**A3 specification correction:** §10 item 46's top-level `declare const callable: any`
fixture exercised the pre-existing `SliceCall::Symbolic` refusal rather than the cited
callable-`any` `SliceCall::OnBinding` rail. Replace it with
`function callAny(callable: any) { return callable() }`. This changes evidence targeting
only; A3 scope and production symbolic-call semantics are unchanged.

The corrected §10 authored-`any` fixture is:

```ts
function explicit(x: any) { return x }
function implicit(x) { return x }
function rest(...x) { return x }
function callAny(callable: any) { return callable() }
function asserted(x: unknown) { return x as any }
```

For `callAny`, require all of:
1. Both calls return usable values.
2. Both project exactly to `Primitive(Any)`.
3. Both have `degradation == None`.
4. First call: `from_cache == false`, `cold_computes >= 1`, candidate count exactly `1`.
5. Second call: `from_cache == true`, `cold_computes == 0`, candidate count exactly `1`.
6. First and second projected results are identical.

Add an exact `projected == Some("Primitive(Any)")` assertion for `callAny` so a
clean-and-warm WRONG type cannot satisfy the test.

Do NOT change the `SliceCall::Symbolic` arm in `flow_return.rs` for A3. The measured
`UnrepresentableCallee`, cold, zero-candidate result is a pre-A3 fail-closed limitation,
not a wrong-and-warm result subject to A3 retraction. The underlying coverage deficiency —
TypeScript defines a call on a known `any` value as producing `any` — is a genuine
end-state gap owned by D7 (call/context/value surfaces), to be inventoried and classified
by A5/A6 first. If A6 ratifies it Supported/Stable, D1/D2 implement it before the atomic
cutover. It is not owned by A3.

"Authored `any` remains distinct" is a PRESERVATION boundary for A3: prove representable
semantic `any` is not reclassified as `FlowGap` or made cold. It is not a mandate to
expand a pre-existing unsupported call rail.
