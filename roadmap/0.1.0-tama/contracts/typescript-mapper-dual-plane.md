# TypeScript mapper: the dual-plane contract

This contract is the sole owner of the TypeScript mapper architecture and of the
observation identity the mapper and the semantic oracle share. It replaces the
rejected closure package and the string mapper plane. Every charter in
`rev11.typescript-mapper` implements against this text; where a charter's
generated prose and this contract disagree, this contract wins.

The contract is authority, not implementation. It adds no production code. It
states the boundary the implementing blocks must land inside, and the boundary
their reviews check them against.

## 1. Two planes, one direction

There are exactly two planes.

**The mapping plane** answers geometry: given a carrier source (`.vue`,
`.svelte`, any adapter carrier) and the TypeScript surface projected from it,
which region of one corresponds to which region of the other. Its inputs are the
committed carrier bytes and the emission record produced while building the
surface. Its output is a total, typed correspondence.

**The semantic plane** answers meaning: given a position, a symbol, or a query,
what TypeScript says. Its inputs are a certified engine binding and a query
identity. Its output is a result carrying its own exactness and capability
contract.

The dependency direction is one-way and total: the semantic plane consumes
mapping-plane products; the mapping plane never calls the semantic plane. A
mapper that can ask the oracle a question has made geometry depend on
type-checking, which makes the correspondence non-total, non-deterministic under
cancellation, and unavailable while the engine is down — three failures that
present as a hang rather than an error. The prohibition is therefore
architectural, not stylistic: it is what makes the mapping plane answerable
without an engine at all.

This is enforced structurally by dependency direction, not by inspecting names:
the crate that owns the mapping products must not reach the crate that owns the
oracle binding, and the mapping product type must expose no callback, closure
parameter, trait object, or handle through which an oracle query could be
issued.

**Forbidden by construction.**

- A mapper callback into the semantic oracle, in any shape.
- A string mapper plane: correspondence recovered by searching, slicing, or
  matching generated text. Geometry comes from the emission record that produced
  the text, never from re-reading the text. This is the same rule
  `CodeTransform` already states for source maps: modifying built output
  desynchronises byte offsets, so the transform — not the string — is the truth.
- A second correspondence path that exists "for now" beside the first.

## 2. The five projection classes

The classification is total over **both** surfaces, and it has to be stated
that way because two of the five classes are one-sided. Every byte of the
projected TypeScript surface belongs to exactly one of Identity, Relocated,
Rewritten, or Synthesized; every byte of the carrier source belongs to exactly
one of Identity, Relocated, Rewritten, or Elided. The three shared classes are
the ones that carry a correspondence; Synthesized is projected bytes with no
carrier preimage, and Elided is carrier bytes with no projection.

Stating totality over the projected surface alone would be a hole rather than a
simplification: an Elided region contributes no projected byte, so an
implementation that dropped elided carrier regions entirely would still satisfy
a projected-side-only partition while leaving carrier positions with no
disposition at all, which is exactly the mis-mapping to a neighbour the Elided
class exists to refuse. The correspondence is bidirectional, so the partition
that makes it total must be too.

Totality decides which class a byte belongs to. It does not decide how many
projected regions a carrier region corresponds to, and that has to be stated
separately, because the emission already in the tree is one-to-many: a
`v-model` value expression is emitted once as a read and again as an assignment
target, a `v-slot` destructuring pattern is emitted alongside the bindings it
introduces, and each emission is a projected region of the same class with the
same carrier preimage. For Identity, Relocated, and Rewritten, one carrier
region therefore corresponds to **one or more** projected regions, and the
correspondence is complete in the carrier-to-projected direction: the mapping
product answers a carrier region with every projected region derived from it,
never with the first one found. A provider operation anchored at a carrier
region that reaches fewer than all of that region's projected regions fails
closed rather than applying partially.

Leaving that unstated would be a hole of the same class as stating totality on
one side only. A mapping product typed as a function from carrier region to
projected region satisfies the partition exactly — every projected byte carries
a class, every carrier byte carries a class — while a rename issued from the
carrier side rewrites one of three emissions of a `v-model` expression and
leaves the projected surface internally inconsistent. Neither fail-closed class
catches it: those bytes are Relocated, whose disposition is full.

The partition is closed and exhaustive; a region that matches no class is a
lowering defect, not a sixth class.

| Class | Carrier preimage | Correspondence | IDE disposition |
| --- | --- | --- | --- |
| **Identity** | exact carrier bytes, unmoved relative to their block | offset delta | full: every provider feature maps both ways |
| **Relocated** | exact carrier bytes emitted at a different position or nesting | region-to-region, order-preserving | full |
| **Rewritten** | carrier bytes whose text is transformed while a single carrier region remains responsible for them | region-to-region, not byte-to-byte | full at region granularity; a sub-region edit that cannot be mapped exactly fails closed |
| **Synthesized** | none | none | fail closed: no hover, no edit, no diagnostic surfaced at a carrier position |
| **Elided** | carrier bytes with no projection | none | fail closed: a position inside an elided region has no TypeScript correlate and must be reported as such, never mapped to a neighbour |

The two fail-closed classes are the load-bearing ones. An unmapped provider edit
whose range covers synthesized or elided bytes is refused, never approximated —
a mis-mapped edit corrupts the user's file, while a refused one is a visible,
correctable gap.

## 3. Observation identity

Three identities are already distinct nominal types and must stay distinct.
Their separation is the whole of the observation-identity rescope.

**`InputBasisId` — the in-flight observation basis.** It names the committed
input state a result is being produced against. It is *not* a cache-candidate
key and must never enter one: a basis-keyed cache answers a question about a
snapshot that has already been superseded.

