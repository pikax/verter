<!-- unified-charter-v2
id=TA1A
name=Canonical algebra comparator builder and mint substrate
phase=rev11
train=rev11.type-algebra
product=rev11
kind=implementation
semantic_role=delivery
class=foundational-authority-substrate
predecessors=
owner=rev11.type-algebra:the single canonical semantic algebra authority
conflict_domains=semantic_authority
resource_class=rust-mixed
review_profile=semantic-3
gate_profile=targeted-domain
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=high
verification_effort_default=high
confirmation_effort_min=high
confirmation_effort_default=high
size=L
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/rev11-type-algebra/TA1A.md
max_production_loc=1200
max_production_files=16
max_related_packages=2
rescope_loc=1600
rescope_files=18
rescope_unrelated_packages=3
-->

# TA1A — Canonical algebra comparator builder and mint substrate

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

TA1A and TA1B together replace the single node "Canonical union and intersection
algebra closure" (former TA1). A preflight inventory against former TA1's scope
measured ~24 production files / ~1,100–1,400 LOC against its 1,500 LOC / 18 file
budget: LOC fit, files did not. The cause is structural, not incidental — the
block's own enforcement mechanism flips `SemanticNodeData::{Union, Intersection}`
to an opaque payload, and that flip alone forces a token edit at every authored,
ordered and rebuilder construction site in the same compile. The remedy is to split
**by mechanism**, never by "already-published subset" (a subset split would leave
non-published constructors outside the authority). TA1A is the half that can be
built, proved, and reviewed WITHOUT flipping the payload representation; TA1B is the
half whose entire job IS the flip plus the exhaustive site closure it forces.

Per the ruling (`decisions/2026-08-31-canonical-type-algebra-predecessor.md`), "**The
authority exists; the defect is enforcement.** `ProjectSemanticDispatch` owns the
live `NormalizeUnion` / `NormalizeIntersection` queries, assigned to
`U2.QUERY_VALUE_DOMAIN`, which `U6.FLOW_RETURN_SUBSTRATE` already declares as a
prerequisite. Construction is not closed over it: the official union builder applies
`X | never = X` through `absorb_union`, but the lower-level
`intern_normalized_union_or_intersection` applies only one level of union
flattening, limited literal subsumption, and a sort-and-dedup by raw arena ordinal,
while intersections are interned as given. The flow-return join and path walking
both construct through that lower-level path directly." TA1A and TA1B together are
the separately reviewable unit `U2.CANONICAL_TYPE_ALGEBRA`, "a sub-block of
`U2.QUERY_VALUE_DOMAIN` — not a second normalization system." Neither is a new
normalization engine, a parallel normalization train, nor a general native-checker
or type-system rewrite; scope "is bounded to satisfying the existing canonical-node
contract and A–D."

TA1A owns the substrate: the iterative, cycle-safe, scope-insensitive structural
comparator (`Equal | Distinct | Incomplete`); the canonical union and intersection
builders (recursive flattening, structural `T | T = T`, lattice absorption, and
proven-disjoint scalar collapse — only PROVEN disjointness, never a guessed
relation); the `CanonicalMint` that is the sole place these builders are invoked
from; the freshness plumbing (origin edges, self-roots for every file-scoped node
the identity walk inspected INCLUDING discarded duplicates, descent through
`Global` intermediates, `Incomplete` ⇒ `ReturnOnly`); the rewrite of the
`intern_normalized_union_or_intersection` interning funnel onto the new comparator
and builders; and routing of every construction join inside
`ProjectSemanticDispatch`'s own live dispatch context — concretely, the
`NormalizeUnion` / `NormalizeIntersection` query execution, the official union
builder's `absorb_union` path, and the flow-return join and path-walking direct
construction the ruling names as the measured defect — onto that substrate.

TA1A deliberately does **not** flip `SemanticNodeData::Union` /
`SemanticNodeData::Intersection` to an opaque payload: the `CanonicalMint` still
produces today's transparent variant shape. It also does not attempt the
repo-wide exhaustive construction-site migration, the carrier-category allowlist, or
the compile-fail bypass proof — those are TA1B's closure job, forced by the flip
TA1B performs. TA1A's own footprint is the comparator, builders, mint, freshness
plumbing, the interning funnel, and the construction joins REACHABLE FROM DISPATCH —
not every one of the ~34 production construction sites the ruling's reconnaissance
measured. The dispatch-reachable set is wider than the funnel alone: it includes the
path-walking and projection joins, the mapped / keyof / utility reducers, relation and
call widening, object-spread projections, and the synthesized closed unions that reach
component-meta or typeinfo publication. Those sites route here because they are live
dispatch joins that produce findings-class results today; what remains for TA1B is the
payload flip and the exhaustive closure of every REMAINING construction site under the
carrier-category registry, which the flip forces the compiler to enumerate.

