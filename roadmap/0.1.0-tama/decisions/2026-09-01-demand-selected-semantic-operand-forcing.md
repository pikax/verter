# Demand-selected semantic operand forcing before D8

- Status: accepted
- Date: 2026-09-01
- Scope: adds the `rev11.type-evaluation` train, amends D8's predecessor and exact-input contract, and changes no implementation ledger, GitHub mapping, issue catalog, public/native/wire authority, or unrelated flow predecessor.

## Context and verified gap

The live type-resolution architecture is already demand-driven at its outer query and
projection boundaries, but selective operators still receive operands that have been
materialized too early. This makes the documented navigation and shallow-processing
principles weaker than their API boundary: the evaluator can avoid walking a losing
branch after selection, yet lowering has already performed semantic work to construct
that branch's `SemanticNodeId`.

The gap is directly visible in the current `origin/train/rev11-flow` source:

- `crates/verter_session/src/project_semantic_dispatch/lower.rs`, in the
  `TypeExpr::Conditional` lowering arm, lowers `true_type` through
  `lower_type_expr_with_infer_factory`, then lowers `false_type` through the same
  semantic lowerer, and only afterward calls
  `execute_type_node(SemanticQueryKey::Conditional { ... })`.
- `crates/verter_session/src/semantic_query.rs` defines
  `SemanticQueryKey::Conditional` with four already-materialized
  `SemanticNodeId` operands (`check`, `extends`, `true_branch`, `false_branch`) plus
  `distributive`. It carries no operand locator and no demand/context with which the
  winning branch could be chosen before branch forcing.
- Projection, indexed access, `keyof`, mapped types, and generic instantiation already
  have reusable semantic query families, but their entrances do not share one sealed
  operand/forcing capability. Some routes can still enumerate or materialize a broader
  surface before the narrower key/path demand is authoritative.

This is an authority-boundary defect, not grounds for a second type evaluator. The
existing `SemanticQueryKey` → `ProjectSemanticDispatch::execute` →
`SemanticGraphStore` graph is the sole query-time resolver and must remain so.

## Intent contract

The actor is the shared semantic query graph. The problem is that an operator which can
select a strict subset of its operands does not always receive those operands in a form
that can remain unforced until selection. The capability should exist so conditional,
projection, indexed/key-domain, mapped, and generic evaluation perform semantic work
only for the demanded result while preserving exact TypeScript semantics, provenance,
cache validation, cancellation, and bounded reuse.

Required observable outcomes:

1. A decided conditional forces only its selected branch. Infer bindings are visible
   only in the selected true branch. Existing conditional dispatch remains the owner of
   distributivity. An open conditional suspends and carries residual projection demand
   into both branches. A true full-root `Expanded` demand retains current semantics.
2. `ProjectPath`, `ProjectMember`, indexed access, and `keyof` determine the requested
   segment/key domain before unrelated base-surface work. Residual paths propagate
   without sibling enumeration. TE3 treats conditional shells and materialized branch
   carriers as opaque existing carriers; conditional selection and dead-branch forcing
   remain TE2 authority, and the combined behavior is closed only after TE4/TE5.
3. Mapped and generic evaluation substitutes and instantiates demanded keys only.
   Mapper classification never materializes the value body. Binder, default,
   constraint, remap, and substitution identity is exact.
4. Dead semantic operands add no semantic dependency facts and perform zero attributable
   semantic work: zero forcing attempts, locator dereferences, substitutions, nested
   semantic dispatches or relation reads, semantic allocations/origin writes, and
   semantic fact reads. Intersection selection may first perform at most one
   contribution-classification probe per potentially contributing arm per `(path
   segment, complete ProjectionReductionContext)` cold evaluation under the existing
   request budget; every probe's key/selection facts are traced, and identical warm
   demand adds zero probes. An undecidable arm remains open/partial; only after
   non-contribution is proven does that arm's member-value/body/deep work become dead and
   therefore zero. Parse and shallow indexing are excluded because they prepare the
   owner artifact rather than force an operand.
5. Reusable authored identity is content-free and complete. Version/freshness authority
   remains ordinary `ReadSetSignature` plus strict self-root validation. A same-owner edit
   in a dead operand may conservatively reject a warm candidate through that existing
   self-root contract; recomputation must still equal fresh and perform zero dead-operand
   semantic work. No finer-grained self-root architecture is introduced. Cancelled,
   budgeted, unstable, recursive, or partial work is `ReturnOnly`. Per-family retention
   is bounded, and repeated warm demand does not grow candidates.

Forbidden outcomes:

- a second semantic recipe graph, resolver, relation authority, distributivity planner,
  recursive demand walker, request-local memo, or native-checker implementation;
- closures, function pointers, trait-object evaluators, AST/OXC references, stored
  `TypeExpr` recipes, arbitrary environment maps, source text, spans, content hashes,
  independently supplied allocation ordinals/ranking, or graph-allocation order as
  durable authored identity;
