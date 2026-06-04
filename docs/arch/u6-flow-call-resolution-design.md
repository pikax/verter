# U6 — Flow narrowing + Call resolution + Contextual typing (LOCKED design)

> Block: `U6` (Native Flow Return) — the RESCOPE-GATE-REQUIRED design
> (`docs/arch/semantic-db-overhaul-unified-remaining-plan.md` §2290 "U6 — Native Flow Return (B11)"). This is a
> **PLAN block / DESIGN GATE**: it LOCKS the airtight mechanisms for the U6 flow/call/narrowing/contextual
> engines and the implementation mini-DAG; it does **not** build the engines. Implementation is sequenced AFTER
> the U2 value-domain spine + the U2.RELATION_INFER substrate via the U6 sub-blocks
> (`docs/arch/native-flow-return.md`).
>
> **Parent / upstream designs this builds OVER and NEVER contradicts:**
> - `docs/arch/u2-relation-infer-design.md` — LOCKED. Assignability/`Relate`, the coinductive-SCC admission,
>   the `CheckerTransaction` / `InferenceSession` / `CheckerReentryStack` substrate, the per-session
>   `SessionAdmissionLedger`, the session-close RE-DISCHARGE, the `ReturnOnly` discipline, admission-table rows.
>   U6 CONSUMES this. The `CheckerReentryStack` is BUILT at U2/RI-3 (sole builder), only `Relate` WIRED at U2;
>   U6 wires the `FlowReturn` / `ResolveCall` / `ContextualTypeAt` / `FlowNarrowingAt` typed views onto it.
> - `docs/arch/u2-query-value-domain-design.md` §2.1, §2.2, §2.5, §14 — LOCKED SHAPES. `FlowNarrowingAt` /
>   `ContextualTypeAt` → `ProgramAnalysis` value domain (`ProgramAnalysisGraph` wire, never `GraphTypeNode`);
>   the `Relate` row; `ProgramAnalysisContext` {P,R}+flow+contextual+subst; R21 five split env hashes; R6
>   content-free keys. This design does NOT redesign these shapes.
> - `docs/arch/native-flow-return.md` — the parent U6 subplan (the block contracts, the `FlowReturn` key shape,
>   the `FlowSlice` fact + `validates_program_analysis_domain`, the demand-sliced `ReturnPathPeeker`). This
>   design LOCKS the cross-cutting mechanisms the subplan's block contracts carry and CORRECTS the two
>   load-bearing items the subplan's prose left ambiguous (D2 the narrowing-invalidation rail; the
>   CONTEXTUAL_CALLBACK fixation-time mechanism ownership).
> - `docs/arch/native-typeinfo-parity-u2-reducers.md` — the U0 block-metadata DAG guard
>   `typeinfo_parity_block_dag_is_acyclic_and_consumed_keys_and_mechanisms_are_prereqs` (checks 1–5) + the fi08
>   deadlock note.
>
> **Status: DESIGN-LOCKED.** Produced by a two-panelist design panel (codex gpt-5.5/xhigh + a claude reviewer,
> same max-mandate question) and adjudicated by an independent fresh adjudicator who INDEPENDENTLY verified the
> two load-bearing claims (the D2 narrowing-invalidation rail; the fi08 ownership/acyclicity) rather than
> concurring. Panel artifacts: `/tmp/mom/U6/PANEL.md`, `panel-codex2.txt`, `panel-claude.md`, `ADJUDICATION.md`.
> The adjudicator returned a SEND-BACK against the *parent subplan's prose* (it credits the wrong D2
> invalidation rail and leaves the CONTEXTUAL_CALLBACK fixation mechanism unowned); this doc is the re-issue
> that folds in all seven adjudicated deltas. None of the corrections weakens a locked upstream invariant — all
> are tightenings the locked relation-infer / value-domain substrate already implies.

---

## 0. Scope and the one-sentence architecture

The U6 flow/call/narrowing/contextual engines are **four typed views of the ONE
`SemanticQueryKey → ProjectSemanticDispatch::execute → SemanticGraphStore` dispatch**, sharing the one
`CheckerReentryStack` (built at U2/RI-3) and the one `InferenceSession` substrate (U2.RELATION_INFER) — NEVER a
second flow walker, a second call resolver, a second inference matcher, or a query-time OXC re-walk. Call
resolution opens one **speculative `InferenceSession` per overload candidate** and admits only the **winning,
`CompletedDeterministic`** result; every cached flow/narrowing/contextual value is **version-rooted on the
value** (the `FlowSlice` fact's whole-function `flow_body_stable_hash` + the union of consumed
`ReadSetSignature.facts`), never on its query-identity key (R6); flow/narrowing/predicate/contextual facts live
in `FactDomain::ProgramAnalysis` and publish through `ProgramAnalysisGraph`, **never** as `GraphTypeNode` type
nodes; and the cross-engine recursion `ResolveCall → FlowReturn → narrowing → ResolveCall` discharges through a
transient re-entry assumption on the one shared stack — only a converged/stable/deterministic per-domain result
warm-admits, everything else routes through `ReturnOnly`.

This design is part of the ONE resolver. The flow/call/narrowing/contextual engines are nodes of the single
dispatch — they are not engines that "call into" the resolver, they ARE the resolver's cold-compute frame for
their query kinds.

### 0.1 What the current code does wrong (the thing being deleted)

Today the return-type "inference" is a lightweight OXC scanner
(`crates/verter_semantic/src/analysis/type_eval_build.rs` — `infer_return_type`:1119, `collect_return_types`:1138,
`infer_expression_type`:1167, `extract_object_literal_as_type`:1041, `append_spread_array_element_types`:1307)
that walks `Statement::ReturnStatement` directly off the parse tree (holding an arena borrow), only descends into
`BlockStatement`/`IfStatement`, treats every identifier as `TypeOf(path)` without resolving against the
parameter/local environment, and unions returns naively. It is **not** callable from `execute_cooperative`, never
participates in `ReadSetSignature.facts` validation, never emits an audit event, and never warms a reusable
cache. There is no demand-sliced flow graph, no narrowing engine, no first-class call-resolution key. U6 DELETES
the scanner and replaces it with the demand-sliced flow substrate (`docs/arch/native-flow-return.md`
U6.FLOW_RETURN_SUBSTRATE) routed through the one dispatch.

---

## Decision 1 — `ResolveCall` (U6.CALL_RESOLVE): overload order + speculative sessions + per-arg identity

### 1.1 The key and the first-class status

`ResolveCall` is a **first-class `SemanticQueryKey` landed at U6** (enum variant + `SemanticQueryKeySpec` row +
`execute_resolved_call` dispatch + `build_resolve_call` executor + cache-identity guards together, in the
U2-finalized slot-identity SHAPE with NO cache re-key). U2.QUERY_VALUE_DOMAIN finalized only the slot-identity
SHAPE/model it reuses — it did NOT pre-register the variant (the standing
`semantic_query_key_spec_table_equals_enum` meta-guard would reject any U2 tree referencing it).

