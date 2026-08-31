# Canonical type algebra as a D2B predecessor (rev11.flow)

- Status: accepted
- Date: 2026-08-31
- Scope: adds two predecessor units to D2B; amends no other node's budgets or boundaries

## Context

Review of the D2B candidate surfaced eight defects in control-position narrowing,
seven of which were repaired inside the flow evaluator. Four remaining findings were
measured against the pinned checker and the live public boundary
(`get_flow_return_type_with_audit`, two calls) and are NOT narrowing defects:

- **A** `declare const nothing: never; return { v: cond ? nothing : "a" }` —
  checker `{ v: string }`, substrate `{ v: Union(never | string) }`, clean and warm.
- **B** an evolving `let` joined through a `switch` — checker `{ v: string }`,
  substrate `{ v: Union(string | string) }`, clean and warm. The `if/else` twin
  produces the same shape but rides a degradation and never warms.
- **C** `type C1 = { x: string }; type C2 = { x: number }; (v: C1 & C2) => v.x` —
  checker `{ v: never }`, substrate `{ v: Intersection(number & string) }`, clean and warm.
- **D** `type Tag = ` + "`item-${string}`" + ` | "none"` under a truthiness guard —
  the checker excludes `""` from the falsy edge; the substrate keeps both arms,
  clean and warm.

A, B and C are semantically equal to the checker's answer but structurally different.
None loses a return contributor. All four publish warm.

## Ruling (architecture authority)

**The authority exists; the defect is enforcement.** `ProjectSemanticDispatch` owns
the live `NormalizeUnion` / `NormalizeIntersection` queries, assigned to
`U2.QUERY_VALUE_DOMAIN`, which `U6.FLOW_RETURN_SUBSTRATE` already declares as a
prerequisite. Construction is not closed over it: the official union builder applies
`X | never = X` through `absorb_union`, but the lower-level
`intern_normalized_union_or_intersection` applies only one level of union flattening,
limited literal subsumption, and a sort-and-dedup by raw arena ordinal, while
intersections are interned as given. The flow-return join and path walking both
construct through that lower-level path directly.

**Correction to the first ruling's stated root cause.** That ruling held that node
allocation is not hash-consing. Verified against source, it is: `arena.rs` interns
through 16 sharded dedup indexes with content `Eq` over `(payload, scope)` as the
identity authority, and its module doc states two nodes intern to the same id IFF
they are structurally and scope equal. The real cause of the duplicate-union finding
is that SCOPE IS DELIBERATELY PART OF IDENTITY — `Primitive(String)` interned under
two different files is legitimately two ids — and the helper deduplicates by arena
ordinal, so the arms never collapse. The work is therefore not "add hash-consing" but
"make constituent identity scope-insensitive and set-shaped for union and
intersection members".

**A–D are blocking for D2B.** Not because a value is wrong, but because
`FlowReturnResult` contractually promises the whole-return node is canonical and
D2B-AC2 pins exact identity: extensional equality does not satisfy a public
canonical-node contract, and all four warm-admit. Owner attribution changes where the
fix lands, not whether D2B may certify it.

**Do not create a parallel normalization train.** Amend the existing U2 owner. The
separately reviewable unit is `U2.CANONICAL_TYPE_ALGEBRA`, a sub-block of
`U2.QUERY_VALUE_DOMAIN` — not a second normalization system. Case D additionally
requires a shared demand-scoped `ClassifyTruthinessDomain` query returning whether a
type has truthy and/or falsy inhabitants, owned outside flow and consumed by it.

### Four-layer contract

| Layer | Owns | May assume |
|---|---|---|
| Raw flow evidence | Every return contributor and edge, source/control provenance, freshness, inference-only status, feasibility, failures. | Only that referenced semantic reads are typed and fact-rooted. Not normalization, not TS inference policy. |
| Canonical semantic types | Universal algebraic laws and reusable facts: `T \| never = T`, `T \| T = T`, flattening, provably disjoint scalar intersections to `never`, truthiness-domain classification. | Raw evidence is complete. It may preserve source carriers through origin sidecars, but may not retain redundant algebra members merely to preserve provenance. |
| TypeScript-compatible inference | Which syntactic contributors participate, conditional/evolving-value widening, alias representative selection, context-specific subtype-to-supertype reunion. | Canonical operations and relation facts are authoritative. Every composite it emits is constructed through the canonical algebra. It cannot redefine `never`, equality, or intersection. |
| Display | Ordering, spelling, truncation, rendering of the selected carrier. | The supplied type and alias choice are already semantically decided. It never resolves or repairs types. |