The node is accepted when its four owned obligations hold together: (1) the
comparator is exhaustive over `SemanticNodeData`'s manual equality, ignores only the
arena sidecar scope, is iterative and cycle-safe, and returns
`Equal | Distinct | Incomplete` with `Incomplete` preserving both arms; (2) the
canonical builders enforce at least flattening, `T | T = T`, `T | never = T`, and
provably impossible primitive intersections collapse to `never`, with an undecided
relation never guessed; (3) the freshness plumbing records the ruling's self-root and
origin-edge evidence and refuses to warm an `Incomplete` or depth-budgeted result;
and (4) the three findings the ruling measured through the live public boundary — a
`never` contributor surviving a union (finding A), a duplicate union constituent
surviving a `switch` join (finding B), and an unreduced provably-conflicting
intersection (finding C) — publish correct and warm once routed through this
substrate via the interning funnel and the live dispatch join.

The layering this node owns is the ruling's **canonical semantic types** layer: it
owns "universal algebraic laws and reusable facts: `T | never = T`, `T | T = T`,
flattening, provably disjoint scalar intersections to `never`, truthiness-domain
classification" (truthiness-domain classification is TA2's node; this node supplies
only the union/intersection half) and may assume "raw evidence is complete. It may
preserve source carriers through origin sidecars, but may not retain redundant
algebra members merely to preserve provenance." It does NOT own the raw-flow-evidence
layer, the TypeScript-compatible-inference layer, or the display layer. Per the
ruling: "Type-impossible contributors stay in raw evidence with provenance:
algebraic `never` does not erase evidence, and evidence does not force `never` to
remain a union constituent." Finding A's widening of `"a"` to `string` belongs to TS
inference, not to this node. This charter accepts one authority-substrate boundary;
it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_semantic/src`, `crates/verter_session/src`.
- Named API/data boundaries: the new comparator and `CanonicalMint` types this node
  introduces; the live `SemanticQueryKey::NormalizeUnion` /
  `SemanticQueryKey::NormalizeIntersection` queries and their `ProjectSemanticDispatch`
  execution; the official union builder and its `absorb_union` law application; the
  `intern_normalized_union_or_intersection` interning path (rewritten as the funnel
  onto the new substrate); `SemanticNodeData` and its manual equality rules, read
  ONLY — this node does not change the enum's representation; the node arena's
  sharded dedup indexes and the `NodeArena::node_scope(id)` sidecar; `NodeScopeId`
  and the `Global` scope of a derived composite; `ReadSetSignature` / observed
  self-roots / origin edges; and `structural_hash_of` as a PREHASH ONLY.
- Mutation boundary: only the production surfaces and named API/data boundaries
  above; every changed path must be inside both that charter surface and the
  acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **None.** This node is foundational: nothing existing gates it, and it is
  independent of TA2 so the two are reviewable in parallel. It is the sole explicit
  predecessor of TA1B, and — transitively through TA1B — of D2B.
- **External requirements:** agents check any listed requirement; tooling does not
  validate external state.

## Source-specific scope

- **Identity mechanism (verbatim constraint).** "'Scope-insensitive' means ignoring
  the ARENA SIDECAR scope (`NodeArena::node_scope(id)`) during comparison. It does
  NOT mean ignoring scope-bearing semantic payload fields — `BareRef` scope,
  declaration identity, value roots, infer-binder identity all remain identity. The
  canonical authority is a dedicated, exhaustive structural comparator whose
  identity is `SemanticNodeData`'s manual equality rules, recursively replacing
  child ordinals with child structural identity, ignoring only that sidecar scope.
  It returns `Equal | Distinct | Incomplete`; `Incomplete` preserves both arms and
  suppresses canonical warm admission."
- **`structural_hash_of` is not the authority.** "It may serve as a prehash for
  candidate narrowing and primary ordering only, and a hash match must be followed
  by exact cycle-safe comparison; collisions take a deterministic structural
  tie-break and never deduplicate." It "diverges from arena equality materially: it
  includes `TypeParam.display_name` which arena equality excludes, contributes only
  a variant tag for `DeferredCallable`, omits identity-bearing `SurfaceMember`
  fields such as `has_implementation_body` and `excess_origin`, and its depth and
  unresolved-child sentinels deliberately permit distinct structures to share a
  fingerprint. Its byte contract is not to be changed to serve this block; shared
  traversal plumbing may be extracted, but canonical identity and audit
  fingerprinting remain distinct policies."
- **Memoization bound.** "caching a completed ROOT fingerprint for `(store identity,
  node)` is sound only for a fresh depth-0 traversal with an empty ancestor path.
  Splicing a cached CHILD hash into another traversal is unsound, because cycle and
  depth encoding depend on the ancestor path. This block adds no unbounded global
  fingerprint or representative cache; a bounded per-normalization root cache is
  permitted, and a store-level cache requires measured justification and bounded
  retention."
- **Scope of a derived composite.** "A derived multi-arm composite interns under
  `Global`, as the existing lower-level builder already does. There is no scope join
  to invent for a multi-file composite: `File(A)` would falsely attribute B's
  contribution and vice versa, and a set of dependency roots is not a lexical scope.
  Contributors retain their own scopes; the multi-file dependency set belongs in
  `ReadSetSignature` / observed self-roots / origin edges, never in `NodeScopeId`.
  Singleton normalization returns its retained member unchanged, including that
  member's scope, and authored shells and ordered carriers retain their authored
  scope because they are not derived composites."
- **Freshness rule (verbatim).** "Freshness does not come from reclaiming arena
  nodes." "File-scoped nodes also remain forever in the append-only arena;
  invalidation removes only their dedup-index entries, so a canonical node
  outliving an edit is not by itself stale publication. The freshness rule is:
  normalization records every original contributor in the origin edge; it records
  self-roots for every file-scoped node whose structure the identity walk
  inspected, INCLUDING discarded duplicates; algebraic reads remain in the ordinary
  read set; and an incomplete or depth-budgeted comparison is `ReturnOnly`, never a
  warm canonical result. The existing direct-member rooting must be extended when
  canonical comparison descends through a `Global` intermediate: comparison may
  ignore those descendant scopes for equality, but cache evidence must retain them
  for validation."
- **Enforcement inclusion list owned by this node.** The live dispatch-context
  construction joins: the `NormalizeUnion` / `NormalizeIntersection` query
  execution inside `ProjectSemanticDispatch`; the official union builder's
  `absorb_union` path; and the flow-return join and path-walking direct raw
  construction that the ruling names as the measured defect behind findings A–C.
  The dispatch-reachable remainder of the ruling's enforcement inclusion list —
  mapped / keyof / conditional / utility reducers, relation and call widening,
  object-spread projections, and synthesized closed unions reaching component-meta or
  typeinfo publication — routes here as well, because each is a live dispatch join
  producing findings-class results today. What is left to TA1B is substitution and
  post-substitution finalization, the final pre-seal closure, and the exhaustive
  closure of every construction site the payload flip forces the compiler to
  enumerate.
- **Enforcement exclusion list, DEFINED BY CARRIER SEMANTICS rather than by named
  function.** "authored-syntax lowering and equivalent locator-shape shell lowering;
  display- and materialization-only `TypeExpr` union/intersection construction; and
  EVERY order-sensitive heritage or overload carrier — not only the
  merged-declaration reducer and the declaration-surface merge, but companion
  filters and rebuilders such as the own-body-last heritage reconstruction.
  Authored shells stay available for authored display; once such a shell
  participates in a DERIVED semantic union or intersection, the derived result goes
  through canonical algebra while the authored node remains recoverable through
  graph and origin evidence." This node's substrate must not be wired into an
  excluded carrier; TA1B owns proving the exclusion list is respected across the
  exhaustive site closure.
- **Forbidden normalization positions.** "Normalization is safe only at semantic
  construction and finalization boundaries: all pure union/intersection
  construction routes through the canonical authority, and one idempotent final
  closure runs after fixed-point convergence, substitution and TS inference but
  BEFORE the result is sealed. It must never run: before flow slice hashing —
  `FlowSliceHash` hashes the selected source graph, skeleton, demand and edges, not
  semantic result nodes; normalized result identity must not enter it; inside
  raw-evidence storage; inside `MergedDecl` or the other ordered merge carriers —
  that reducer emits order-sensitive intersection-shaped carriers for heritage
  precedence and overload groups, where commutative sorting or dedup would break
  own-body-last precedence and overload order; treat them as opaque; after
  `SealedFlowCompletion` / `CompleteFlowResult` is minted — the cache gate requires
  the proof's value to equal the admitted value exactly; in display." The final
  idempotent pre-seal closure itself is TA1B's, but the substrate this node builds
  must not itself run in any of these positions.
- **Alias and policy discipline.** "Alias and source identity are preserved through
  carriers and origin edges; normalization never inlines aliases or whole
  declaration graphs, and normalizes only the already-demanded semantic portion.
  Reads performed by normalization use canonical dispatch and enter the fact
  read-set; unknown or budgeted results suppress admission; any policy version
  affecting output belongs in `ResultContractId` or the typed query context, never
  a global boolean and never `FlowSliceHash`."

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then
select the smallest evidence set that actually discriminates the touched contract.
Existing behavioral coverage, compiler/type/capability enforcement, static
validation, canonical gates, bounded inspection, and benchmarks are valid when
accompanied by a terse rationale.

- **TA1A-AC1 — comparator contract:** the comparator is exhaustive over
  `SemanticNodeData`'s manual equality, is iterative and cycle-safe (no unbounded
  recursion on a cyclic structural graph), ignores only the arena sidecar scope, and
  returns `Equal | Distinct | Incomplete` with `Incomplete` preserving both arms and
  suppressing warm admission. A `Distinct` pair must NOT collapse, and an UNDECIDED
  disjointness must NOT reduce to `never`.
- **TA1A-AC2 — canonical builder laws:** the union and intersection builders enforce
  flattening, `T | T = T`, `T | never = T`, and provably disjoint scalar
  intersections to `never` — only PROVEN disjointness, never a guessed relation.
  Derived composites intern under `Global`; a singleton normalization returns its
  retained member with that member's own scope.
- **TA1A-AC3 — findings publish correct and warm:** findings A, B and C from the
  ruling are the discriminating fixtures — `declare const nothing: never; return { v:
  cond ? nothing : "a" }`, an evolving `let` joined through a `switch`, and `type C1
  = { x: string }; type C2 = { x: number }; (v: C1 & C2) => v.x` — each measured
  through the live public boundary (`get_flow_return_type_with_audit`), once routed
  through this node's substrate and interning funnel.
- **TA1A-AC4 — incremental equivalence and bounded work:** prove incremental equals
  fresh and that an `Incomplete` or depth-budgeted comparison is `ReturnOnly`, never
  warm; self-roots must be recorded for every file-scoped node the identity walk
  inspected INCLUDING discarded duplicates, so an edit to a discarded duplicate's
  file misses the warm read; a canonical result that descended through a `Global`
  intermediate must still retain the descendants' file roots as cache evidence.
  Prove no hidden duplicate parse, resolve, comparison, allocation, or retained
  candidate — no unbounded global fingerprint or representative cache is
  introduced; any per-normalization root cache is bounded and depth-0/empty-
  ancestor-path only. Use applicable existing counters, inspection, or benchmarks;
  otherwise record a terse not-applicable rationale.
- Every proposed new test must name a plausible regression or contract boundary not
  already discriminated; prose/format assertions are allowed only when those bytes
  are the public contract. Do not add implementation mirrors, duplicate
  permutations, or universal negative/mutation tests.
- Test homes: `crates/verter_semantic/tests`, `crates/verter_session/tests/cases`.

## Deletions and forbidden designs

- Delete or bypass-proof the direct raw construction sites this node closes (the
  live dispatch-context joins named above) in the flow-return join and path
  walking, each citing the authority route that displaces it; absence of proof
  means preserve and allowlist explicitly.
- **Never build a second parallel type system** or a second normalization engine.
  The ruling is explicit: "Do not create a parallel normalization train. Amend the
  existing U2 owner."
- **Never inline aliases.** Normalization "never inlines aliases or whole
  declaration graphs, and normalizes only the already-demanded semantic portion".
- **Never apply subtype absorption.** Finding B is "canonical semantic idempotence,
  not subtype absorption"; `T | T = T` is identity-based, and a supertype arm never
  swallows a subtype arm here.
- **Never introduce general mutual-assignability.** Only PROVEN disjointness
  collapses an intersection; an undecided relation is never guessed.
- **Never expand into a native-checker or type-system rewrite.** "Scope is bounded
  to satisfying the existing canonical-node contract and A–D. This is NOT a general
  native-checker or type-system rewrite."
- Never change the `structural_hash_of` byte contract to serve this node, never
  deduplicate on a hash match without exact cycle-safe comparison, and never splice
  a cached CHILD hash into another traversal.
- Never normalize in a forbidden position: before flow slice hashing, inside
  raw-evidence storage, inside `MergedDecl` or any other ordered merge carrier,
  after `SealedFlowCompletion` / `CompleteFlowResult` is minted, or in display.
- Never invent a scope join for a multi-file composite, and never put a dependency
  set into `NodeScopeId`.
- Never add a dual-running authority, compatibility fallback, string/regex semantic
  recovery, test-only production bypass, or a global policy boolean; a policy
  version affecting output belongs in `ResultContractId` or the typed query context.
- **Never flip `SemanticNodeData::Union` / `SemanticNodeData::Intersection` to the
  opaque payload, and never attempt the exhaustive repo-wide construction-site
  migration, the carrier-category allowlist, or the compile-fail bypass proof.**
  Those are TA1B's job; performing them here silently enlarges this charter into
  TA1B's mechanism.
- Do not implement successors or silently enlarge this charter. Discovery of a
  second independently acceptable outcome requires an amendment and a new DAG node
  before mutation.

## Budgets and mandatory rescope

- Target ceiling: 1,200 production LOC, 16 production files, 2 related
  crates/packages.
- Mandatory rescope above 1,600 production LOC, 18 files, 3 unrelated
  crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is
  combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete
  result, map/provenance loss, or identity aliasing. An `Incomplete` comparison
  never warms.
- Performance budget: when preflight identifies touched authority or a hot path,
  equivalent-work counters may increase by 0 and wall/allocation/RSS regression
  allowance remains 0.0% unless an owning-authority amendment supplies exact
  replacement thresholds. Otherwise performance evidence is not applicable; do not
  create counters or a soak solely to satisfy this charter.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary —
  in particular if `NormalizeUnion` / `NormalizeIntersection` are not the live
  authority, or if arena interning is not content-`Eq` over `(payload, scope)` as
  the ruling's premise correction records — or if the complete diff will not fit
  one review context.
- Stop if closing the live dispatch-context join requires changing the meaning of
  an order-sensitive heritage or overload carrier; that carrier is excluded by
  carrier semantics, and a conflict is an amendment, not a local decision.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation,
  allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_semantic -p verter_session`
