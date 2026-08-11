# U6 flow-return: measured gaps and the target design

**Status.** `U6.FLOW_RETURN_SUBSTRATE` has landed its substrate (skeleton, graph, demand
planner, slice/cache rails, dispatch, audited public seam). It also, outside its scope,
grew procedural approximations of narrowing, closure capture, and abrupt-completion
semantics that belong to blocks which have not landed. Those approximations are the source
of every defect recorded below.

This document records what is measurably missing, why the current shape keeps regenerating
the same defect class, who owns each remaining piece, and the target design. It is written
so the work can be picked up without re-deriving any of it.

Companion documents: [`native-flow-return.md`](native-flow-return.md) (the ratified
substrate design and block decomposition) and
[`u6-flow-call-resolution-design.md`](u6-flow-call-resolution-design.md) (the `this`/capture
and call-resolution contracts).

---

## 1. What is measurably wrong

Eleven divergences from the pinned checker, all confirmed by direct measurement — a probe
binary through the public API (`VerterHost::upsert` →
`get_flow_return_type_with_audit(..., whole_return())`) against
`tsgo 7.0.0-dev.20260526.1 --ignoreConfig --strict --noEmit`, with a negative control
proving the oracle probe discriminates in both directions.

Every one is **wrong-and-warm**: `degradation=None`, admitted to cache, and served from
cache on replay. Three fabricate `any`. This is the defect class the cache contract exists
to prevent — the cache is faithfully warming a producer-certified wrong answer.

| # | shape | tsc | current |
|---|---|---|---|
| G1 | `function f(x: string) { if (typeof x === "number") return "dead" as const; return "live" as const }` | `"dead" \| "live"` | `"live"` |
| G2 | same via user-defined predicates, ternary position | `{ v: string \| { dead: boolean } }` | `{ v: string }` |
| G3 | `unique symbol` discriminants — provably disjoint intersection survives | `{ v: string }` | `{ v: string \| (A & B) }` |
| G4 | IIFE capture-write in a declarator initializer | `{ b: number }` | `string` |
| G5 | IIFE capture-write in an `if` test | `{ b: number }` | `string` |
| G6 | write after closure creation | `() => "a" \| "b"` | `() => "a"` |
| G7 | sibling-closure / deeper-closure write | `() => "a" \| "b"` | `() => "a"` |
| G8 | unannotated mutable capture | `() => "b"` | `() => string` |
| G9 | guard established before closure creation | `() => string` | `() => string \| number` |
| G10 | labeled / `try` / `throw` suffix return | `"a" \| "b"` | `"a" \| undefined` |
| G11 | closure in a sequence operand; arrow capturing typed `this` | `() => "a"` | `any` / `() => any` |

`ClassExpression` in a field initializer also publishes `any`; that one is already recorded
as a known ledger entry (residual non-call `any` fabrication) rather than a new finding.

### Why the round counts were misleading

Eight adversarial review rounds produced confirmed-major counts of 13, 6, 9, 4, 6, 4, 5, 11.
That is **not** a convergence series, and it should not be read as one: the reviewed
denominator changed every round. In round 8 the pass that re-reviewed the already-covered
surface confirmed 0 of 2 findings, while the two passes on fresh axes confirmed 11 of 11.
The reviewed surface is locally saturated; the capability frontier is open. A zero-major
round under that instrument would be a sampling accident, not proof of correctness.

---

## 2. Root cause

The substrate has **two** flow representations, and the second one decides meaning.

`FunctionFlowGraph` (`crates/verter_semantic/src/analysis/flow/flow_graph.rs`) is a sparse
typed dependence graph built once per function content version from `FunctionBodySkeleton`
alone. Its edge vocabulary is four classes: `ValueDef`, `PathWrite`, `EvalEffect`,
`ControlRegion`. It is used for demand slicing — selecting *which* content a demand needs.