Per-finding assignment: A is canonical algebra (raw evidence keeps the `never`
contributor and its provenance; TS inference owns widening `"a"` to `string`;
canonical union normalization removes `never`). B is canonical semantic idempotence,
not subtype absorption. C reduces to `never` only where domains are PROVABLY disjoint
— an undecided relation is never guessed, and the authored carrier stays recoverable
through origin evidence. D is a certified falsy-edge fact defect: canonical semantics
says the falsy restriction of `Tag` is `never`, while TS inference separately decides
that the syntactic numeric return still contributes, so the final declaration may
still read `{ v: Tag } | { v: number }`. The fix is not to drop the return.

Type-impossible contributors stay in raw evidence with provenance: algebraic `never`
does not erase evidence, and evidence does not force `never` to remain a union
constituent.

### Forbidden normalization positions

Normalization is safe only at semantic construction and finalization boundaries: all
pure union/intersection construction routes through the canonical authority, and one
idempotent final closure runs after fixed-point convergence, substitution and TS
inference but BEFORE the result is sealed. It must never run:

- before flow slice hashing — `FlowSliceHash` hashes the selected source graph,
  skeleton, demand and edges, not semantic result nodes; normalized result identity
  must not enter it;
- inside raw-evidence storage;
- inside `MergedDecl` or the other ordered merge carriers — that reducer emits
  order-sensitive intersection-shaped carriers for heritage precedence and overload
  groups, where commutative sorting or dedup would break own-body-last precedence and
  overload order; treat them as opaque;
- after `SealedFlowCompletion` / `CompleteFlowResult` is minted — the cache gate
  requires the proof's value to equal the admitted value exactly;
- in display.

Alias and source identity are preserved through carriers and origin edges;
normalization never inlines aliases or whole declaration graphs, and normalizes only
the already-demanded semantic portion. Reads performed by normalization use canonical
dispatch and enter the fact read-set; unknown or budgeted results suppress admission;
any policy version affecting output belongs in `ResultContractId` or the typed query
context, never a global boolean and never `FlowSliceHash`.

## Decision

Take the prerequisite-first path. The canonical-node contract is NOT weakened, and
A–D are NOT recorded as accepted warm debt.

1. `U2.CANONICAL_TYPE_ALGEBRA` lands as an explicit sub-block under
   `U2.QUERY_VALUE_DOMAIN` and an explicit D2B predecessor. It closes union and
   intersection construction over the authoritative normalization path, removes or
   bypass-proofs direct raw construction from the flow-return join and path walking,
   establishes semantic constituent identity sufficient for canonicalization without
   relying on fresh `SemanticNodeId` equality, and enforces at least flattening,
   `T | T = T`, `T | never = T`, and provably impossible primitive intersections such
   as `string & number = never`.
2. `ClassifyTruthinessDomain` lands as the demand-scoped truthiness authority, owned
   outside flow, absorbing rather than duplicating the existing per-arm truthiness
   helpers. Flow consumes the fact.
3. Both become explicit D2B predecessors in the DAG and ledger before D2B landing
   resumes.

Scope is bounded to satisfying the existing canonical-node contract and A–D. This is
NOT a general native-checker or type-system rewrite.

## Consequences

- D2B does not land until both predecessors land.
- D2B's review profile re-runs afterward; the content change invalidates every
  current verdict.
- D2B's acceptance criteria are re-evaluated against the canonical-node contract.
- The two flow-evaluator repairs in flight — const-asserted literal identity through
  an evolving-variable join, and `in` modelling property presence separately from
  value non-`undefined` — are independent of this decision and remain valid.


## Mechanism ruling (follow-up, after the premise correction)