**`QueryIdentity<Q>` — the snapshot-independent question.** Semantic arguments,
the observed profile identities in canonical sorted order, and the
result-contract identity. It carries no `InputBasisId`, which is exactly what
makes it a valid cross-snapshot cache-candidate key. The observed profile set is
part of the identity, not context: two answers produced under different
capability interpretations are different answers and must not share a slot.

That component is a set, and the composition makes it one. Observing a profile
twice is the same question as observing it once, so it must compose the same
identity — multiplicity is not a distinction this identity is permitted to
carry. Sorting alone does not supply that: a sorted list of profile digests is
order independent and still encodes a repeat as a second slot for one question.
The shared canonical encoder therefore deduplicates a set-shaped field as well
as sorting it, which is where this requirement holds — for this identity and
for every other composition that describes a set of observed things. A
composition that reintroduced multiplicity here is a defect against this
section, not a gap it discloses.

**`SemanticFlightKey<Q>` — the in-flight production key.** `(QueryIdentity<Q>,
InputBasisId)`. Strictly larger than `QueryIdentity<Q>`, so the two cannot
coerce in either direction. Cross-snapshot joining is a runtime decision that is
off by default; it is never a property of the key type.

**Lineage is not content.** A carrier unit's identity is its source lineage plus
its logical role. An edit or a reparse changes the unit's content and revision;
it must not mint a new unit. A mapping product keyed on content rather than
lineage silently loses every association across the first keystroke.

**Aliasing is a compile error, not a convention.** These are separate nominal
types with no conversion between them; the compile-fail contracts under
`crates/verter_identity/tests/compile-fail/` are the proof and must remain.

**Where these three live is part of the vertical's surface.** `InputBasisId`,
`QueryIdentity`, and `SemanticFlightKey` are defined in
`crates/verter_identity/src`, and coverage of them belongs in the crate that
owns them. That crate is therefore inside this vertical's production surface,
and a charter in `rev11.typescript-mapper` whose surface list omits it is
disagreeing with this contract — which, per the precedence stated at the top,
this contract wins. A block obliged to change one of the three is not out of
scope for doing so; it is out of scope only for changing something else.

## 4. The certified engine binding

`CertifiedTypeEngineBinding` is the sole route by which a TypeScript engine's
answer enters the semantic plane. It is a witness, not a handle: holding one
means a project binding has been resolved, the engine's capability
interpretation has been observed and recorded in the profile set that composes
`QueryIdentity`, and the basis the answer will be attributed to is live.

No production path may produce a semantic result from an inferred project, an
unbound engine, a path-shaped notification, or a fallback route. This is the
same discipline the project-bound external-TypeScript contract already applies
through its `BoundProject` witness, raised to the whole semantic plane rather
than one backend.

Raising it widens which backends the prohibition binds, not which lanes it
eliminates, and one existing lane is deliberately outside it. The explicitly
labelled `SyntheticScratch` lane is retained: it produces no semantic result in
this contract's sense, because it never supplies configured-project semantics,
batch typecheck, cross-file results, or project-cache warming. It is a
non-semantic scratch lane, not an inferred-project route, and an implementing
block that deleted it would be regressing a sanctioned capability rather than
satisfying this section.

## 5. Lifecycle: what may be published, and what may be kept

- A result is publishable only against a live basis. If the basis it was
  produced under is superseded before publication, the result is returned to its
  caller and discarded, never warmed.
- Degraded outcomes never warm: cancellation, budget exhaustion, engine
  unavailability, partial completion, and supersession are all return-only.
- A cache slot is keyed by `QueryIdentity`, never by `SemanticFlightKey` and
  never by `InputBasisId`. Validity is decided by re-validating the recorded
  observations against the live state, not by the age of the entry.
- Incremental must equal fresh. A warm answer that differs from the answer a
  cold run would give is a correctness defect, not a performance trade.

## 6. Ownership

One capability has one owner. Where two candidate owners could answer the same
question, the contract names one and the other is deleted, not demoted.

| Capability | Owner |
| --- | --- |
| Mapping products and their geometry | the mapping-product owner inside `CodeTransform` |
| Projection of a carrier surface into mapping products | the content-mapper projection plane |
| Certified binding, query identity, flight keys, result contracts | the semantic plane |
| Activation and deletion of the displaced routes | the activation block |

The projection plane and the semantic capability closure land dormant. Nothing
routes through them until activation, and activation is atomic with the deletion
of the route it replaces. A period during which both the old and the new route
can answer is forbidden — that is the dual-running authority this program
rejects everywhere.

Landing dormant is a decided override of the general instruction to delete a
superseded path in the same change that replaces it, and it is recorded here so
that an implementer meeting the two rules together meets a decision rather than
an undocumented conflict. Activation has to be atomic with the deletion, which
requires the replacing route to already exist in the tree; the interval this
permits is unrouted code, never a second answer, so the sharper half of the rule
— no window in which two routes can answer one question — is preserved exactly.
A block landing a dormant plane is following this contract and does not record a
deviation for it.

## 7. Deletion and survivor rows

Concrete, not narrative. Each displaced route is deleted or structurally
rejected; each preserved artifact is carried forward as a stated obligation on a
named receiving criterion. The rows themselves live in the closure register at
`closure/typescript-mapper/register.toml` and are validated there.

## 8. What this contract does not settle

Three questions are deliberately left open and transferred, with owners, to the
implementing blocks: the hang topology of the semantic plane under concurrent
flights; the selection of the projection and semantic topologies; and the
pre-change implementation baseline the successors compare against. They are
recorded as residues in the closure register with a direct receiving criterion
and a resolution gate. No fourth open question is admissible: a newly discovered
one requires an amendment and a new node, not a fourth residue row.