Meaning is then decided elsewhere. `flow_slice_content.rs` lowers a second, syntax-shaped
control tree (`SliceRegion`, `SliceStatement`, `SliceExpr`), and `FlowEvaluator`
(`flow_return.rs`) interprets it using parallel name-keyed maps and side sets — `locals`,
`declared_locals`, `var_locals`, `param_writes`, `narrowings`, plus assorted booleans.

That is precisely the structure the substrate's own contract forbids:

> The edge vocabulary is open by construction: further dependence classes (narrowing
> predicates, closure escapes, loop summaries, `try`/`finally` overrides) extend
> `FlowEdgeKind` on this SAME graph — **a second flow structure is forbidden**.
> — `flow_graph.rs:24-28`

and which the guard registry rejects by name:

> built on the shared **FlowFrame lattice** over the ONE `FunctionFlowGraph`'s
> narrowing-predicate edge class — never a second flow structure; **a merge algebra over
> the evaluator's `locals` / `var_locals` maps IS that second structure and is REJECTED AS
> OUT OF CONTRACT**.

`FlowFrame` as a lattice does not exist in the tree. The only symbol matching that name is
`FlowFramePop`, an unrelated recursion-stack enum.

### The five information losses

Each defect above traces to a dimension the current representation cannot express:

1. **Subject emptiness is used as a control verdict.** `GuardNarrowing::Impossible` collapses
   to a boolean that suppresses an entire arm. "The narrowed subject is `never`" and "this
   control edge is dead" are different facts; TypeScript keeps unrelated returns from a
   branch whose subject narrowed to `never`. → G1, G2.
2. **No nominal identity for `unique symbol`.** `SemanticNodeData` has `Primitive(Symbol)`
   and literals but no nominal unique-symbol leaf, so a provably disjoint pair reads as
   overlapping. Flow additionally owns a *second* local relation classifier
   (`nodes_provably_disjoint`) beside the shared relation authority. → G3.
3. **Effects are recognised by syntactic parent.** The IIFE capture-write scan is reachable
   only from the expression-statement arm; declarator initializers and `if` tests lower
   through different paths and never call it. Every new expression position is a new
   omission site. → G4, G5.
4. **A capture is a value snapshot, not a binding across a temporal frontier.** Nested
   evaluation clones the name maps at creation and starts with an empty narrowing overlay.
   There is no binding identity carrying declared authority, creation point, last reaching
   assignment, and an invalidating-write set. `push_nested_capture_reads`
   (`analysis/flow/mod.rs:1227-1258`) builds a full nested skeleton — **including its
   `writes` vector** — then harvests only free reads and discards the rest. → G6, G7, G8, G9.
5. **Completion is a boolean.** `statement_guarantees_current_function_return` is a
   hand-maintained match over return/block/if only, while the lowerer already models labels,
   switch, try/catch/finally, throw, and break with far richer semantics. There is no shared
   completion algebra, so labeled/`try`/`throw` fabricate a spurious `undefined`. → G10.

Plus a provenance hole feeding G11: a shallow-inference wildcard returns `any`, and an
unmodelled rootless leaf publishes it as a real value. A genuine authored `any` and "we have
no model" are indistinguishable downstream.

### An executable check of what the structures already carry

A spike built real skeletons from the failing programs (a control confirmed the instrument
discriminates, so the negatives are genuine absences):

| fact | present in skeleton? | consequence |
|---|---|---|
| labeled / `try` / `finally` regions | **yes** | G10 is evaluator-only |
| `if`-test retained as `control_input` | **yes** | G1/G2 are evaluator-only |
| IIFE call site in a declarator initializer | **yes** | G4/G5 are evaluator-only |
| nested-closure **reads** of a capture | **yes** | the R7 mechanism |
| nested-closure **writes** to a capture | **no** — computed, then discarded | the one real producer gap |

So four of the five fact classes are already indexed and simply not consumed: those
functions re-derive from syntax what the skeleton already holds. Exactly one producer change
is required — retain the nested write/capture-escape footprint that is already being
computed.

---

## 3. Ownership — most of this is not this block's scope

`native-flow-return.md` assigns the missing edge classes explicitly:

> This block lands the graph + the value-def / path-write / eval-effect / control-region edge
> classes; **the narrowing-predicate edges land across the `U6.NARROW_*` narrowing
> sub-blocks (collectively — one shared `FlowFrame` lattice)** and **the closure-escape /
> loop-summary / `try`/`finally`-override edges in `U6.LOOP_CLOSURE`** (each ADDS its edge
> class to this same graph, never a second structure).

The block registry records the state:

| block | status |
|---|---|
| `U6FlowReturnSubstrate` | LandingUnverified |
| `U6LoopClosure` | **Pending** |
| `U6Narrow{Typeof,Equality,Truthiness,In,Instanceof,Discriminated,Substitution,Invalidation}` | **Pending** |
| `U6ValueInference` | **Pending** |

Mapping the gaps onto their owners:

| gaps | owner | vehicle |
|---|---|---|
| G1, G2 | `U6.NARROW_*` | narrowing-predicate edges + the shared `FlowFrame` lattice |
| G3 | shared relation authority | nominal `unique symbol` identity + tri-state comparability |
| G4, G5 | `U6.LOOP_CLOSURE` | closure-escape edges + one position-independent effect transfer |
| G6–G9 | `U6.LOOP_CLOSURE` | closure-escape edges + capture summaries |
| G10 | D6 / `U6.LOOP_CLOSURE`; A3 has no G10 obligation under AMD-004 | debt `FR-D8`: demanded completion reduction, completion edges, root coverage, and final clean semantics on the sole `FunctionFlowGraph`; no syntax-only fallback or second classifier |
| G11 (`this`) | `U6.FLOW_RETURN_SUBSTRATE` | content-free `this` facts + `This`/`Capture` IR carriers |
| G11 (sequence) | `U6.FLOW_RETURN_SUBSTRATE` | a pass-through disposition in the shared value classifier |
| `ClassExpression` | recorded ledger entry | executable-region kind for field initializers |

### Recorded completion debt

| debt ID | disposition | debt | durable owner | resolution gate | acceptance ID |
|---|---|---|---|---|---|
| `FR-D8` | `DEFER` under `AMD-004` | Exact structural completion and G10 discrimination; the current producer can still publish the G10 wrong-and-warm result. | D6 / `U6.LOOP_CLOSURE` | Must close before D6 enters `REVIEW`. Heavy implementation may begin only after the D6 lock contains a closed, code-first carrier inventory. The demanded `FunctionFlowGraph` must be the sole completion reducer; G10 must match the pinned checker, X05/X68/X80/X88 must remain checker-correct clean/warm, and no syntax-only classifier or second completion authority may exist. | `d6_structural_completion_closes_g10_without_false_refusals` |

The `this` half of G11 is already specified — `u6-flow-call-resolution-design.md` §C6 states
that this block owes "the content-free `this` source/identity/locator facts plus the
`This`/`Capture` carriers", with a typed `ReturnOnly` for a non-reconstructible capture and
an explicit prohibition on guessing `any`.

**The conclusion that matters:** this block should not implement three unlanded blocks'
semantics procedurally. Where it cannot answer within its own scope, it must return typed
degradation and stay cold. Two recorded ledger entries already carry the gate "the U6 lane
may not close with this entry open", owned by that same lattice work, with acceptance tests
pre-written.

---

## 4. Target design

One graph. One lattice. One relation authority. Typed refusal everywhere else.

### 4.1 Extend the graph, never add a second one

Add the edge classes the contract already reserves by name: `NarrowingPredicate`,
`ClosureCapture`, `ClosureEscape`, `LoopSummary`, `TryFinallyOverride`. The demand planner
continues to do reachability over the same graph; no new CFG appears.

### 4.2 Return collection is structural — reachability never gates it

This is the single most important correction in this document, and it removes machinery
rather than adding it.

**Control-flow reachability does not decide which `return` statements contribute.** Returns
are collected *structurally* — every return statement in the body — and their expression
types are unioned. Flow analysis is used for two separate and narrower jobs: the **type of
each return expression** at its own program point, and **whether the endpoint is reachable**
(which adds `undefined`).