**Identity.** "Scope-insensitive" means ignoring the ARENA SIDECAR scope
(`NodeArena::node_scope(id)`) during comparison. It does NOT mean ignoring
scope-bearing semantic payload fields — `BareRef` scope, declaration identity, value
roots, infer-binder identity all remain identity. The canonical authority is a
dedicated, exhaustive structural comparator whose identity is `SemanticNodeData`'s
manual equality rules, recursively replacing child ordinals with child structural
identity, ignoring only that sidecar scope. It returns `Equal | Distinct |
Incomplete`; `Incomplete` preserves both arms and suppresses canonical warm
admission.

`structural_hash_of` is NOT sanctioned as that authority. It may serve as a prehash
for candidate narrowing and primary ordering only, and a hash match must be followed
by exact cycle-safe comparison; collisions take a deterministic structural tie-break
and never deduplicate. It diverges from arena equality materially: it includes
`TypeParam.display_name` which arena equality excludes, contributes only a variant
tag for `DeferredCallable`, omits identity-bearing `SurfaceMember` fields such as
`has_implementation_body` and `excess_origin`, and its depth and unresolved-child
sentinels deliberately permit distinct structures to share a fingerprint. Its byte
contract is not to be changed to serve this block; shared traversal plumbing may be
extracted, but canonical identity and audit fingerprinting remain distinct policies.

Memoization: caching a completed ROOT fingerprint for `(store identity, node)` is
sound only for a fresh depth-0 traversal with an empty ancestor path. Splicing a
cached CHILD hash into another traversal is unsound, because cycle and depth encoding
depend on the ancestor path. This block adds no unbounded global fingerprint or
representative cache; a bounded per-normalization root cache is permitted, and a
store-level cache requires measured justification and bounded retention.

**Scope of a derived composite.** A derived multi-arm composite interns under
`Global`, as the existing lower-level builder already does. There is no scope join to
invent for a multi-file composite: `File(A)` would falsely attribute B's
contribution and vice versa, and a set of dependency roots is not a lexical scope.
Contributors retain their own scopes; the multi-file dependency set belongs in
`ReadSetSignature` / observed self-roots / origin edges, never in `NodeScopeId`.
Singleton normalization returns its retained member unchanged, including that
member's scope, and authored shells and ordered carriers retain their authored scope
because they are not derived composites.

**Freshness does not come from reclaiming arena nodes.** File-scoped nodes also
remain forever in the append-only arena; invalidation removes only their dedup-index
entries, so a canonical node outliving an edit is not by itself stale publication.
The freshness rule is: normalization records every original contributor in the origin
edge; it records self-roots for every file-scoped node whose structure the identity
walk inspected, INCLUDING discarded duplicates; algebraic reads remain in the
ordinary read set; and an incomplete or depth-budgeted comparison is `ReturnOnly`,
never a warm canonical result. The existing direct-member rooting must be extended
when canonical comparison descends through a `Global` intermediate: comparison may
ignore those descendant scopes for equality, but cache evidence must retain them for
validation.

**Enforcement boundary.** Route through the canonical authority: path-walking and
projection joins; mapped / keyof / conditional / utility reducers; substitution and
post-substitution finalization; relation and call widening; object-spread
projections; flow joins, SCC and fixed-point output, and the final pre-seal closure;
and synthesized closed unions reaching component-meta or typeinfo publication.

Exclude, by CARRIER SEMANTICS rather than by named function: authored-syntax lowering
and equivalent locator-shape shell lowering; display- and materialization-only
`TypeExpr` union/intersection construction; and EVERY order-sensitive heritage or
overload carrier — not only the merged-declaration reducer and the declaration-surface
merge, but companion filters and rebuilders such as the own-body-last heritage
reconstruction. Authored shells stay available for authored display; once such a
shell participates in a DERIVED semantic union or intersection, the derived result
goes through canonical algebra while the authored node remains recoverable through
graph and origin evidence.

Enforcement is an explicit raw-constructor allowlist with bypass proof for authored
and ordered carriers, with all other production semantic composite construction
routed through the authority. Reconnaissance measured the surface at 34 production
construction sites across 17 files, of which roughly 14 reach a published flow,
component-meta or typeinfo surface.