- public `TypeInfo`, wire, native-checker, flow, relation, truthiness, canonical-algebra,
  component-meta publication, or display expansion merely to host the capability;
- partial/cancelled/budgeted work admitted warm, nominal mode/rank used as satisfaction,
  or unbounded candidate growth.

## Decision

Add the five-node `rev11.type-evaluation` train. Its sole owner is:

> the sole demand-selected semantic operand forcing authority inside the existing
> SemanticQuery graph

The train is an authority refinement of that graph, not a new evaluation system.

### TE1 — sealed operands and one forcing capability

An operand is a closed choice between:

- an already-materialized `SemanticNodeId`, used only as a store/generation-local
  runtime handle in existing node-keyed family identity, never persisted, compared
  cross-store/generation, or treated as durable authored identity; or
- a content-free authored locator plus the exact sealed lexical environment,
  substitution, binder, split-env, and any non-`ProjectionReductionContext` identity
  that is truly authored operand meaning.

The force request, not the operand, owns the one complete existing
`ProjectionReductionContext`: `(mode, demand, provenance, merge_role,
vue_heritage_policy)`. One capability-limited boundary, semantically
`force(operand, request_context)`, combines operand identity with that unchanged request
context exactly once in existing query dispatch/family identity. It never reconstructs
or defaults over any context axis, duplicates it, or stores a second context. One-axis
tests distinguish all five request axes, including `merge_role`. It then composes only
existing `LowerLocator`, `Instantiate`,
projection, conditional, relation, and canonical-algebra queries through
`ProjectSemanticDispatch`. It is a closed API, not a generic trait. It holds no closure,
AST pointer, `TypeExpr`, recipe program, or arbitrary environment map. A materialized
node force merges that node's provenance edges, read-set fragments, and self roots into
the candidate being built. One cold force may directly dereference the selected
operand's own locator at most once; transitively demanded operands and subqueries are
charged and bounded independently per semantic key.

### TE2 — conditional selection before branch forcing

Conditional keys carry sealed operands while the force request carries the one complete
existing five-axis `ProjectionReductionContext`. The existing
`conditional_branch_selection`/`SemanticQueryKey::Relate` authority selects before
branch force. Decided selection forces one branch; infer binds only into the selected
true operand. Open selection preserves a conditional carrier and pushes a residual path
into both operands. Existing conditional dispatch owns distributivity. Query-root open
`Expanded` continues to force both branches because both are the requested full result.

### TE3 — projection and key-domain selection

`ProjectPath` remains canonical, with `ProjectMember`/`IndexedAccess` as sugar.
Key/index demand becomes authoritative before unrelated base-surface forcing.
Intermediate path hops use `Navigate`; only the terminal uses the caller mode.
Intersection projection may perform at most one contribution-classification probe per
potentially contributing arm per `(path segment, complete ProjectionReductionContext)`
cold evaluation under the existing request budget. Every probe's selection/key facts
are fact-traced, and identical warm demand adds zero probes. If contribution is
undecidable, the result preserves an open/partial carrier. Once an arm is proven
non-contributing, its member-value/body/deep forcing is zero. `keyof` forces
key-producing structure rather than member values. Conditional shells and materialized
branches use existing opaque carrier behavior and may preserve a residual path; TE3
does not select a conditional branch or own dead-branch semantics. That belongs to TE2,
with combined behavior proved after TE4/TE5.

### TE4 — mapped and generic demanded-key evaluation

Mapped/generic evaluation resolves a demanded key domain and then substitutes/forces
only demanded values. A single-key projection does not enumerate the mapped surface.
Remap-dropped keys and other dead operands are not substituted or instantiated. Mapper
classification is structural and performs no semantic dispatch. Exact binder/default/
constraint/substitution identity participates in the existing query families. The
route/mode-independent open-key-domain carrier-stop remains authoritative.

### TE5 — convergence and bypass deletion

TE5 is one implementation authority-closure/cutover, not a convergence-owned cache
project. TE1–TE4 own family-specific identity, recorded materialized-point
`cached_satisfies`, read-set/self-root validation, cancellation/admission, and retention
work. TE5 performs the bounded structural route switch, deletes or rejects every
residual eager selective-operator route outside the one force capability, and enforces
those landed contracts across families. Structural closed enums, capabilities, and
exhaustive dispatch are the primary enforcement; a name-keyed scanner is not the
authority proof, and TE5 creates no second cache, admission rail, or retention policy.

## Topology and corrected placement

The accepted edges are:

```text
B6, C1 -----------------> TE1
TE1, D3C, TA1B ---------> TE2
TE1, TA1B --------------> TE3
TE2, TE3 ---------------> TE4
TE4 --------------------> TE5
D4, D5, D6, D7, TE5 ---> D8
D8 ---------------------> G1 ---> G2
D8 ---------------------> E2
```