Measured against the pinned checker:

| probe | tsc | what it proves |
|---|---|---|
| `function f() { return "a" as const; return "b" as const }` | `"a" \| "b"` | a statically unreachable return **still contributes** |
| `function f(x: boolean) { if (x) { return "a" as const } }` | `"a" \| undefined` | endpoint reachability is a **separate** question |
| `function f(x: string) { if (typeof x === "number") { return x } return "live" as const }` | `"live"` | the return **does** contribute; its expression narrowed to `never`, and `never` is union-neutral |

The third row is the exact mechanism behind G1 and G2. Today the substrate deletes the whole
branch, losing an unrelated `"dead" as const`. TypeScript keeps the branch; the union simply
loses the arm when — and only when — the returned *expression* is itself `never`.

So the rule is:

```
inferred = union(type_of(expr) for every return statement, structurally)
           ∪ (endpoint_reachable ? {undefined} : {})
```

Correctness follows from `never` being the union identity, not from any reachability gate.
G1/G2 become unspellable because there is no longer a mechanism that can drop a return: an
empty subject domain can only ever produce a `never`-typed *expression*, which the union
absorbs on its own.

This supersedes the dual-`Execution`/`ReturnInference` edge-domain design considered earlier.
That design reached the right answers, but it re-introduced reachability as a semantic input
and then needed a second domain to undo the damage. Structural collection needs neither.

### 4.3 One completion algebra

The sole `FunctionFlowGraph` resolves source-ordered completion events for normal
continuation, return, throw, labeled/unlabeled break, and labeled/unlabeled continue to
dense control identities and direct edges. `try`/`catch`/`finally`, labels, switches, and
loops are represented in that same topology. Delete `statement_guarantees_current_function_return`,
the pending-break booleans, target-indexed completion sets, and the per-syntax suffix flags.
G10's three cases and their unsampled siblings (`switch`, `catch`) close together.

Revision 11 staging under AMD-004: exact structural completion and G10 discrimination
are deferred from the A2–A6 critical path and recorded as debt `FR-D8`, owned by D6 /
`U6.LOOP_CLOSURE`. A3 retracts only non-G10 wrong-complete results through typed
degradation and existing non-admission rails. It has no syntax-only G10 detector and
must preserve checker-correct clean/warm cases, including X05, X68, X80, and X88.
When completion work resumes, the skeleton remains content-free topology, the demanded
`FunctionFlowGraph` is the sole completion reducer, and no second graph or completion
classifier is permitted.

### 4.4 Bindings as lattice state, not name maps

A `FlowFrame` keyed by binding identity, joined at graph joins, iterated to a fixed point on
loops:

```
SlotState { declared, reaching, narrowed, freshness, exactness }
```

`declared` carries authority for both annotated **and inferred** declarations — G8 exists
because only annotated declarators are recovered today.

### 4.5 Closures as summaries over a temporal frontier

```
CaptureEdge   { closure, outer_slot, access: Read | Write, depth }
ClosureSummary{ direct_reads, direct_writes, transitive_reads, transitive_writes, escapes }
```

Computed to a fixed point across sibling and nested closures. A capture seed is the
creation-point `SlotState`; a pre-creation narrowing survives only when the invalidating
write set is provably empty, which is exactly TypeScript's preserved-narrowing rule. Invoking
a closure applies the same summary **in every expression position**, so there is no
declarator/test/statement sibling hook to forget. G4–G9 close as one mechanism.

This requires the one producer change: harvest `nested.writes` alongside `nested.reads` in
`push_nested_capture_reads` and emit capture/escape edges from it.

### 4.6 One relation authority

Add nominal `unique symbol` identity to the semantic value algebra and make overlap a
tri-state (`ProvenEmpty | ProvenOverlap | Unknown`) decision owned by the shared relation
query. Delete flow's private `nodes_provably_disjoint`. This restores the single-resolver
rule; a flow-local type relation is a second engine by another name.