2. Run every final command in the bound `targeted-domain` profile on the squashed
   review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report.
   Behavioral code changes require TDD with a failing discriminating regression
   before production changes; do not invent a test or mutation solely to populate
   evidence.

## Review and lower-severity findings

Apply `semantic-3`: 3 fresh distinct harness tasks covering exactly the profile's
assigned lenses. P0/P1 block final acceptance. A P2 follows the owning review policy
and must have a named owner when deferred; otherwise it blocks. P3 follows the
currently binding owning policy and must be recorded when that policy requires it.
Any post-review content change invalidates every verdict. Final acceptance requires
the complete 3/3 current-round profile to contain independent clean PASS reports on
the squashed review candidate, plus `independent-full` confirmation when required. A
failed review/fix cycle is complete only after all assigned lenses and a
FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch adds one `[[implemented]]` row
to `authority/state/implemented.toml` with the node ID, planned squash commit
message, approximate date with timezone, and optional pull-request number. Row
presence is the implementation fact. Commit metadata is a loose locator only and is
never resolved or validated against Git or GitHub. Reviewers inspect the squashed
candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound
orchestration manifests.

## Scope disposition recorded on review

Review found that the landed node routes construction joins in categories an earlier
reading of this charter reserved for TA1B, and traced the cause to the implementation
brief rather than to the implementation: the brief instructed the implementer to route
every dispatch-context construction join and enumerated the sites in question. Every
routed site was verified behaviourally correct by that review, several of its
disjointness claims independently confirmed against the pinned compiler.

