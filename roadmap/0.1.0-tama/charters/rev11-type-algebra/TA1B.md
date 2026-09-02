<!-- unified-charter-v2
id=TA1B
name=Canonical composite payload and construction-site closure
phase=rev11
train=rev11.type-algebra
product=rev11
kind=implementation
semantic_role=delivery
class=foundational-authority-closure
predecessors=TA1A
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
charter=charters/rev11-type-algebra/TA1B.md
max_production_loc=1000
max_production_files=24
max_related_packages=2
rescope_loc=1500
rescope_files=26
rescope_unrelated_packages=3
-->

# TA1B — Canonical composite payload and construction-site closure

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

TA1B is the second half of the split of former node TA1 ("Canonical union and
intersection algebra closure"). The split is by mechanism, not by "already-
published subset": TA1A ("Canonical algebra comparator builder and mint substrate")
built the comparator, canonical builders, `CanonicalMint`, and freshness plumbing
without changing `SemanticNodeData`'s representation, so its own footprint stayed
bounded. TA1B's entire job is the enforcement mechanism that makes bypassing that
substrate impossible: flipping `SemanticNodeData::Union` /
`SemanticNodeData::Intersection` to an opaque payload minted exclusively by TA1A's
`CanonicalMint`. That flip is exactly what forces a token edit at every authored,
ordered, and rebuilder construction site in the same compile — the reason former
TA1 measured ~24 production files against an 18-file budget while its LOC stayed in
budget. Splitting around only the currently-published construction sites would have
left non-published constructors outside the authority, so TA1B closes EVERY
production construction site the ruling's enforcement inclusion list names, not a
convenient subset.

Per the ruling (`decisions/2026-08-31-canonical-type-algebra-predecessor.md`), TA1A
and TA1B together are the separately reviewable unit `U2.CANONICAL_TYPE_ALGEBRA`, "a
sub-block of `U2.QUERY_VALUE_DOMAIN` — not a second normalization system." Neither is
a new normalization engine, a parallel normalization train, nor a general
native-checker or type-system rewrite. TA1B specifically owns: flipping `Union` /
`Intersection` to the opaque payload; the exhaustive construction-site migration
under an exhaustive carrier-category registry with sealed per-category bypass
mints; the compile-fail proof that an untrusted producer cannot mint a derived
composite or a bypass; and the final idempotent pre-seal closure that runs "after
fixed-point convergence, substitution and TS inference but BEFORE the result is
sealed."

The node is accepted when its four owned obligations hold together: (1) the enum
payload is opaque and every remaining production semantic composite construction
outside the carrier-semantics exclusion list is closed over TA1A's authority, with
each remaining raw constructor recorded on the explicit carrier-category allowlist
with its bypass proof; (2) the allowlist is an exhaustive TYPE/CAPABILITY inventory
— never a name-keyed source scanner — and a compile-fail test proves an untrusted
producer cannot mint a derived composite or a bypass mint from outside its sealed
category; (3) the final idempotent pre-seal closure runs exactly once at the
sanctioned boundary and never re-enters a forbidden normalization position; and (4)
the closure introduces no hidden duplicate parse, resolve, comparison, or retained
candidate beyond what TA1A's substrate already bounds.

The layering this node owns is still the ruling's **canonical semantic types**
layer — TA1B does not move any construction into the raw-flow-evidence,
TypeScript-compatible-inference, or display layers; it only makes the layer TA1A
built structurally unbypassable. This charter accepts one authority-closure
boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_semantic/src`, `crates/verter_session/src`.
- Named API/data boundaries: `SemanticNodeData::Union` / `SemanticNodeData::Intersection`,
  flipped to the opaque payload minted exclusively by TA1A's `CanonicalMint`; the
  exhaustive carrier-category registry and its sealed per-category bypass mints;
  every remaining production raw-construction call site named by the ruling's
  enforcement inclusion list (path-walking and projection joins; mapped / keyof /
  conditional / utility reducers; substitution and post-substitution finalization;
  relation and call widening; object-spread projections; flow joins, SCC and
  fixed-point output; the final pre-seal closure; and synthesized closed unions
  reaching component-meta or typeinfo publication); the raw-constructor allowlist
  and its recorded bypass proof for authored and ordered carriers.
- Mutation boundary: only the production surfaces and named API/data boundaries
  above; every changed path must be inside both that charter surface and the
  acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **TA1A:** implemented ledger row for "Canonical algebra comparator builder and
  mint substrate"; ledger presence alone satisfies the predecessor. Its commit
  message, approximate timezone-bearing date, and optional PR are locator hints
  only. TA1A is TA1B's sole predecessor: it supplies the comparator, canonical
  builders, `CanonicalMint`, and freshness plumbing that the opaque-payload flip and
  the exhaustive construction-site closure depend on.
- **External requirements:** agents check any listed requirement; tooling does not
  validate external state.

## Source-specific scope

- **`structural_hash_of` is not the authority.** "It may serve as a prehash for
  candidate narrowing and primary ordering only, and a hash match must be followed
  by exact cycle-safe comparison; collisions take a deterministic structural
  tie-break and never deduplicate." Its byte contract is not to be changed to serve
  this block; shared traversal plumbing may be extracted, but canonical identity
  and audit fingerprinting remain distinct policies. TA1B inherits this constraint
  unchanged from TA1A — the flip to an opaque payload must not touch
  `structural_hash_of`'s byte contract.
- **Scope of a derived composite.** "A derived multi-arm composite interns under
  `Global`, as the existing lower-level builder already does. There is no scope
  join to invent for a multi-file composite: `File(A)` would falsely attribute B's
  contribution and vice versa, and a set of dependency roots is not a lexical
  scope. Contributors retain their own scopes; the multi-file dependency set
  belongs in `ReadSetSignature` / observed self-roots / origin edges, never in
  `NodeScopeId`. Singleton normalization returns its retained member unchanged,
  including that member's scope, and authored shells and ordered carriers retain
  their authored scope because they are not derived composites." Every site this
  node migrates onto the opaque payload must preserve this scope behavior exactly;
  the flip changes representation, never scope semantics.
- **Enforcement inclusion list owned by this node (full closure).** "path-walking
  and projection joins; mapped / keyof / conditional / utility reducers;
  substitution and post-substitution finalization; relation and call widening;
  object-spread projections; flow joins, SCC and fixed-point output, and the final
  pre-seal closure; and synthesized closed unions reaching component-meta or
  typeinfo publication." TA1A already routed the live dispatch-context joins
  (`NormalizeUnion` / `NormalizeIntersection` execution, the official union
  builder's `absorb_union` path, and the flow-return join / path-walking direct
  construction) onto the substrate; TA1B's closure must confirm those sites still
  compile and behave correctly once the enum becomes opaque, and must additionally
  migrate every remaining site above.
- **Enforcement exclusion list, DEFINED BY CARRIER SEMANTICS rather than by named
  function.** "authored-syntax lowering and equivalent locator-shape shell lowering;
  display- and materialization-only `TypeExpr` union/intersection construction; and
  EVERY order-sensitive heritage or overload carrier — not only the
  merged-declaration reducer and the declaration-surface merge, but companion
  filters and rebuilders such as the own-body-last heritage reconstruction.
  Authored shells stay available for authored display; once such a shell
  participates in a DERIVED semantic union or intersection, the derived result goes
  through canonical algebra while the authored node remains recoverable through
  graph and origin evidence." These carriers are NOT migrated onto the opaque
  payload; they are the excluded set the allowlist records with its bypass proof.
- **Enforcement shape.** "Enforcement is an explicit raw-constructor allowlist with
  bypass proof for authored and ordered carriers, with all other production
  semantic composite construction routed through the authority." Reconnaissance
  measured the surface at "34 production construction sites across 17 files, of
  which roughly 14 reach a published flow, component-meta or typeinfo surface";
  that count is a scoping estimate, not an acceptance quota. **The allowlist is a
  TYPE/CAPABILITY inventory — an exhaustive carrier-category enum with sealed
  per-category bypass mints — never a name-keyed source scanner.** A guard or test
  that keys enforcement on a spelled function, module, or file name is not a
  landed mechanism here; the exhaustive category enum, matched without a wildcard
  arm, is the forcing function.
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
  the proof's value to equal the admitted value exactly; in display." TA1B owns
  landing the final idempotent pre-seal closure at exactly the sanctioned boundary.
- **Correction to the heritage-carrier rationale quoted above (stale wording, do not
  cite as-is).** The quoted rationale's claim that "commutative sorting or dedup
  would break own-body-last precedence and overload order" is STALE for the
  precedence half. `decisions/2026-08-31-canonical-type-algebra-predecessor.md` →
  "Correction: why the heritage carrier is excluded" measured it directly: planting
  a canonical sort of the heritage carrier's arms failed ZERO tests across the
  merged-declaration, heritage and shadow suites, and an unambiguous reversal still
  left every value-level "own body shadows inherited member" test green — every
  reachable consumer resolves own-versus-heritage member conflicts through the role
  stamp and topology classification re-derived at the consumption site, never
  through the arm's position. Member-type precedence is therefore
  order-INDEPENDENT today, and the quoted rationale's stated justification does not
  hold for it. What the order actually protects is the RENDERED TYPE TEXT /
  authored-order display fidelity: a reordered carrier renders `B & A & { x: number
  }` where the authored type is `A & B & { x: number }`, changing hover and display
  output. The overload-order half of the quoted rationale stands as stated — it is
  genuinely pinned by an existing assertion reading the first and last projected
  signature. TA1B's construction-site closure still excludes these ordered
  carriers per the Enforcement exclusion list above; the reason to cite is display
  and authored-order fidelity, not member precedence.
- **Alias and policy discipline.** "Alias and source identity are preserved through
  carriers and origin edges; normalization never inlines aliases or whole
  declaration graphs, and normalizes only the already-demanded semantic portion.
  Reads performed by normalization use canonical dispatch and enter the fact
  read-set; unknown or budgeted results suppress admission; any policy version
  affecting output belongs in `ResultContractId` or the typed query context, never
  a global boolean and never `FlowSliceHash`."
- **Disclosed language limit (record, do not paper over).** Rust visibility makes
  the sealed per-category bypass mints unforgeable from OUTSIDE the crate, and that
  unforgeability is provable by compile-fail (a foreign crate cannot name the
  private mint constructor or match the sealed category enum's private variants).
  IN-CRATE, the same visibility barrier does not hold: any function inside the
  crate can, in principle, call a private constructor. In-crate the forcing
  function is the EXHAUSTIVE carrier-category enum matched without a wildcard arm
  at every migrated site, not the type system — an exhaustive match over a sealed
  enum turns "someone adds a new raw-construction call site" into a compile error
  at the match, not into a runtime-invisible bypass. Do not present in-crate
  unforgeability as achievable; the honest claim is cross-crate unforgeability plus
  in-crate exhaustiveness-forced review, and the acceptance proof and review must
  state it exactly this way.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then
select the smallest evidence set that actually discriminates the touched contract.
Existing behavioral coverage, compiler/type/capability enforcement, static
validation, canonical gates, bounded inspection, and benchmarks are valid when
accompanied by a terse rationale.

- **TA1B-AC1 — sole-owner closure:** the existing `NormalizeUnion` /
  `NormalizeIntersection` authority remains the SOLE normalization owner, the enum
  payload is opaque, and construction is closed over it everywhere outside the
  carrier-semantics exclusion list. Prove that every production semantic composite
  construction outside that exclusion list reaches the authority, and that each
  remaining raw constructor is on the explicit allowlist with its bypass proof
  recorded. Prefer existing type, capability, dependency, compiler, or static
  enforcement over a new scanner — a name-keyed scanner is forbidden as the
  enforcement mechanism (see Deletions and forbidden designs). Add a negative or
  mutation leg only for a plausible critical fail-closed boundary: an
  inclusion-list site restored to raw construction must FAIL TO COMPILE against the
  opaque payload.
- **TA1B-AC2 — compile-fail unforgeability proof:** a test (or a `trybuild`-style
  compile-fail fixture) proves a producer outside the sealed bypass-mint categories
  cannot construct a derived `Union` / `Intersection` node nor a bypass. The proof
  and its recorded rationale state the disclosed language limit exactly: unforgeable
  from outside the crate, exhaustiveness-forced in-crate.
  Reuse existing coverage or extend/table-drive one test before creating a new one.
- **TA1B-AC3 — idempotent pre-seal closure:** the final closure runs exactly once,
  after fixed-point convergence, substitution and TS inference, and strictly BEFORE
  sealing; running it twice on an already-closed result is a no-op (idempotent); and
  it never re-enters a forbidden position — before flow slice hashing, inside
  raw-evidence storage, inside `MergedDecl` or another ordered merge carrier, after
  `SealedFlowCompletion` / `CompleteFlowResult` is minted, or in display.
- **TA1B-AC4 — bounded work:** prove no hidden duplicate parse, resolve, comparison,
  allocation, or retained candidate is introduced by the site migration or the
  closure pass — no unbounded global fingerprint or representative cache. Use
  applicable existing counters, inspection, or benchmarks; otherwise record a terse
  not-applicable rationale.
- Every proposed new test must name a plausible regression or contract boundary not
  already discriminated; prose/format assertions are allowed only when those bytes
  are the public contract. Do not add implementation mirrors, duplicate
  permutations, or universal negative/mutation tests.
- Test homes: `crates/verter_semantic/tests`, `crates/verter_session/tests/cases`.

## Deletions and forbidden designs

- Delete or bypass-proof every remaining raw construction site named by the
  ruling's enforcement inclusion list, each citing the authority route that
  displaces it; absence of proof means preserve and allowlist explicitly.
- **Never build a second parallel type system** or a second normalization engine.
  The ruling is explicit: "Do not create a parallel normalization train. Amend the
  existing U2 owner."
- **Never inline aliases.** Normalization "never inlines aliases or whole
  declaration graphs, and normalizes only the already-demanded semantic portion".
- **Never apply subtype absorption.** `T | T = T` is identity-based, and a
  supertype arm never swallows a subtype arm here.
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
- **Never enforce the allowlist as a name-keyed source scanner.** The allowlist is
  an exhaustive type/capability inventory — a sealed carrier-category enum matched
  without a wildcard arm — never a grep or AST scan keyed on a spelled function,
  module, path, or string identity. A residual scanner is not a landed mechanism
  for this block, even as a "supplement to a structural primary."
  **Never present in-crate unforgeability as achieved by Rust visibility alone.**
  The disclosed language limit above is the honest claim; do not silently narrow it
  in review or in test naming.
- Do not implement successors or silently enlarge this charter. Discovery of a
  second independently acceptable outcome requires an amendment and a new DAG node
  before mutation.

## Budgets and mandatory rescope

- Target ceiling: 1,000 production LOC, 24 production files, 2 related
  crates/packages.
- Mandatory rescope above 1,500 production LOC, 26 files, 3 unrelated
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
  in particular if TA1A's `CanonicalMint` and comparator are not yet landed and
  ready to receive the flip, or if the complete diff will not fit one review
  context.
- Stop if closing an inclusion-list site requires changing the meaning of an
  order-sensitive heritage or overload carrier; that carrier is excluded by carrier
  semantics, and a conflict is an amendment, not a local decision.
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

Before squashing or review, the implementation patch transitions this node's
predeclared row in `authority/state/implemented.toml` from `status = "pending"`
to `status = "implemented"` with the planned squash commit message, approximate
date with timezone, and optional pull-request number. The transitioned row is the
implementation fact. Commit metadata is a loose locator only and is never resolved or
validated against Git or GitHub. Reviewers inspect the squashed candidate patch without
SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.

## Boundary correction inherited from the predecessor's review

The predecessor's review corrected where the dispatch-reachable construction joins
land. They routed through the canonical authority in TA1A — path-walking and
projection joins, mapped / keyof / utility reducers, relation and call widening,
object-spread projections, and the synthesized closed unions reaching component-meta
or typeinfo publication — and that placement was ratified there rather than moved,
because the routing is correct and tested and relocating it would be churn.

This node's remaining scope is therefore sharper, not smaller in kind: the payload
flip; substitution and post-substitution finalization; the final idempotent pre-seal
closure; and the exhaustive closure of every construction site the flip forces the
compiler to enumerate, under the carrier-category registry with its sealed per-category
bypass mints and the compile-fail proof.

The forcing function is the point. Because the flip makes the compiler enumerate every
remaining site, this node cannot silently miss one, which is precisely why the
architecture authority refused a split around the sites that happened to be published
at measurement time.

## Inherited observation — apparent-type callability is wider than the carriers already moved

The predecessor's closing round moved arrays, tuples and template literals from
"provably yields no call signatures" to the fail-closed arm, after measuring against
the pinned compiler that a global interface augmentation makes values of all three
callable. Mapped types and object-spread programs were verified genuinely
signature-free and stayed.

The same argument extends further and was deliberately NOT acted on there: a `keyof`
domain and the string primitive and string literal carriers are equally callable under
a `String` augmentation. Moving them too would have broadly disabled the canonical
route for member-value intersections, which is a scope decision this node is the right
place to make rather than a closing round on its predecessor.

The implementation therefore had to choose between admitting those carriers to the
fail-closed arm or adding a real apparent-type callability question. It could not retain
the disproven claim that those carriers never yield call signatures, because that would
silently reverse an overload set.

RESOLVED in the landed implementation
(`walk.rs::value_may_contribute_call_signatures`): the augmentable
apparent-type carriers — a deferred `keyof` shell, and every surface whose
global backing interface user code may legally augment with a call signature
(`Array` / `String`, so arrays, tuples, and template literals) — joined the
fail-closed possibly-callable arm and preserve their raw ordered form. SCALAR
primitive and literal arms alone stay on the canonical route, by the measured
order-safety argument rather than a shape guess: different-domain scalar pairs
collapse to `never` before any ordering exists, and same-domain scalar arms
share one backing interface's signature list, so commutative reordering is
unobservable to overload resolution. No documented never-callable claim
remains for the augmentable carriers.