### 4.7 Make "complete" unforgeable

The defect class is *not* "a wrong type". It is "a result with unmodelled content published
as complete and cached". Close it with a typestate rather than a convention:

```
FlowSolveOutcome = Complete(CompleteFlowReturn) | Partial(PartialFlowReturn) | NoValue(..)
CompleteFlowReturn { result, proof: FlowCoverageProof }   // private constructor
```

`FlowCoverageProof` is minted only after the worklist drained every selected block and edge,
every value op returned `Exact`, every completion/capture/effect obligation was consumed, and
the fixed point converged. **The cache admission API accepts only `CompleteFlowReturn`.**
Unmodelled syntax yields `FlowGap { site, reason }`, and there is no conversion from
`FlowGap` to a semantic node — so `SliceExpr::Any` disappears and a fabricated `any` becomes
unspellable rather than merely fixed.

Partial results stay useful to consumers; they are simply type-ineligible for warm admission.

---

## 5. Why we do not copy tsc's algorithm

TypeScript's checker binds flow nodes and walks **backward** from each reference through
antecedents, caching per flow node. Mirroring that algorithm is available and is **not** the
right choice here.

**The specification is tsc's observable semantics, not tsc's implementation.** We must return
what tsc returns on the supported surface. Nothing requires reaching that answer the way tsc
reaches it, and tsc's algorithm is shaped by constraints Verter does not share.

Where the proposed design is genuinely better:

- **Reuse.** tsc's backward walk is per-reference; its caches are per-compilation and die with
  the program. Verter computes a forward summary once per function **content version**, stored
  content-addressed and demand-sliced, so N demands over the same function reuse one fixed
  point. tsc has no equivalent because it has no content-addressed artifact store.
- **Demand slicing.** Verter answers member-projected demands (`f().a.b`) by graph
  reachability without materialising the rest of the surface. tsc has no demand concept; it
  computes the whole flow type of the reference.
- **Honesty about coverage.** tsc has no "I do not model this" state — unhandled shapes
  silently become `any`. The coverage-proof typestate gives Verter a state tsc cannot express:
  typed refusal that is structurally barred from warming a cache.
- **Precision where tsc bails for latency.** tsc applies depth/size cutoffs mid-check and
  reports "excessively deep" rather than answering. Because Verter's fixed point is computed
  once and cached, it can afford to converge where tsc gives up — and those are cases where
  tsc *errors* rather than *answers*, so being better there is not divergence.

**The constraint that bounds "better":** on the supported surface, the published type must
equal tsc's. Being *more precise* than tsc is a divergence bug, not an improvement — the user's
editor shows tsc's answer, and a Verter type that disagrees is wrong even when it is more
sound. Two known cases where tsc is deliberately unsound (it does not invalidate narrowings
across arbitrary calls, and it applies specific closure rules rather than a general effect
analysis) must be **matched, not corrected**. Where a closure summary proves more than tsc
uses, the extra precision is used to decide *whether we may answer at all*, never to publish a
narrower type than tsc would.

So: implement tsc's semantics, with a reusable cached fixed point instead of a per-reference
backward walk, and a typed refusal state tsc does not have.

**A porting caveat, recorded because it was nearly missed.** An algorithm-level port was
seriously considered and produced one correction worth keeping regardless (§4.2: returns are
structural, not reachability-gated). The lesson generalises: *tsc's observable behaviour is
authority; anyone's summary of tsc's behaviour — including this document — is not.* Before
implementing any rule here, probe the pinned checker directly and pin the result as a corpus
row with a discriminating assertion. Each of the three §4.2 rows exists because a plausible
mental model of the semantics was wrong in a way only measurement exposed.

---

## 6. Work items, in order

Each item states its owner and how it is proven. None may publish a warm result until its
acceptance test passes cold **and** warm.