The placement before D8 is load-bearing. D8 certifies U6 convergence and
complete-result admission. Landing selective forcing after D8 would change which facts
are read, which partial/cancellation paths may publish, which operands participate in
flow/query completion, and which candidates satisfy a demand after D8 had certified
the old behavior. Therefore TE5 is a **direct D8 predecessor** and D8's exact
predecessor contract names TE5's one-authority/admission result.

No direct TE edge is added to G1, E2, G2, or a native-checker node. They inherit TE5
transitively through D8. A redundant direct edge would falsely suggest public,
query-runtime, or checker ownership and would make later boundary review harder.

TE2 keeps D3C as the architect-mandated predecessor because D3C is strictly the
ledger-visible completion/ordering fence for the atomic D3R/D3I/D3P/D3C landing. TE2
continues to consume the pre-existing shared `SemanticQueryKey::Relate`,
`InferenceSession`, and `InferBinderId` authorities. It consumes no D3R nominal
`Identity`/`Comparable` outcome, no D3I `FlowBinding` identity, and no D3P/D3C flow,
product, worklist, admission, or budget API, including `FlowProductStore`, `FlowReturn`,
and `FlowDischargeReport`.
TE2 and TE3 depend on TA1B where their derived composites must use the sealed canonical
algebra. TE4 joins the conditional and projection/key-domain halves before TE5's
bounded structural closure.

## Why the rejected alternatives are wrong

### A second recipe graph

A `SemanticRecipeId` or operand recipe DAG would duplicate the existing semantic query
graph's identity, cycle handling, singleflight, fact validation, origin, cancellation,
budgets, and retention. The two graphs would need a cross-graph coherence protocol and
could disagree on which work is live. The authored locator already provides a sealed
address for unforced syntax; the existing queries already provide the reusable work
units. A second graph has no independent semantic authority to own.

### A new relation or distributivity authority

Branch selection and infer binding are relation judgements. Reimplementing a cheap
assignability predicate or moving distributivity into the forcing layer would allow
predicate-time and build-time conditional semantics to diverge. The forcing layer asks
the existing conditional/relation authority which operands are live and performs only
that demanded work.

### Closures, AST pointers, `TypeExpr`, or arbitrary environments

Those representations are not durable, content-free, safely shareable query identity.
They can capture stale content, parser arenas, request-local state, or unqualified name
maps. A store/generation-local materialized node handle in an existing node-keyed
family, or a typed authored locator plus sealed exact authored identity, is the only
admitted operand representation. The node handle is not a persisted or cross-store
identity and must contribute its provenance/read-set roots to its consumer.

### A generic recursive demand walker

Navigation is already defined as a thin, non-owning path walker. Giving a new walker
recursive declaration resolution, instantiation, relation, or cache writes would make
it a second resolver. Every new semantic node continues to enter the shared query API.

## Cache, cancellation, and proof consequences

- Authored keys carry semantic identity and split env dimensions only. Existing
  node-keyed families may use a store/generation-local `SemanticNodeId` handle, while
  values carry `ReadSetSignature.facts` and self roots and revalidate on every warm hit.
- Candidate satisfaction is based on recorded work, not requested-label rank. A narrow
  result never claims a whole surface; a whole result backfills only exact points it
  materialized.
- Cancellation/budget checks run before each force and before admission. Degraded work
  is `ReturnOnly`, publishes no candidate/origin/reverse index, and cannot release a
  deferred member batch as though complete.
- Per-family caps and existing eviction semantics remain the bound. The train adds no
  operand cache or second in-flight table.
- A same-owner dead edit may conservatively reject through ordinary strict self-root
  validation. Such rejection does not license dead dependency facts: recomputation must
  match fresh and still perform zero dead-operand semantic work.
- Proof uses durable semantic-work counters. Parse/shallow indexing is intentionally
  outside the dead-operand claim; otherwise an implementation could falsely fail merely
  because the owner file was indexed once, or falsely pass by moving semantic work into
  an uncounted helper.

## Sizing and closure classification

TE1–TE4 and TE5 each target at most 800 production LOC and 8 production files. Each
requires mandatory rescope above 1,500 production LOC, 12 files, or 3 unrelated
crates/packages; the 12-file threshold is not advance permission to exceed the 8-file
target. TE5 is `kind=implementation`, `semantic_role=delivery`, while retaining the
`foundational-authority-closure` class and its accepted convergence-and-bypass-deletion
name. Its scope is the route switch, bypass deletion, and cross-family structural
enforcement; family-specific cache/admission changes remain in TE1–TE4.

## Consequences and non-goals

- D8 is not READY until TE5 has an implemented-ledger row in addition to D4–D7.
- G1, E2, and G2 inherit the new ordering without charter amendments because their
  existing direct D8 contract is sufficient.
- The train may make breaking internal API changes. It may not preserve old and new
  forcing paths in parallel.
- No implementation-ledger rows, GitHub issue mappings/content, commits, PRs, or
  external mutations are part of this authority-definition decision.
- D2B's current recorded predecessor inconsistency involving TA1B/TA2 is explicitly
  outside scope and remains untouched.