```rust
SemanticQueryKey::ResolveCall {
    callee: SemanticNodeId,                  // resolved via the typed-IR resolver, NEVER a text parse
    call_kind: CallKind,                     // Call | New | TaggedTemplate | Decorator | JsxFactory
    receiver_this: Option<SemanticNodeId>,   // `this`-receiver method calls
    args: Arc<[CallArgKey]>,                 // per-arg identity (§1.3)
    explicit_type_args: Arc<[SemanticNodeId]>,
    contextual_result: Option<SemanticNodeId>, // the ENCLOSING contextual target / expected return type
    policy: OverloadPolicy,                  // first-applicable-for-calls / last-visible-for-ReturnType
    context: ResolveCallContext,             // R21 split env (R T L J, +P where the producer reads parse-env — Decorator/JsxFactory resolution + contextual-arg parse reads) + substitution + projection-reduction
}
```
Value domain: `SemanticQueryValue::ResolvedCall(Arc<ResolvedCallResult>)`. The key carries NO content/version/
`fact_dep` hash (R6); version rooting is on the value's `ReadSetSignature` (§6). `CallResolutionBudget` is keyed
on this full identity; `BudgetExceeded` ⇒ `ReturnOnly` (three-layer: no result, no overload-candidate /
inference-binding intermediate, no fact signature / backfill).

### 1.2 Overload resolution order (the locked algorithm)

Call resolution runs ON the U2.RELATION_INFER `InferenceSession` substrate (parent §4.2) — there is no
call-specific inference matcher. The `build_resolve_call` DRIVER:

1. Resolve `callee` to a `SymbolHandle` via the typed-IR resolver (no text). Resolve the ordered overload set
   via `ResolveOverloadSet` (U2.CLASS_SURFACES owns the ordered-signature SHAPE; the implementation signature is
   internal-only; `ReturnType<typeof overloaded>` / `ConstructorParameters` use the **LAST visible** overload —
   that SHAPE selection is U2.CLASS_SURFACES, consumed here).
2. For each candidate overload **in declaration order**, open ONE **speculative `InferenceSession`** on the
   active `CheckerTransaction`. Inside that session: applicability check + argument-to-parameter assignment via
   **binding-producing `Relate`** (mutating the session's `InferenceInfo`, collecting candidates per the
   relation-infer explicit candidate-combination rule) + fixation + final substitution. Per-arg contextual
   typing of un-annotated callback/object-literal args is the **session's fixation fixed-point** (Decision 4).
3. **First-applicable wins (for calls).** The DRIVER selects the FIRST candidate whose applicability+inference
   succeeds. Candidates AFTER the winner are **never opened** (no session). The winner KEEPS its session and
   publishes its completed `ResolvedCall`; the chosen signature's return is solved by a **recursive `FlowReturn`
   dispatch** under the normalized substitution env.

### 1.3 Per-arg `ContextSensitiveExprKey` — fields are IDENTITIES, never flags (ADJUDICATED FIX)

`CallArgKey::{ Eager, ContextSensitive }`:
- A **closed** argument (its type does not depend on the contextual parameter) normalizes to an `Eager` TYPE
  identity (`SemanticNodeId`-grade).
- A **context-sensitive** argument (un-annotated arrow / object literal whose type depends on the contextual
  param) keeps an EXPRESSION identity `ContextSensitiveExprKey { flow_narrowing, substitution, binder,
  contextual_typing }`, where:
  - **`contextual_typing` is the RESOLVED contextual-target TYPE IDENTITY** the candidate signature supplies to
    the arg (a `SemanticNodeId`-grade id — the candidate's parameter type incl. rest/destructuring target +
    relevant `this`/callback target after substitution), **NOT a boolean / "context-sensitive: yes/no" flag.**
    Two overload candidates with parameter types `P_A` vs `P_B` MUST NOT collide on the same syntactic arg; that
    is exactly why each candidate runs in its own speculative session and the same arg is re-typed per candidate.
  - **`substitution` is the substitution env in force AT TYPING TIME**, NOT the final env — the fixation loop
    re-types the arg as the env evolves; a final-env value would collapse two iterations onto one identity.
  - `flow_narrowing` is the caller-side narrowing context at the call site; `binder` is the lexical binder
    identity.
- The **enclosing contextual target is NOT a missing axis** — it enters via the parent key's `contextual_result`
  field. With `contextual_typing` and `substitution` as identities, the four-tuple is **minimal and sound** —
  no further axis is required.

### 1.4 Loser no-leak is a SPECIFIED MECHANISM, not an asserted outcome (ADJUDICATED FIX — the load-bearing composition with relation-infer)

"Loser sessions are discarded with no entry/fact/backfill" is the *goal*; the *mechanism* that makes it hold
against the relation-infer `SessionAdmissionLedger` deferral is:

> **`admit(candidate session ledger) ⇔ candidate == selected_winner ∧ session == CompletedDeterministic`.**

Overload selection is resolved at the `build_resolve_call` DRIVER level **before any candidate session's
`SessionAdmissionLedger` is drained**. Every non-winner session — one that failed applicability, or one
cancelled at the instant the winner was found — is transitioned to **`Abandoned(reason)`**
(`SupersededBySelection` for the cancelled-on-win case, the applicable failure reason otherwise), so every
**deferred binding-`Relate` member** inside it routes through relation-infer **admission-table row 8 /
release-without-publish ⇒ `ReturnOnly`** (no entry, no fact signature, no backfill, no reverse-index metadata).
**Only the winner's session is allowed to reach the admitting `CompletedDeterministic` drain.**

Why this is sound (the relation-infer interaction the brief flagged): a binding-`Relate` inside a candidate's
session has its in-flight reentry node keyed by the **transient per-session `SessionId`** (relation-infer §2.2,
line 366) — private to that candidate's `CheckerTransaction`, never a cache key. A loser's in-flight node can
therefore never be joined by another waiter or remapped onto a published §2.7 `Relate` key; abandoning the
session releases its ledger entries without any re-discharge. A losing candidate even reaching
`CompletedDeterministic` (its own fixation converged) is irrelevant — the DRIVER gate forbids draining a
non-winner ledger, and that converged-but-globally-rejected hypothesis is precisely what relation-infer
admission-table **row 6** (speculative/losing candidate ⇒ `ReturnOnly`) forbids warm-admitting. The winner's
published `ResolvedCall` references only **completed §2.7 `Relate` keys** (the session-converged re-discharge of
§2.3 step 4 / §3.3 re-keys the transient `SessionId` to the completed `InferenceContextKey` at session-close);
no transient `SessionId` leaks into the published `ResolvedCall`.

Named deferred guard (specified here, owned by U6.CALL_RESOLVE, lands WITH the variant):
`losing_overload_candidate_binding_relate_is_returnonly` — a converged-but-not-selected candidate publishes no
`Relate` entry / fact signature / backfill. Plus the cache-identity guard
`resolve_call_same_expr_different_flow_or_substitution_does_not_warm_hit` whose discriminating fixture MUST
include the **different-candidate-contextual-param** case (not only different flow/substitution).

---