1. **`U6.FLOW_RETURN_SUBSTRATE` (this block) — retract to scope.** Return typed degradation,
   cold, for closure capture, guard narrowing, and abrupt completion beyond this block's
   surface, instead of a procedural approximation. Land the `this`/`Capture` carriers and the
   sequence pass-through disposition that this block genuinely owns. *Proof:* every gap in §1
   not owned here returns typed degradation and is refused warm admission; no case in §1
   publishes `degradation=None`.
2. **Relation authority — nominal `unique symbol` + tri-state comparability.** Delete flow's
   private classifier. *Proof:* G3 matches the checker; the negative control (structurally
   overlapping intersection) still survives.
3. **`U6.NARROW_*` — narrowing-predicate edges + the shared `FlowFrame` lattice.** Structural
   return collection (§4.2) lands here, together with the removal of every reachability gate
   on return contribution. *Proof:* G1, G2, and the two recorded ledger entries
   (conditional-`var` branch join; unapplied write-effect source order) all publish clean and
   warm-admissible; the three §4.2 probes are pinned as rows.
4. **`U6.LOOP_CLOSURE` — closure-escape, loop-summary, `try`/`finally`-override edges.**
   Capture summaries (§4.5), the completion algebra (§4.3), and the producer change to
   `push_nested_capture_reads`. *Proof:* G4–G10 match the checker, and the position matrix
   below is uniform.
5. **Coverage-proof typestate + `FlowGap`.** Delete `SliceExpr::Any`. *Proof:* a compile-fail
   test showing neither a caller nor the cache can construct `CompleteFlowReturn` without the
   private proof; a mutation of one value op to `FlowGap` yields `Partial` and leaves no memo
   candidate on replay.
6. **Delete the superseded paths in the same change** — the second control tree, the
   position-specific IIFE scanners, the suffix-return classifier, the snapshot capture path,
   and the flow-local relation classifier. No compatibility path, no shadow evaluator.

---

## 7. The corpus harness must be strengthened first

**Status: the strengthening below is LANDED** —
`crates/verter_session/src/u6_flow_expect_tests.rs` provides the recursive `ExpectedNode`
expectation (signatures with exact ARITY and ordered parameter types, exact literal
string/number values, intersections in source order, order-insensitive EXACT union
constituent sets, distinct `TypeParam` / `DeclRef` / `BareRef` identities), the
public-boundary companion (`Boundary::Audit` / `Boundary::AuditRefusal`:
`get_flow_return_type_with_audit` invoked twice with BOTH calls modelled — result class,
typed `degradation`, exact projected JSON, `from_cache`, cold-compute count; a cold replay
must genuinely recompute, a refusal pins its full typed `FlowReturnError` identity —
compared on BOTH calls, so a refusal that changes kind across calls or is swapped for a
different typed refusal fails — and a refusal is never admitted warm), a typed
checker-syntax projection that parses each deep-pinned row's `checker` column and compares
it SEMANTICALLY against the live graph verdict-directed (a `MatchesChecker` row must equal
it; a `KnownOwed`/`Degraded` row must NOT — so `RENDER_INCOMPARABLE` exempts presentation
bytes only, and a bogus reclassification fails the verified byte-divergence check),
negative controls for every RETAINED comparison clause (unexercised vocabulary was
deleted only under an exhaustive DIRECTIONAL argument — accept-arm removals that
fall to the controlled `_ => false`: `Lit::Bool` / `Lit::BigInt`; alias transparency
in the matcher; the checker-syntax `Ref` ↔ `BareRef`/`TypeParam` acceptance arms.
The round-3 deletion of the `SignatureKind::Call` discriminant was made on a FALSE
sample-probe claim — the annotation-typed parameter form `x: new () => Box` DOES
reach a `Construct` signature on this rail — and is RESTORED in both comparators,
controlled live in both directions), the crossed capture-write position
matrix, and oracle/profile stamps on every assertion. The five rows named below carry
recursive + boundary pins; `D01_helper_new` additionally carries the real-corpus
`ReturnOnly` boundary pin (`warm_replay: false`) and
`X83_sequence_wrapped_iife_effect_fails_closed` the real-corpus refusal pin
(`Boundary::AuditRefusal` carrying the typed
`Failure(Unsupported(InvokedClosureEffect))` identity) — the typed non-admission
contract exercised on real rows.
Four of the five named rows measure deep-equal to the checker;
`N25_impossible_predicate_statement_keeps_dead_contributor` measured DIVERGENT
(`v: A | B | "ok" | "no"` where the checker computes `"no" | "ok"`, wrong-and-warm) and is
re-pinned as a `KnownOwed` expected-versus-actual gap against `U6.NARROW_LATTICE`. The
matrix records the live position-dependence of the invoked-IIFE capture-write (statement /
sequence / call-argument positions refuse; declarator-init / if-test / template /
short-circuit / object-literal positions publish the stale pre-write value clean and warm)
as pinned per-position outcomes that fail on ANY position-local movement.

