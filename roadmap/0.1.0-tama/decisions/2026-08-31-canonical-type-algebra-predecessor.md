# Canonical type algebra as a D2B predecessor (rev11.flow)

- Status: accepted
- Date: 2026-08-31
- Scope: adds two predecessor units to D2B; amends no other node's budgets or boundaries.
  No other node receives algebra ownership. The consumer clauses in D3R, D3P, D4, D7, E4 and
  `NCF-FD-NARROW` were clarified to name TA1A and TA2 as the authorities they already consume.

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
- **D** ``type Tag = `item-${string}` | "none"`` under a truthiness guard —
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

## Deferred: one generated projection is stale by construction

The `NCF-FD-NARROW` amendment landed in its SOURCE, the `fact_sources` row of
`catalogs/native-checker-family-manifest.toml`, which is the authority. Its generated
projection at `charters/expansion-native-checker/generated-families/NCF-FD-NARROW.md`
could not be regenerated: no generator exists in the tree. The thirty generated family
charters were added in one commit with no accompanying tool, they carry per-slice
content that is not derivable from the manifest alone, and the generator is owned by
NCK4, which has a GitHub issue mapping but no implemented ledger row.

The projection therefore stays stale until NCK4 lands, and the DAG validator does not
cross-check generated bodies against the manifest, so nothing detects it
mechanically. The exact delta the regenerated file will need is one bullet under
`#### Required fact and proof inputs`, between the relation/comparability proofs entry
and the assignment/capture effects entry:

    - ClassifyTruthinessDomain truthiness-domain facts; checker-private truthiness
      classification is forbidden

Recorded here so the amendment is not lost between the manifest and the projection.

A second, narrower limitation is recorded rather than worked around: the manifest
schema is `additionalProperties: false` over a fixed thirteen-field slice shape with
no per-slice prohibition field, so the ban on checker-private truthiness
classification rides the `fact_sources` string rather than a structural forbidden-
design field. Giving it a field of its own requires a schema amendment plus the NCK4
generator, and is not done here.

## Amendment: the algebra unit splits by mechanism

A preflight inventory measured the atomic canonical-algebra unit at roughly 24
production files and 1,100-1,400 LOC. The line count fits the amended 1,500-LOC
target; the FILE count does not fit 18, and the cause is structural rather than
incidental: the enforcement mechanism flips the union and intersection payloads to an
opaque carrier, which forces a token edit at every authored, ordered and rebuilder
construction site in the same compile. Bringing it under the ceiling would require a
blanket conversion that lets any producer mint a composite, which defeats the seal
the mechanism exists to provide.

The architecture authority pre-authorized exactly one remedy for that outcome, and
forbade the obvious alternative. Splitting around the roughly fourteen sites that
currently reach a published surface is NOT acceptable: it would leave the
non-published semantic constructors outside the authority, ready to become defects
the moment they are exposed. Neither a published subset nor a temporarily partially
closed authority is a valid intermediate state.

The sanctioned split is by MECHANISM:

- **TA1A** — the comparator, the freshness plumbing, the canonical builder, and the
  sealed mint substrate, with the enum payload NOT yet flipped. Its acceptance is
  that the three measured findings publish correctly and warm at the public boundary.
- **TA1B** — the payload flip, the exhaustive construction-site migration under the
  carrier-category registry, the compile-fail unforgeability proof, and the final
  idempotent pre-seal closure.

TA1B depends on TA1A, and the flow cutover depends on TA1B; TA1A is reached
transitively and is deliberately not listed as a second predecessor edge. Consumer
charters name TA1A, because what they consume is the canonical builder.

One disclosed language limit rides with TA1B rather than being engineered around:
Rust visibility makes the mints unforgeable from OUTSIDE the crate and provable by
compile-fail, while in-crate the exhaustive carrier-category match is the forcing
function rather than the type system. In-crate unforgeability is not claimed.

## Correction: why the heritage carrier is excluded

The ruling excluded the ordered heritage and overload carriers on the stated grounds
that reordering them would destroy heritage precedence and overload order. A negative
control run against that exclusion showed the first half is NOT true of this tree, and
the exclusion needs its real justification recorded so a later implementer does not
reason their way past it.