## Decision 2 — Flow narrowing (`FlowNarrowingAt`) + NARROW_INVALIDATION — the invalidation-soundness CRUX (ADJUDICATED FIX)

`FlowNarrowingAt { point: ProgramPointId }` → `ProgramAnalysis` value domain (`ProgramAnalysisContext`
{P,R}+flow+contextual+subst, env dims P R T L J), facts in `FactDomain::ProgramAnalysis`, published via
`ProgramAnalysisGraph` — **never a `GraphTypeNode` arm** (qvd §2.2). The narrowing JOIN ALGEBRA lives on the
shared `FlowFrame` lattice (positive / negative / intersection / union composition for conflicting predicates;
`AliasCorrelation` for destructured-discriminant correlation), built across the `U6.NARROW_*` sub-blocks on the
ONE `FunctionFlowGraph`'s `narrowing-predicate` edge class — never a second flow structure.

### 2.1 The crux: exactly when a cached narrowing invalidates

A cached narrowing at a point is valid ONLY while the **control-flow BASIS** that produced it is unchanged. The
`ProgramAnalysis` fact must root on the basis so that an edit to the basis MISSES the warm narrowing. This is the
flow analogue of relation-infer's admission-soundness crux (`ReadSetSignature.self_root_canonicals` + `facts`).

**The corrected rationale — what is the validity rail, and what is merely a discriminant.** The `FlowSlice`
fact records `{ function_slot, projection_path, slice_hash, selected_binding_ids, selected_effect_ids,
selected_control_region_ids, closure_summary_ids }` + `flow_body_stable_hash`. The parent subplan's prose
("the extra `FlowSlice` fields are required because effect-only changes must invalidate") is **true but
mechanism-ambiguous**, and reading it as "root validity on the slice's recorded `selected_effect_ids`" is the
**under-rooting bug** (it ships the fi02 defect — §2.3). The corrected mechanism:

- The `selected_binding_ids` / `selected_effect_ids` / `selected_control_region_ids` / `closure_summary_ids` /
  `slice_semantic_hash` are a **deterministic function of `(FunctionFlowGraph, ReturnProjectionDemand)`**. They
  are **CANDIDATE-SELECTION DISCRIMINANTS** — they distinguish concurrent slice candidates under one
  `function_slot`-rooted slot (a `Skeleton` slice vs a bound-expansion slice; different demand points coexisting
  as candidates). They are **NOT a re-derived validation rail**: re-deriving them on a warm read means
  re-running the cold `ReturnPathPeeker` reachability (the structural ids) and re-resolving each on-path callee's
  effect summary (the negative "does-not-invalidate" classifications) — i.e. the exact cold work the cache
  exists to avoid. Given an unchanged `flow_body_stable_hash` the live graph is structurally identical, so a
  re-derivation reproduces byte-identical ids — adding **zero** invalidation precision; given a changed hash it
  is already a warm miss. So they CANNOT be the rail.
- The only gate the validator can evaluate cheaply on a warm read is the **whole-function
  `flow_body_stable_hash`**: it is computed ONCE during shallow analysis and lives in/under `IndexedReady`
  (eagerly available, no re-plan), body-SENSITIVE and cosmetic-INSENSITIVE.
  `StoreView::validates_program_analysis_domain` re-derives the live function's `flow_body_stable_hash` from the
  current `FunctionFlowGraph` and **fails closed** on any mismatch. Because it is WHOLE-BODY-sensitive it busts
  on the **INTRODUCTION** of an effect the old slice never traversed — not merely on a change to an
  already-selected one. That is what makes both fi01 and fi02 sound.

### 2.2 THE FINAL ROOTING RULE (the analog of `self_root_canonicals` + `facts`)

> A `FlowReturn` / `FlowNarrowingAt` value is version-rooted by
> `FactVersionRef::ProgramAnalysis(ProgramAnalysisFactRef { function_slot, projection_path,
> flow_body_stable_hash, slice_semantic_hash })` **plus** the union of every consumed sub-dispatch's
> `ReadSetSignature.facts` — notably the consumed `ResolveCall` fact-set (for predicate/assertion carriers and
> cross-file callee signatures), the consumed closure/callee **effect-summary** facts, and
> `Member` / `MemberPresence` / `LibIntrinsic` / `TypeEnvOptions` / route / project-generation facts.
> Warm-read validity is the **conjunction**:
> - **(a)** the live function's `flow_body_stable_hash` (re-derived from the current `FunctionFlowGraph` in
>   `IndexedReady`, available WITHOUT re-planning the slice) **equals** the recorded `flow_body_stable_hash` —
>   the **SOLE intra-function invalidation gate**, whole-body, fail-closed; **AND**
> - **(b)** every consumed fact in the unioned `ReadSetSignature.facts` revalidates against the live
>   `StoreView` — the **cross-function / cross-file gate**.
>
> `slice_semantic_hash` and `selected_*_ids` are candidate-selection discriminants; given (a) they are
> reproducible-by-construction and add NO invalidation precision — they MUST NOT be relied on as an independent
> re-derived rail.

**Over-invalidation is FORCED and correct.** Whole-body `flow_body_stable_hash` busts every cached slice of a
function on any body edit (even an edit to an independent branch). This is **not a free choice**: the validator
cannot recompute a per-slice basis on the warm path without re-planning, which defeats the cache. It is
correctness-safe and is the design intent. **The per-slice tightening is REJECTED** — pursuing it is the
under-rooting bug of §2.3. The doc must stop blurring the **region root** (`flow_body_stable_hash`, ALWAYS
whole-body, the validity rail) with the **slice hash** (`FlowSliceHashNode` / `slice_semantic_hash`,
only-reachable-slice, a discriminant; a full-body slice hash is rejected for member-projection) — these are
different objects.

`FlowSliceBudget` overflow / cycle / partial / cancellation ⇒ `ReturnOnly` (relation-infer admission-table
row 4 analog). The keys stay R6 content-free; `flow_body_stable_hash` rides the value, never the key.

### 2.3 Worked example — the narrowing-invalidation case (fi01 + fi02)

```ts
function f(x: unknown) {
  if (typeof x === "string") {     // guard: narrows x to `string`
    /* ...statements... */
    return x.length;               // USE: x is `string` here — narrowing cached at this ProgramPointId
  }
}
```
- **Cold compute** cuts a slice for the USE point: the `narrowing-predicate` edge from the `typeof` guard, the
  control region of the `if`-true branch, and the effects between guard and use (here: none invalidating). The
  published `FlowNarrowingAt` value records `flow_body_stable_hash(f)` + the consumed facts; the narrowed type is
  `string`. The `selected_*_ids` on the `FlowSlice` fact discriminate this candidate from a `Skeleton` or a
  different-demand-point slice.

- **fi01 — reassignment inserted between guard and use (MUST invalidate):**
  ```ts
    if (typeof x === "string") {
      x = computeUnknown();        // NEW write — reassignment
      return x.length;             // x is no longer narrowed
    }
  ```
  The body edit changes `flow_body_stable_hash(f)` → gate (a) FAILS → warm miss → recompute, which now sees the
  reassignment effect and does NOT narrow. **Sound.** A slice-local-effect-id rail would also catch this (the
  edit is on-path), which is why fi01 alone does not expose the bug.