Disposition: ADOPT-NOW. The routing stays where it landed and this charter's boundary
is corrected to describe it, because moving correct, tested routing between nodes to
satisfy a boundary that was never the ruling's own wording would be churn with no
correctness benefit. The mechanism split the architecture authority actually mandated
is preserved intact: TA1A holds the comparator, builders, mint, freshness and the
dispatch-reachable joins with the payload still transparent; TA1B holds the flip, the
carrier-category registry, the compile-fail proof, the pre-seal closure, and the
exhaustive closure the flip forces. Neither a published-subset split nor a partially
closed authority was created.


## Amendment: derived-composite constituent identity excludes source-coordinate spans (architect ruling, 2026-09-01)

Architect ruling, verbatim:

> **A1 — choose (a):** exclude source-coordinate span payloads from
> derived-composite constituent identity only; declaration-site interning
> remains unchanged, and discarded-arm spans remain provenance/freshness
> evidence.
>
> **Acceptance status:** A1 reopens TA1A-AC1/AC2; AC3's existing fixtures
> remain valid but are insufficient for acceptance.

**What was measured.** A review lens over the flow-shape corpus found rows
labelled clean (`MatchesChecker`, no degradation, one warm candidate) whose
published node differs from the checker. Dumped from raw arm payloads: the two
`D06_switch_return` arms are byte-identical except `MemberSpans` (`62..72` vs
`94..104`), and the structural comparator returned `Distinct`. That verdict was
faithful to this charter as previously worded — the identity-mechanism clause
says scope-insensitivity "does NOT mean ignoring scope-bearing semantic payload
fields", and `SurfaceMember.spans` is documented as interning identity — but
the consequence is now rejected: under that reading, canonical `T | T = T`
could never fire for object arms produced by distinct `return` statements, so
every multi-return function published a structurally non-canonical union.