Measured: planting a canonical sort of the heritage carrier's arms failed ZERO tests
across the merged-declaration, heritage and shadow suites. Planting an unambiguous
reversal — which a coincidental id-order sort cannot imitate — still left every
VALUE-level "own body shadows inherited member" test green. Every reachable consumer
resolves own-versus-heritage member conflicts through the role stamp and topology
classification re-derived at the consumption site, never through the arm's position.
Member-type precedence is therefore order-INDEPENDENT today, and the ruling's stated
rationale does not hold for it.

What the order does carry is the RENDERED TYPE TEXT. A reordered carrier renders
`B & A & { x: number }` where the authored type is `A & B & { x: number }`, changing
hover and display output. That is the property the exclusion actually protects, and it
was under-protected: the one display-consistency test that existed happened to be
immune to an ascending-id sort, because its fixture interns the heritage reference
before the freshly built own object, making the sort a no-op there.

A discriminating pin now exists. It uses three arms with an adversarial interning
order — the second heritage reference interned first, so its node id sorts lower —
because a two-arm fixture has nothing to swap past and cannot expose a sort at all.

Overload order was checked in the same pass and is genuinely pinned by an existing
assertion that reads the first and last projected signature and requires them to
differ, so that half of the exclusion stands as stated.

Consequence for the construction-site closure: the ordered carriers remain excluded,
but the reason to cite is display and authored-order fidelity, not member precedence.
An implementer who tests only value-level shadowing will conclude the exclusion is
unnecessary, and will be wrong.

## Ratified budget variance — the substrate node

The substrate node breached both budget dimensions and the breach was escalated to the
architecture authority rather than absorbed. Ruling: it lands at its measured size and
is NOT split again. No sound mechanism boundary remained — moving the
dispatch-reachable joins would reverse the recorded ADOPT-NOW disposition and become
the site-based split already refused; separating the comparator and builders from
freshness and admission would leave a warm-unsafe substrate that is not independently
acceptable; and the seven review fixes close correctness properties of one authority
rather than rollback-independent features.

The charter's budget row is deliberately NOT rewritten. Rewriting a threshold to make a
completed node appear compliant would erase the fact that the escalation correctly
occurred, so the original target and the breached rescope thresholds remain as the
historical acceptance record, and the overrun is recorded here instead:

- measurement: cumulative GROSS additions from the node's start point; gross additions
  are the budget measure, net change is supplementary disclosure
- scope: `crates/*/src/**`, excluding test modules
- approved actual: 2,417 gross lines added, +1,893 net, 22 production files, 2 related
  crates
- disposition: architect-ratified overrun, no further split

Three causes compounded, recorded in order of contribution. The largest was an
orchestration error: the implementation brief instructed routing of every
dispatch-context construction join and enumerated sites the charter reserved for the
closure node, which was dispositioned ADOPT-NOW and the charters corrected, widening
real scope without the budget row moving. The second was the fix round itself, roughly
869 production lines closing seven blocking findings. The third was an under-predicting
preflight, which projected the ATOMIC unit at ~1,100-1,400 lines across ~24 files while
the substrate node alone reached 22.

## Revised budget — the closure node

The closure node's original 700 lines / 16 files is not credible and is revised BEFORE
implementation. The representation flip still forces edits at already-routed sites, so
ratifying the routing into the substrate node reduced semantic work without removing
the compiler-wide file fanout — the same pressure that broke the substrate's file
count.

Revised: 1,000 production lines / 24 files target, 1,500 / 26 rescope, 2 related
crates, classified `L` at high effort, retaining the `semantic-3` review profile. It is
NOT split now. If exact preflight exceeds the revised ceiling, escalate again — and the
only permissible further mechanism boundary is the opaque-payload and exhaustive-site
seal first, the idempotent pre-seal closure second. A subset of construction sites
remains refused.

## Ratified budget variance — the closure node

The closure node exceeded the file ceiling that had just been revised for it, and the
breach was escalated rather than absorbed. Ruling: ratify, do not split.

- measurement: cumulative GROSS additions from the node's start point
- approved actual: 663 gross lines added, 28 production files, 1 related crate
- accounting: two further changed files are modified only inside cfg-gated test
  modules and do NOT count as production files; the implementer's 26-file figure is
  not adopted
- disposition: architect-ratified overrun, no further split

The thresholds themselves stay unchanged, as they did for the substrate node, so the
escalation remains visible in the record rather than being erased by a rewritten row.