- **fi02 — the load-bearing case: a previously-opaque call later becomes invalidating (MUST invalidate, and is
  exactly where a slice-local rail is UNSOUND):**
  ```ts
    if (typeof x === "string") {
      opaqueHelper();              // originally does NOT assign x → narrowing PRESERVED across it
      return x.length;             // x still `string`
    }
  ```
  Cold compute records the narrowing as preserved *because* `opaqueHelper()` does not write `x` — the **absence
  of an invalidating effect is load-bearing**. Now an edit makes `opaqueHelper` (possibly in another file)
  assign to a captured/aliased `x`, or the local body inserts `x = …`:
  - If the invalidating write is **local** to `f`: `flow_body_stable_hash(f)` changes → gate (a) fails → miss.
    A slice-local rail rooted on the *recorded* `selected_effect_ids` would VALIDATE (the new write is not in the
    cached effect set — it did not exist when the slice was cut) and serve the **stale narrowing** — the exact
    unsoundness. The whole-body hash fails closed on the *introduction* of an effect.
  - If the invalidating change is in a **cross-file callee** (`opaqueHelper`'s body now writes through a closure
    capture, changing its effect summary): `flow_body_stable_hash(f)` is unchanged (the change is not in `f`'s
    body), but the consumed callee **effect-summary fact** in the unioned `ReadSetSignature.facts` is stale →
    gate (b) fails → miss. This is why the rule is a **conjunction** — the body hash alone cannot see cross-file
    callee effects.

Named deferred guards (owned by U6.NARROW_INVALIDATION / U6.FLOW_RETURN_SUBSTRATE, land WITH the variant):
`flow_narrowing_roots_on_body_stable_hash_not_per_slice`, `program_analysis_fact_domain_validates_flow_slice`,
`flow_slice_keys_on_body_sensitive_hash_not_parse_stable_hash`,
`narrowing_facts_are_program_analysis_not_graph_type_nodes`.

---

## Decision 3 — `PredicateAssertion` (U6.PREDICATE_ASSERTION) + the fi08 ownership (closes the fi08 deadlock class)

### 3.1 Mechanism

Type-guard functions (`x is T`) and assertion functions (`asserts x is T` / `asserts cond`) are carried as
`SignatureEffect::{ Predicate { param_idx, narrowed_to }, Assertion { param_idx, narrowed_to },
AssertsCondition { effect } }` metadata on function signatures — **NOT** standalone published `TypeExpr` /
`GraphTypeNode` type nodes. `assertion_effect` carries a **dotted-member-path** (`asserts obj.prop is T` narrows
`obj.prop`). The carrier is lowered **once** by `lower_ts_type` for `TSTypePredicate` during shallow analysis
(typed-IR-only — no query-time re-parse). The solver's caller-side branch substitution: on a `FlowExpr::Call`
whose resolved callee signature carries the effect, apply the **positive** fact on the true branch and the
**negative** fact on the false branch (predicate); apply the **asserted** fact past the call (assertion); and
**instantiate `narrowed_to` at the call site against `normalized_type_args`** for generic predicates. A
signature-only (declared, body-less) predicate still applies its signature fact. A consumer asking for the
*return type* of a predicate function gets its `boolean` (or the asserted result), **never the carrier**.

### 3.2 The fi08 ownership (the deadlock class, closed)

`flow_invalidations_fi08_asserts_narrows_dotted_member_path` has substrate `FlowNarrowing` but its **dominant
`mechanism_id` is the assertion-effect-on-a-dotted-path engine, owned by `U6.PREDICATE_ASSERTION`**. The U0 DAG
guard **check-2** (`row.mechanism_id` owner == `row.block_id`) therefore FORCES
`fi08.block_id = U6.PREDICATE_ASSERTION` — it CANNOT sit under `U6.NARROW_INVALIDATION`. The deadlock it closes
is concrete: had `fi08` stayed a `NARROW_INVALIDATION` row while consuming
`PredicateAssertion.assertion_effect_dotted_member_path` (owned by `U6.PREDICATE_ASSERTION`), **check-3**
(consumed-mechanism owner must be a transitive prerequisite) would force
`U6.NARROW_INVALIDATION → U6.PREDICATE_ASSERTION`; but `U6.PREDICATE_ASSERTION` already depends on
`U6.NARROW_INVALIDATION` (it lands its effect ONTO that frame), so the prereq graph would CYCLE and **check-1**
fails. Moving `fi08` to `U6.PREDICATE_ASSERTION` makes every edge one-way.

`U6.PREDICATE_ASSERTION` prereqs = { `U6.FLOW_RETURN_SUBSTRATE`, `U6.NARROW_INVALIDATION`,
`U6.NARROW_SUBSTITUTION`, `U6.CALL_RESOLVE` }. Its consumed owners: the narrowing frame (NARROW_INVALIDATION —
declared prereq ✓), the assertion-effect engine (itself ✓), the `Predicate`/`Assertion` carrier read off the
`ResolveCall`-resolved callee signature (CALL_RESOLVE — declared prereq ✓). **No back-edge:** CALL_RESOLVE prereqs
= {FLOW_RETURN_SUBSTRATE, U2.RELATION_INFER, U2.CLASS_SURFACES}; NARROW_* prereq = {FLOW_RETURN_SUBSTRATE};
neither depends on PREDICATE_ASSERTION. The edge `U6.PREDICATE_ASSERTION → {narrowing sub-blocks, CALL_RESOLVE}`
is strictly **one-way → acyclic**.

**The hard ownership boundary (the check-2/3 contract in prose):** `U6.CALL_RESOLVE` may *identify* the resolved
callee signature but must **NOT** lower/apply predicate/assertion effects; the `U6.NARROW_*` sub-blocks may
provide the generic narrowing primitives but must **NOT** consume `assertion-effect-on-dotted-path`;
`U6.PREDICATE_ASSERTION` is the **SOLE owner** of that mechanism.

### 3.3 The dotted-path rooting obligation (per Decision 2)