**Corrected contract.** The comparator's identity is `SemanticNodeData`'s
manual equality rules, recursively replacing child ordinals with child
structural identity, ignoring EXACTLY TWO payload axes: the arena sidecar
scope (as before), and the source-coordinate span payloads —
`SurfaceMember.spans`, `IndexSignature.spans`, `Signature.signature_span` /
`Signature.return_type_span`, and `FunctionParam.span`. On this one point the
Identity-mechanism clause and TA1A-AC1's "ignores only the arena sidecar
scope" are RETRACTED and superseded; every other part of AC1/AC2 stands
unchanged (`Equal | Distinct | Incomplete`, iterative and cycle-safe,
`Incomplete` preserves both arms and suppresses warm admission, a `Distinct`
pair never collapses, an undecided disjointness never reduces to `never`,
derived composites intern under `Global`, singleton normalization returns the
retained member with its own scope). Scope-bearing semantic payload fields —
`BareRef` scope, declaration identity (`declaration_origin` included), value
roots, infer-binder identity, and `Signature.occurrence` (a served-position
identity, not a raw coordinate) — remain identity.

The boundary, drawn precisely:

- **Declaration-site interning is UNCHANGED.** Spans stay arena `Eq`/`Hash`
  identity: an identical same-file shape at a different source location still
  interns to a distinct node. Only the derived-composite (union/intersection
  arm) comparison changed. Held by the arena-distinctness leg of the
  discriminating test named below, which fails if the exclusion is
  over-applied to arena `Eq`.