The overage is the predicted compiler-enforced fanout of the opaque-payload seal: the
seal is what forces every construction and read site to declare a category, which
produces many small edits across many files and few lines overall — 663 lines across 28
files. The only permitted split would leave the seal half above the ceiling anyway, so
it would not satisfy the rescope condition it was meant to answer.

No second node is created. Forced, it would contain only the idempotent whole-result
re-close before sealing plus its placement and idempotence proof, which does not
justify a separate landing and review cycle. That closure is instead reviewed as a
dedicated concern inside the closure node's own review profile.

This variance concerns budget only and prejudges neither correctness nor the
apparent-type callability decision.

## Bounded-work rationale for the closure node

The charter routes bounded-work evidence to the review report and permits a terse
not-applicable rationale in place of counters, forbidding their creation solely to
satisfy it. Recorded here because no in-tree evidence convention exists.

The seal adds no cache, counter, retained candidate or global fingerprint. Its memo
change only SUPPRESSES publishes. Canonicalization at the newly routed sites runs on
changed paths only, under budgets the substrate node already established.

CORRECTED TWICE. First, review measured that the closure was NOT free. It early-returns
only after the full canonical pipeline has already run — flatten with per-member root
recording, the dedup comparator, absorption, sort and re-intern — so every union-top
flow close pays that pipeline and deposits non-trivial canonical evidence, advancing
the evidence epoch even on the pure no-op path. The original wording here, that it
re-interns an already-canonical top with no new repeated work to count, was inaccurate
and is retracted rather than quietly amended.

Two consequences were checked. The epoch advance cannot suppress a substitution
publish, because a substitution walk never calls flow and each substitution captures
its own epoch baseline after any prior bump. But an incomplete canonicalization inside
the closure is a new suppression ingress: with a build frame it folds cache-suppress
onto the enclosing flow build, and without one it marks the whole request partial. The
direction is safe — it suppresses and never promotes — and no incomplete deposit was
observed on that path, but the breadth is unquantified.

One real cost is disclosed rather than measured. The evidence-blind-replay fence makes
the cross-request substitution memo permanently unpublishable for any walk whose
canonicalization observed file-scoped roots, and file-rooted composites are the common
case in real user code, so that path now recomputes per request. That is the sound
direction: the alternative is replaying an under-rooted entry into a request that never
observed the roots it depends on, which is a stale warm read and a correctness-budget
violation rather than a performance one. Making it cheap again requires per-entry root
carriage on the memo, which is a cache-model change owned elsewhere and deliberately
not attempted here.

No counters or soak were built to quantify it, per the charter. The trade is stated so
a later owner can find it, not buried in a measurement that would have had to be
invented.

CORRECTED A THIRD TIME (architect review, 2026-09-01): the paragraph below's opening
claim that the following review round "made it free" is itself an overbroad
performance claim — this same rationale has now been corrected for an inaccurate
performance claim twice already, immediately above. It is retracted in place rather
than deleted, per this trail's convention, and retained only as the historical record
of what was previously claimed. The accurate final state, and the only claim made
here, is: proved-canonical union tops re-close in O(1); unproven/untagged tops still
run the pipeline; and the substitution-memo replay fence described above still causes
disclosed cross-request recomputation. No stronger, and no newly-measured, performance
claim is asserted beyond that sentence.

[HISTORICAL / "made it free" framing retracted above — mechanism description retained]
Then the review round that followed made it free: a canonically-tagged union top now
skips the pipeline entirely on an O(1) tag test, with no evidence deposit and no epoch
advance, and the tag's own semantics guarantee that skip is exactly the idempotence
no-op — but only under a qualifier a later probe had to force. As first written the
builder stamped the tag UNCONDITIONALLY, incomplete paths included: an over-cap arm
set, an exhausted compare budget, a dangling arm or an undecided peek all carried a
tag asserting a canonical form they did not have, so the skip fired on a list that was
not canonical and a budget-exceeded result became warm-classifiable on resurfacing,
against the rule that budget exhaustion never warms. The builder now REFUSES the tag on
incomplete evidence — those results carry a distinct unproven tag and pay the full
re-close — so the tag is a witness of proven canonical form rather than of mint
provenance. The paragraph above therefore describes the state between those two rounds and
is retained rather than deleted, because the measurement that produced it is what
forced the cheap path to exist. For the common case the closure is now O(1); the
pipeline still runs, with its evidence deposit and epoch advance, for a top that is not
canonically tagged.