The dotted-member-path assertion narrowing fact roots on **BOTH** (a) the local `flow_body_stable_hash` (an edit
reassigning `obj.prop` between the assertion and the use changes `f`'s body → warm miss) **AND** (b) the
**unioned consumed `ResolveCall` callee-carrier facts** — the `Predicate`/`Assertion` carrier is read off the
(possibly cross-file) callee signature, so `asserts x is string` → `asserts x is number` must invalidate. The
U6.PREDICATE_ASSERTION contract's "Facts read" line MUST explicitly name the unioned `ResolveCall` carrier facts,
not only `Member`/`MemberPresence`.

Named deferred guards (owned by U6.PREDICATE_ASSERTION):
`predicate_signature_without_body_audits_signature_only_outcome`,
`predicate_assertion_effect_is_signature_metadata_not_published_type_node`.

---

## Decision 4 — `ContextualTypeAt` (U6.CONTEXTUAL_CALLBACK): contextual typing + callback inference

### 4.1 Mechanism (ordering + the session-fixation fixed-point — no second engine)

Callee → callback contextual typing flows **BEFORE** the callback's return is solved. A **nested `FlowFrame` per
callback invocation** pre-binds the callback's parameters to the contextual types derived from the callee
signature, then solves the callback body in that frame, returning a `FlowReturnResult` the outer call's signature
resolution consumes for generic inference (depositing candidates into the session's `InferenceInfo`). The
iterative generic-inference loop (when contextual typing of a callback param depends on a type variable also
constrained by another argument) **IS the session's FIXATION fixed-point** running INSIDE the active
`InferenceSession` of the enclosing `CheckerTransaction` — there is **no separate callback-inference loop
engine** (relation-infer §3.4). The cross-engine cycle `ResolveCall → FlowReturn(callback) → ResolveCall`
discharges on the shared `CheckerReentryStack` (distinct nodes keyed by full normalized identity), never a
self-await or budget-spin. Object-literal-argument contextual typing (`acc` in
`arr.reduce((acc, item) => …, {} as Record<string, V>)`) is isolated to the nested frame and must NOT pollute the
caller frame. `ThisType<T>` contextual `this` binding is supplied through `ContextualTypeAt` (no apparent
members, a `ProgramAnalysisGraph` fact, no surface rewrite).

`ContextualTypeAt { point }` → `ProgramAnalysis` value domain. `FlowInputContext` (the `input` key field of
`FlowReturn`: the contextual callback input signature + the relation/call demand mode) makes two re-entries
differing only in contextual input signature distinct cache candidates.

### 4.2 Convergence — `CompletedDeterministic` is STRONGER than canonical-hash equality (ADJUDICATED FIX — codex tightening)

The loop is bounded by `flow_policy.loop_budget` and abandoned to `ReturnOnly` on budget exhaustion
(relation-infer admission-table rows 4/8). **Convergence on `SubstitutionEnv::canonical_hash()` equality ALONE
is TOO WEAK.** The relation-infer admission rule is "ANY obligation `Unknown`/cancelled/`BudgetExceeded` ⇒ the
entire SCC is `ReturnOnly`." The session reaches `CompletedDeterministic` — and only then admits — iff ALL of:
- **stable substitution** (a `SubstitutionEnv::canonical_hash()` fixpoint across a fixation iteration), AND
- **no pending `ReturnOnly` dependencies** (no consumed sub-dispatch still in-flight / abandoned / budget-exceeded), AND
- **deterministic binding-`Relate` results** (every binding-producing `Relate` it consumed admitted at its own
  session-close — relation-infer §3.3), AND
- **stable contextual-frame / input identity** (the `FlowInputContext` and the per-candidate contextual-target
  identity did not change across the converging iteration).

This is a tightening the locked relation-infer substrate already implies (§3 admission rule); it weakens nothing.

### 4.3 The contextual-target axis must be a resolved IDENTITY (ADJUDICATED FIX — mirror of 1b)

The `ContextualTypeAt` re-keying must keep the **same callback program point under two different outer-overload
candidates DISTINCT**. That is carried by the `contextual` axis of `ProgramAnalysisContext` on the
`ContextualTypeAt` key (qvd §2.2: `ProgramAnalysisContext {P,R} + flow + contextual + subst`). For this to hold,
the `contextual` axis MUST be the **resolved contextual-target type identity** (the candidate's param type), not
a flag — exactly the same requirement as Decision 1(b)'s `ContextSensitiveExprKey.contextual_typing`. The
U6.CONTEXTUAL_CALLBACK contract must state this mirror property explicitly (the SHAPE is locked in qvd §2.2; the
contract must not leave it implicit).

Named deferred guards (owned by U6.CONTEXTUAL_CALLBACK):
`callback_contextual_typing_does_not_pollute_caller_frame`,
`contextual_callback_input_signature_differentiates_cache_candidates`,
`this_type_contextual_object_literal_binding_in_contextual_type_at`.

---

## Decision 5 — VALUE_INFERENCE (U6.VALUE_INFERENCE): object / spread / `satisfies` return shapes

Non-call return shapes — `Spread` / `ObjectMember` / `IndexedAccess` / `TemplateLiteralComputed` / `AsConst` /
`Satisfies` — on the return path:
- **Spread / `Object.assign` reduce LEFT-TO-RIGHT, later explicit writes win** (the two-frontier value-provider
  rule). `return {...a, ...b, k: 1}` reduces with explicit `k: 1` winning.
- **`satisfies`** (oracle-pinned against `tsgo 7.0.0-dev.20260526.1`, never reasoned from prose): `E satisfies T`
  checks assignability of `E` to `T`, contextually types `E` with `T`, then **KEEPS the inferred SOURCE type of
  `E`, not `T`**. Fresh object literals get excess-property checks unless the target admits the key; source keys
  are retained. The return-position widening that applies to the kept source literal is characterized by
  `satisfies_widens_inner_value_to_primitive_without_as_const`.
- **Per-property freshness / spread-taint is SESSION-OWNED** (U2.RELATION_INFER §4.2 —
  `freshness_tracks_per_property_spread_taint`) and **CONSULTED on the return path, NEVER reimplemented** — there
  is NO second excess-check engine. Mapped / conditional return annotations instantiate against body-derived
  types via the existing `MappedType` / `Conditional` reducers.

Composition with Decision 2: the kept source type's narrowing facts root on `flow_body_stable_hash`; the
`satisfies` validation reads target `Member` facts folded into the unioned `ReadSetSignature.facts` (the
cross-function half of the D2 rule). Spread/`Object.assign` facts root property sources, overwrite order, and
consulted freshness facts via the same unioned read-set — no separate rail.

**ADJUDICATED doc FIX (dual-landing / second-walker risk):** U6.VALUE_INFERENCE must cite the `peeker.rs`
right-to-left value-provider scan + definite-write suppression as a **CONSUMED mechanism owned by
U6.FLOW_RETURN_SUBSTRATE** (which LANDS it), **NOT** re-list `peeker.rs` as a file it changes — re-listing
invites a second value-provider walker (the precise second-engine class). The guard
`flow_slice_is_graph_reachability_not_procedural_walk` (landed in the substrate) covers it.

Named deferred guards (owned by U6.VALUE_INFERENCE): `satisfies_does_not_widen_returned_value`,
`flow_return_spread_reduces_left_to_right_later_write_wins`; `freshness_tracks_per_property_spread_taint` is
EXERCISED here (owned at U2.RELATION_INFER) and must not be regressed.

---

## Decision 6 — Consistency with landed CRITICAL invariants (NEVER weaken) + the depth-sentinel GAP

| Invariant | U6 conformance |
|---|---|
| ONE resolver | flow/call/narrowing/contextual are nodes of the single `SemanticGraphStore` dispatch; `ResolveCall` is a shared key, not a body-solver helper; callee resolution is typed-IR. |
| typed-IR-only | the OXC scanner (`infer_return_type` et al., LIVE at `type_eval_build.rs:1119`) is DELETED in U6.FLOW_RETURN_SUBSTRATE; no source-slicing / regex / `parse_type_annotation` at query time. |
| `ProgramAnalysisGraph`, never `GraphTypeNode` | flow/narrowing/predicate/contextual facts publish through `ProgramAnalysisGraph` (qvd §2.2); predicate/assertion carriers are `SignatureEffect` metadata, never a published return-type node. |
| R21 five split env hashes | `FlowReturnContext` / `ResolveCallContext` / `RelationContext` / `ProgramAnalysisContext` carry `R T L J` (+ `P` where the producer reads parse-env) split, never a bundled `project_config_hash`. |
| R6 version-rooting on the value | the `FlowReturn` / `ResolveCall` query-identity keys carry NO `flow_body_stable_hash` / `parse_stable_hash` / `fact_dep_signature`; the body hash rides the content-addressed `FlowSliceHashNode` artifact node + the `FlowSlice` fact's `FactVersionRef::ProgramAnalysis` (the documented two-family split — content-addressed artifact caches DO carry the body hash, query-identity caches do NOT). No violation. |
| shallow-by-default | `ReturnType<typeof callee>` projector admission is path-precise; no eager body expansion. |
| one shared `CheckerReentryStack` | the `FlowReturn` / `ResolveCall` / `ContextualTypeAt` / `FlowNarrowingAt` typed views are wired onto the U2/RI-3-built stack AT U6; U6 owns `checker_reentry_graph_spans_flow_call_contextual_narrowing` + `cross_engine_cycle_discharge_admits_only_stable_deterministic_results`. |

**Explicit NEGATIVE guards (named, deferred to the owning sub-block, land WITH the behavior):**
`legacy_return_scanner_removed` + `flow_solver_never_slices_source_text` (no query-time OXC scanner path);
no fallback call-return resolver; `narrowing_facts_are_program_analysis_not_graph_type_nodes` +
`no_flow_slot_in_published_type_surface` (no `ProgramAnalysis` fact through `GraphTypeNode`);
`flow_slice_keys_on_body_sensitive_hash_not_parse_stable_hash` (no content/version/fact hash on a query-identity
key).

### 6.1 The flow depth-sentinel GAP (ADJUDICATED — a real legacy-deletion omission)

relation-infer §2.4 / Rescope (lines 28, 680) **assigns the flow depth-sentinel retirement to U6** ("the flow
depth-sentinel retirement is DEFERRED to U6 — it is replaced by the `FlowReturn` view of `reentry_stack`"). Yet
NO U6 block's Legacy Deletions list this deletion (U6.FLOW_RETURN_SUBSTRATE names only the scanner + the
opaque-`ReturnType` carrier; U6.CALL_RESOLVE names only the in-body call resolver + text callee resolution + the
second `ReturnType` route). Per CLAUDE.md ("without explicit legacy deletion lists, agents skip deletions and
leave dual paths alive") this leaves a SECOND cycle-control path (a depth counter) alongside the `FlowReturn`
view of the shared `CheckerReentryStack` — the divergence/hang class the architecture forbids (the same argument
relation-infer §2.4 makes for `RefCycleResultDb`).

**Assign the deletion to `U6.FLOW_RETURN_SUBSTRATE`** (that block establishes the flow cycle-id space as the
flow-typed VIEW of the ONE shared `CheckerReentryGraph`). Add to its Legacy Deletions:
> "the flow/return-path depth-recursion sentinel (the `MAX_RESOLVE_DEPTH` / `HostConfig.depth_budget`-based
> return-type recursion guard applied on the flow/return path) — replaced by the `FlowReturn` view of
> `reentry_stack`; no depth counter coexists with the shared re-entry stack on the flow path,"
with the guard `no_depth_sentinel_on_flow_return_path`.

**Required escape clause (verified against the tree):** the live depth guards are `MAX_RESOLVE_DEPTH = 128`
(`session/types.rs:2148`, external type resolution), `HostConfig.depth_budget` (defaults to
`component_meta_materialize::MAX_DEPTH = 4096`), `RELATION_MAX_DEPTH = 192` (relation stack-safety); **no
`flow_depth` / `depth_sentinel` symbol exists today**. If, on implementation, the only live `depth_budget` is the
generic path-projection / component-meta-materialize / external-resolution guard and it is provably **NOT** on
the flow-return cycle path, the U6.FLOW_RETURN_SUBSTRATE contract MUST state so explicitly and record that there
is no flow depth-sentinel to retire (a no-op deletion line) — the locked upstream asserts there is one, so this
must be **resolved in-doc, not left silent**, and the `no_depth_sentinel_on_flow_return_path` guard must assert
the absence of any depth counter on the flow-return path regardless.

---

## Implementation mini-DAG (U6 sub-blocks)

```
U6.FLOW_RETURN_SUBSTRATE                 ← U2.QUERY_VALUE_DOMAIN, U2.RELATION_INFER, U4
   ├── U6.CALL_RESOLVE                   ← FLOW_RETURN_SUBSTRATE, U2.RELATION_INFER, U2.CLASS_SURFACES[shape-only]
   ├── U6.NARROW_{TYPEOF,EQUALITY,TRUTHINESS,IN,INSTANCEOF,DISCRIMINATED,SUBSTITUTION,INVALIDATION}  ← FLOW_RETURN_SUBSTRATE   (mutually independent)
   ├── U6.ASYNC_GENERATOR                ← FLOW_RETURN_SUBSTRATE
   ├── U6.VALUE_INFERENCE                ← FLOW_RETURN_SUBSTRATE, CALL_RESOLVE
   ├── U6.PREDICATE_ASSERTION            ← FLOW_RETURN_SUBSTRATE, NARROW_INVALIDATION, NARROW_SUBSTITUTION, CALL_RESOLVE
   ├── U6.CONTEXTUAL_CALLBACK            ← CALL_RESOLVE, FLOW_RETURN_SUBSTRATE, NARROW_DISCRIMINATED
   ├── U6.CROSS_FILE                     ← VALUE_INFERENCE, CALL_RESOLVE
   └── U6.LOOP_CLOSURE                   ← CALL_RESOLVE, PREDICATE_ASSERTION
```
Topological order (one valid linearization): FLOW_RETURN_SUBSTRATE → CALL_RESOLVE → {NARROW_*} → ASYNC_GENERATOR
→ VALUE_INFERENCE → PREDICATE_ASSERTION → CONTEXTUAL_CALLBACK → CROSS_FILE → LOOP_CLOSURE. **Acyclic.** Every
consumed key/mechanism owner is a transitive prerequisite (U0 DAG-guard checks 3/4): CALL_RESOLVE consumes
`Relate`/`ResolveOverloadSet`/`ResolveClassSurface`/`ApparentType` (U2 ✓) and PRODUCES `ResolveCall` (U6-owned);
PREDICATE_ASSERTION consumes `ResolveCall` + the narrowing frame (✓); CONTEXTUAL_CALLBACK consumes `ResolveCall`
+ narrowed callback-param facts (NARROW_DISCRIMINATED ✓); VALUE_INFERENCE consumes the call-driven cases
(CALL_RESOLVE ✓) + `MappedType`/`Conditional`/`IndexedAccess`/`TemplateLiteralReduce`/`Relate` (U2 ✓); CROSS_FILE
consumes object returns (VALUE_INFERENCE ✓) + cross-file callees (CALL_RESOLVE ✓); LOOP_CLOSURE consumes
`ResolveCall` (✓) + predicate detection (PREDICATE_ASSERTION ✓).

### The CONTEXTUAL_CALLBACK mechanism-ownership split (ADJUDICATED FIX — REQUIRED for DAG-guard check-2/3)

There is a latent check-2/3 hazard: during overload fixation, `U6.CALL_RESOLVE` types callback-arg bodies in a
nested `FlowFrame`. If that callback-body-typing mechanism were labeled owned by `U6.CONTEXTUAL_CALLBACK`, then
CALL_RESOLVE would consume a CONTEXTUAL_CALLBACK-owned mechanism while CONTEXTUAL_CALLBACK depends on
CALL_RESOLVE → **check-3 fails (consumed-mechanism owner not a transitive prereq) → the block graph cycles**.
The locked resolution **splits the mechanism**:
- The contextual-callback-input pre-binding **DURING session fixation** (the transient nested-`FlowFrame`
  machinery driven by `FlowInputContext`, run inside the `InferenceSession`) → **owned by
  `U6.FLOW_RETURN_SUBSTRATE`** (+ the U2.RELATION_INFER session machinery) — a transitive prereq of
  CALL_RESOLVE ✓.
- The **published `ContextualTypeAt` query** (the stable contextual-target identity for a program point) +
  `ThisType<T>` → **owned by `U6.CONTEXTUAL_CALLBACK`** — consumes `ResolveCall` (CALL_RESOLVE prereq ✓).

With this split no edge reverses: CALL_RESOLVE consumes only FLOW_RETURN_SUBSTRATE / session mechanisms;
CONTEXTUAL_CALLBACK consumes `ResolveCall`. This is the "there is no second engine" framing (Decision 4) made
mechanically check-2/3-clean. The U6 manifest rows MUST register `mechanism_id` / `consumed_mechanisms` /
`mechanism_owning_block` accordingly.

**Doc-accuracy note:** the parent subplan's ASCII "Block dependency graph" (`native-flow-return.md` ~lines
79–113) under-draws edges — it shows VALUE_INFERENCE / CROSS_FILE / LOOP_CLOSURE as direct children of
FLOW_RETURN_SUBSTRATE, but their contracts add VALUE_INFERENCE ← +CALL_RESOLVE; CROSS_FILE ← VALUE_INFERENCE +
CALL_RESOLVE; LOOP_CLOSURE ← CALL_RESOLVE + PREDICATE_ASSERTION. Acyclicity is pinned by the contracts (the DAG
guard builds from `TYPEINFO_PARITY_BLOCKS`), not the ASCII; the ASCII should be redrawn to match the contracts
or annotated as keystone-edge-only.

---

## Now-landable-guard decision

This is a **doc/plan gate**: the only artifact that lands here is this design doc. Per the project's
three-artifact CRITICAL-rule policy, a `(CRITICAL)` heading may only land WITH a guard that can be written
against the current tree.

- **No guard that NAMES `FlowReturn` / `ResolveCall` / `ContextualTypeAt` / `FlowNarrowingAt` can land at this
  gate.** The standing meta-guard `semantic_query_key_spec_table_equals_enum` REJECTS any tree referencing those
  variants before they are registered (registration is U6 implementation work). So every cache-identity guard
  (`resolve_call_key_covers_*`, `resolve_call_same_expr_different_flow_or_substitution_does_not_warm_hit`,
  `flow_return_key_covers_*`), every cross-engine guard
  (`checker_reentry_graph_spans_flow_call_contextual_narrowing`,
  `cross_engine_cycle_discharge_admits_only_stable_deterministic_results`), and every `FlowSlice`/domain guard
  (`program_analysis_fact_domain_validates_flow_slice`,
  `flow_slice_keys_on_body_sensitive_hash_not_parse_stable_hash`,
  `flow_narrowing_roots_on_body_stable_hash_not_per_slice`) is **SPECIFIED-in-doc + NAMED + owned by its future
  U6 sub-block**, landing WITH the variant it tests.
- **The grep/structural guards are RED-now** (`legacy_return_scanner_removed` /
  `flow_solver_never_slices_source_text` / `no_flow_slot_in_published_type_surface` assert the absence of code
  LIVE at `type_eval_build.rs:1119`, or the presence of a `flow` module that does not exist yet). They go green
  only when U6.FLOW_RETURN_SUBSTRATE lands. Deferred, owned by U6.FLOW_RETURN_SUBSTRATE.
- **No NEW `(CRITICAL)` rule text is minted by U6 into `CLAUDE.md` / a `.claude/skills/*/SKILL.md` at this
  gate.** The U6 block contracts CITE the parent's existing `(CRITICAL)` demand-sliced-flow / one-resolver /
  typed-IR-only / shallow-by-default rules. The two genuinely-new architectural rules this design pins — "flow
  narrowing roots on whole-body `flow_body_stable_hash`, never a per-slice basis" (Decision 2) and "no
  depth-sentinel coexists with the shared re-entry stack on the flow path" (Decision 6.1) — have guards
  (`flow_narrowing_roots_on_body_stable_hash_not_per_slice`, `no_depth_sentinel_on_flow_return_path`) that
  **cannot be written against the current tree** (the flow substrate is not built; the meta-guard rejects the
  variants; the scanner is still live). Per the gate rule "do NOT add a CLAUDE.md `(CRITICAL)` heading whose
  guard can't be written yet," these are **SPECIFIED here + NAMED + owned by U6.FLOW_RETURN_SUBSTRATE /
  U6.NARROW_INVALIDATION**, and the `(CRITICAL)` heading + `CRITICAL_RULE_GUARDS` row land in the SAME change as
  the guard when that sub-block is implemented. The R6 meta-guard set is therefore UNCHANGED at this gate (it
  scans `CLAUDE.md` + `.claude/skills/*/SKILL.md` only, never `docs/arch/*`).
- **The ONE mechanically-discriminating-NOW artifact** is the U0 block-metadata DAG guard
  `typeinfo_parity_block_dag_is_acyclic_and_consumed_keys_and_mechanisms_are_prereqs` — it operates on
  `TYPEINFO_PARITY_BLOCKS` + `mechanism_id` / `consumed_mechanisms` / `mechanism_owning_block` **manifest
  metadata, NOT `SemanticQueryKey` variants**, so it is NOT blocked by the meta-guard. It discriminates the two
  design decisions this gate pins: **check-2 = the fi08 ownership** (`fi08.block_id = U6.PREDICATE_ASSERTION`)
  and **checks-1/3/4 = the mini-DAG acyclicity + the CONTEXTUAL_CALLBACK mechanism-ownership split**. It goes
  live the moment the U6 block rows + their mechanism metadata are registered in the manifest (a U0 / early-U6
  manifest extension) — the earliest point any U6 design content becomes mechanically checkable, ahead of the
  query variants. **This gate does not register those rows** (registration is owned by the U6 manifest-extension
  step, not this design block); it LOCKS the metadata values they must carry.

The deferred-guard → owner registry below is this gate's landable artifact.

| Guard | Owner sub-block | Lands |
|---|---|---|
| `losing_overload_candidate_binding_relate_is_returnonly` | U6.CALL_RESOLVE | with `ResolveCall` variant |
| `resolve_call_key_covers_args_this_contextual_type_overload_policy_and_context` | U6.CALL_RESOLVE | with variant |
| `resolve_call_same_expr_different_flow_or_substitution_does_not_warm_hit` (+ different-candidate-contextual-param fixture) | U6.CALL_RESOLVE | with variant |
| `checker_reentry_graph_spans_flow_call_contextual_narrowing` | U6.CALL_RESOLVE | with the 4 wired views |
| `cross_engine_cycle_discharge_admits_only_stable_deterministic_results` | U6.CALL_RESOLVE | with the cross-engine cycle |
| `flow_narrowing_roots_on_body_stable_hash_not_per_slice` | U6.NARROW_INVALIDATION | with the narrowing rail |
| `program_analysis_fact_domain_validates_flow_slice` | U6.FLOW_RETURN_SUBSTRATE | with the `FlowSlice` fact |
| `flow_slice_keys_on_body_sensitive_hash_not_parse_stable_hash` | U6.FLOW_RETURN_SUBSTRATE | with the key |
| `no_depth_sentinel_on_flow_return_path` | U6.FLOW_RETURN_SUBSTRATE | with the cycle-id space |
| `legacy_return_scanner_removed` / `flow_solver_never_slices_source_text` | U6.FLOW_RETURN_SUBSTRATE | with scanner deletion |
| `narrowing_facts_are_program_analysis_not_graph_type_nodes` / `no_flow_slot_in_published_type_surface` | U6.FLOW_RETURN_SUBSTRATE | with `ProgramAnalysisGraph` |
| `flow_slice_is_graph_reachability_not_procedural_walk` | U6.FLOW_RETURN_SUBSTRATE | with the planner |
| `callback_contextual_typing_does_not_pollute_caller_frame` | U6.CONTEXTUAL_CALLBACK | with the nested frame |
| `this_type_contextual_object_literal_binding_in_contextual_type_at` | U6.CONTEXTUAL_CALLBACK | with `ContextualTypeAt` |
| `predicate_assertion_effect_is_signature_metadata_not_published_type_node` | U6.PREDICATE_ASSERTION | with the carrier |
| `satisfies_does_not_widen_returned_value` / `flow_return_spread_reduces_left_to_right_later_write_wins` | U6.VALUE_INFERENCE | with the return shapes |

---

## Residual risks (recorded)

1. **Whole-body over-invalidation (Decision 2).** A body edit to an independent branch of a large function busts
   every cached slice of it. FORCED + correctness-safe (the validator cannot recompute a per-slice basis on the
   warm path without re-planning). Mitigation: the `FlowSliceHashNode` slice cache + the candidate discriminants
   keep recompute scoped to the demanded slice; benched at U10/U3. Per-slice tightening is REJECTED, not deferred.
2. **Cross-file callee effect-summary fact granularity (Decision 2(b) / Decision 3(b)).** The unioned consumed
   facts must include each on-path callee's effect summary at a granularity that flips when the callee becomes
   invalidating; an under-granular summary fact re-introduces the fi02 cross-file miss. Pinned by the
   narrowing-invalidation guards' cross-file fixtures (owned by U6.NARROW_INVALIDATION / U6.CROSS_FILE).
3. **Speculative-session fan-out (Decision 1).** One `InferenceSession` per overload candidate × per fixation
   iteration is the highest-cost corner; bounded by `CallResolutionBudget` (overload candidates + inference
   bindings + contextual passes) with three-layer `BudgetExceeded` → `ReturnOnly`. Bench-gated at U10.
4. **CONTEXTUAL_CALLBACK mechanism-ownership split (mini-DAG).** If the fixation-time contextual pre-binding is
   mis-labeled as CONTEXTUAL_CALLBACK-owned, DAG-guard check-3 fails. Pinned by the U0 DAG guard once the U6
   manifest rows register their `mechanism_owning_block`.

---

## Rescope / consumers

- **U6.FLOW_RETURN_SUBSTRATE** owns the demand-sliced flow substrate, the `FlowReturn` variant + value arm, the
  `FlowSlice` fact + `validates_program_analysis_domain` rail (Decision 2), the scanner deletion + the
  depth-sentinel retirement (Decision 6.1), the fixation-time contextual pre-binding mechanism (mini-DAG split),
  and the shared `CheckerReentryStack` flow cycle-id view.
- **U6.CALL_RESOLVE** owns the `ResolveCall` variant + value arm + the speculative-session overload algorithm
  (Decision 1), the loser-abandonment admission predicate, `CallResolutionBudget`, the wiring of the four typed
  views onto the shared `CheckerReentryStack`, and the deferred guard
  `checker_reentry_graph_spans_flow_call_contextual_narrowing` +
  `cross_engine_cycle_discharge_admits_only_stable_deterministic_results`.
- **U6.NARROW_*** own the narrowing JOIN ALGEBRA on the shared `FlowFrame` lattice; **U6.NARROW_INVALIDATION**
  additionally owns the narrowing-invalidation rail guard (Decision 2) and the fi01/fi02 fixtures.
- **U6.PREDICATE_ASSERTION** owns the `SignatureEffect` carrier + the assertion-effect-on-dotted-path engine +
  the fi08 row (Decision 3); it is the SOLE owner of `assertion-effect-on-dotted-path`.
- **U6.CONTEXTUAL_CALLBACK** owns the published `ContextualTypeAt` query + `ThisType<T>` (Decision 4 / mini-DAG
  split), NOT the fixation-time pre-binding.
- **U6.VALUE_INFERENCE** owns the object/spread/`satisfies` return shapes (Decision 5); consults the
  session-owned freshness algorithm and the FLOW_RETURN_SUBSTRATE-owned `peeker.rs` scan, reimplementing
  neither.
- **U0 manifest extension** registers the U6 block rows + `mechanism_id` / `consumed_mechanisms` /
  `mechanism_owning_block` metadata so the DAG guard's check-2 (fi08 ownership) + checks-1/3/4 (acyclicity + the
  CONTEXTUAL_CALLBACK split) go live — the one now-discriminating mechanism this gate pins values for.
- **U8 / wire** consumes the `ProgramAnalysisGraph` projections (flow/narrowing/contextual facts), never a
  `GraphTypeNode` flow arm.
- **U10.RESULT_DB** lands the demand-lattice exactness publish gate over the `FlowReturn` multi-candidate slots
  and benches the speculative-session / over-invalidation costs.