- **Heritage/surface-merge dedup is NOT span-stripped.** The surface merge's
  union-side signature and index-signature dedup ("Distinct declarations stay
  distinct — their source spans participate in node data") is
  declaration-site semantics and is deliberately outside this amendment.
- **Discarded-arm spans remain provenance/freshness evidence.** No span data
  is deleted or un-recorded: a discarded arm keeps its spans on its arena
  node, the normalization origin edge still records it as a contributor, and
  its transitive file roots still enter `CanonicalEvidence`
  (`tier1_payload_equal_discard_roots_descendant_files` and the discarded-arm
  rooting in the comparator remain in force).
- **Candidate narrowing is bucketed on canonical identity, not on the audit
  fingerprint.** The pairwise tier's bucket key is a LOCAL,
  per-canonicalization hash aligned with the comparator (`arm_bucket_keys`
  in `canonical_algebra.rs`): `compare_structural(a, b) == Equal` implies
  equal bucket keys, so `T | T = T` fires identically below and above the
  narrowing threshold. `structural_hash_of` keeps its byte contract
  untouched, as this charter requires — it is the audit subsystem's
  fingerprint and correctly covers the spans and `TypeParam.display_name`
  that canonical identity excludes, which is exactly why it is no longer
  the group key. Cycle-reaching arms share one coarse bucket (coinductive
  equality is not positionally hashable, and `Equal` never pairs a cyclic
  arm with an acyclic one); a tripped bucket-hash node cap disables the
  narrowing for that run (one all-arms group under the ordinary pairwise
  budget) instead of grouping on partial keys.
- **Opaque payloads stay conservative.** `ObjectSpreadProgram` /
  `DeferredCallable` arms compare payload-`Eq` only; a pair differing only by
  internal spans stays `Distinct` (both arms kept) — a preservation, never a
  collapse, so it is not reopened by this ruling.

**Re-verified acceptance.**

- **TA1A-AC1 (re-verified against the amended identity):**
  `span_only_distinct_arms_collapse_in_derived_composites_yet_intern_distinct`
  (`crates/verter_session/src/project_semantic_dispatch/tests.rs`) is the
  discriminating evidence for the amended rule specifically: written first and
  observed failing against the pre-amendment comparator (the union kept both
  span-only-distinct arms), passing after; it pins the span-only collapse for
  member, index-signature, signature and parameter spans, the arena
  distinctness of every span-differing pair (the over-application tripwire),
  and the semantic-field negative control that never collapses.
  `span_only_duplicates_collapse_above_prehash_narrowing_threshold` extends
  the proof past the candidate-narrowing threshold: a union with more
  child-bearing arms than the narrowing minimum, holding one span-only
  duplicate pair, collapses to the deduplicated arm set — written first and
  observed failing while the narrowing still bucketed on the span-covering
  audit fingerprint (11 arms survived), passing once the bucketing was
  aligned with canonical identity. The pre-existing AC1 suite
  (`canonical_algebra_collapses_only_proven_facts`,
  the cross-scope dedup and discard-rooting tests) stays green and continues
  to hold the unchanged clauses.
- **TA1A-AC2 (re-verified):** the builder laws are unchanged and their
  existing evidence stands; the amended constituent identity now feeds
  `T | T = T` through the same tests above, and at the live public boundary
  (`get_flow_return_type_with_audit`, cold + warm-replay) the span-class
  corpus rows publish the collapsed canonical shape clean and warm:
  `D06_switch_return`, `X04_try_catch_join`, `X15_labelled_block_return`,
  `X16_switch_fallthrough`, `X25_try_assertion_catch_scope`,
  `X35_labeled_break_carries_write_state`, `X44_switch_exhaustive_boolean`
  now publish the checker's single object, and `X22_switch_break_case_entry`
  publishes exactly the checker's two distinct arms (deep-pinned:
  `Expect::Node` + `Boundary::Audit`, warm replay) — the no-over-collapse
  control alongside the untouched `X100` / `X111` negative controls.