The original finding, kept for the record — five rows added in the last round did not
discriminate their parent tree and therefore characterized nothing:

- `X85_nested_closure_write_updates_captured_binding`, `X87_read_only_let_capture_keeps_reaching_literal`
  — assert only root `NodeShape::Other` + `degradation: None` + `candidates: 1`, which cannot
  distinguish `() => "a"` from `() => "b"` — exactly the distinction G6/G7 turn on.
- `X88_nested_label_inherits_enclosing_suffix_return`, `N25_impossible_predicate_statement_keeps_dead_contributor`
  (named `…omits_dead_contributor` in that round — renamed once measurement showed the dead
  contributor SURVIVES, so the id now describes the characterized behaviour),
  `N26_structurally_possible_predicate_intersection_survives` — same under root `NodeShape::Union`.

This violated the repository rule that a characterization test must fail against the
pre-change tree. Repairing it needed harness surgery, which was authorized and delivered:

- a recursive `ExpectedNode` expectation able to assert signatures, exact literal/primitive
  values, intersections, and order-insensitive exact union constituents;
- graph-level identity preserved where projection would conflate `TypeParam`, `DeclRef`, and
  `BareRef`;
- a public-boundary companion through `get_flow_return_type_with_audit`, invoked twice,
  asserting exact projected JSON, `degradation`, and the second call's cache-replay state.

Each strengthened row must fail against the parent behaviour and pass after its owning change.

### The position matrix the closure work must satisfy

The recurring failure is a class fixed at one site and not its siblings, so closure work is
accepted only against a crossed matrix, not a case list: **binding kind** (`var`/`let`/`const`/
parameter/destructured/destructured-with-default/catch/loop) × **write timing** (before
creation / after creation / inside / sibling closure / deeper closure / never) × **closure
depth** × **expression position** (statement, declarator initializer, `if` test, sequence
operand, return expression, call argument, template, short-circuit operand, object/array
literal, default-parameter initializer, class field initializer) × **guard kind** ×
**completion container** (`if`/loop/`switch`/`try`/`catch`/`finally`/labeled).

Assert that the same capture-write edge appears for a given cell **regardless of expression
position** — that assertion is what makes a position-specific hook impossible to reintroduce.

---

## 8. Residual risks

- The design does not prove the semantics *inside* a supported operation. An algorithmic bug
  in an exact arm can still be wrong-and-warm; oracle coverage stays necessary.
- It does not complete contextual typing, `async`/generator/`await`, exception precision,
  aliasing through opaque calls, or cross-file effect summaries. Each must yield a `FlowGap`
  or conservative invalidation rather than a guess.
- A wrong *dependence edge* is still a graph-construction bug; demand-slice poison-sibling and
  effect-frontier tests remain essential.
- Retracting to typed refusal (§6.1) **reduces measured conformance** — rows that currently
  match by procedural approximation become parked. That is the intended direction: a parked row
  is honest, a warm wrong answer is not. It should be expected in the numbers rather than read
  as a regression.
- TypeScript version drift and genuine registered TypeScript bugs remain governed by the
  pinned-oracle policy, not by this design.