- **TA1A-AC3:** the Y01/Y02/Y03 fixtures remain valid and green; per the
  ruling they are insufficient alone for acceptance — the discriminating
  evidence for the amended identity is the test and boundary rows named
  above.

**Residue disclosed, out of this ruling's scope.** Re-measurement against the
pinned checker (tsc 7.0.2, `--noEmit --strict`, every probe rejected with the
exact type printed and an `IsAny` control) confirmed the recorded checker
answers of all sixteen A1-listed rows. Eight of the sixteen moved to the
checker's exact shape under A1 (the rows named under AC2 above, plus the
arm-count reduction inside `X22`). The other eight — `X26`, `X36`, `X39`,
`X45`, `X46`, `X47`, `X48`, `X49` — retain a DIFFERENT divergence class the
span exclusion cannot and must not touch: each keeps a subtype pair the
checker merges by return-position subtype reduction (e.g. `{ v: number }`
absorbed into `{ v: string | number }`). `X26` reduced from three arms to two
(its span-duplicate collapsed); `X36`, `X39`, `X45`–`X49` are byte-identical
to their pre-A1 shapes — their divergence was never the span class. This
charter forbids subtype absorption in the canonical layer ("a supertype arm
never swallows a subtype arm here"), and the predecessor decision assigns
"context-specific subtype-to-supertype reunion" to the TypeScript-compatible
inference layer, so the residue is owned there (the flow-return evaluator's
inference layer, `U6.VALUE_INFERENCE`), not by this node. The rows stay
extensionally equal to the checker and keep their corpus labels; closing them
needs an inference-layer decision, not a canonical-identity one.
