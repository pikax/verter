<!-- unified-charter-v2
id=TA1
name=Canonical union and intersection algebra closure
phase=rev11
train=rev11.type-algebra
product=rev11
kind=implementation
semantic_role=delivery
class=foundational-authority-closure
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
charter=charters/rev11-type-algebra/TA1.md
max_production_loc=1200
max_production_files=12
max_related_packages=2
rescope_loc=2000
rescope_files=18
rescope_unrelated_packages=3
-->

# TA1 — Canonical union and intersection algebra closure

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

Close union and intersection CONSTRUCTION over the normalization authority that already exists. Per the ruling (`decisions/2026-08-31-canonical-type-algebra-predecessor.md`), "**The authority exists; the defect is enforcement.** `ProjectSemanticDispatch` owns the live `NormalizeUnion` / `NormalizeIntersection` queries, assigned to `U2.QUERY_VALUE_DOMAIN`, which `U6.FLOW_RETURN_SUBSTRATE` already declares as a prerequisite. Construction is not closed over it: the official union builder applies `X | never = X` through `absorb_union`, but the lower-level `intern_normalized_union_or_intersection` applies only one level of union flattening, limited literal subsumption, and a sort-and-dedup by raw arena ordinal, while intersections are interned as given. The flow-return join and path walking both construct through that lower-level path directly." This node is the separately reviewable unit `U2.CANONICAL_TYPE_ALGEBRA`, "a sub-block of `U2.QUERY_VALUE_DOMAIN` — not a second normalization system". It is NOT a new normalization engine, NOT a parallel normalization train, and NOT a general native-checker or type-system rewrite; its scope "is bounded to satisfying the existing canonical-node contract and A–D".

The node is accepted when all four of the ruling's structural obligations hold together: (1) union and intersection construction is CLOSED over the authoritative normalization path — direct raw construction from the flow-return join and path walking is removed or bypass-proofed; (2) semantic constituent identity is established "sufficient for canonicalization without relying on fresh `SemanticNodeId` equality"; (3) the algebra enforces AT LEAST "flattening, `T | T = T`, `T | never = T`, and provably impossible primitive intersections such as `string & number = never`", where "C reduces to `never` only where domains are PROVABLY disjoint — an undecided relation is never guessed, and the authored carrier stays recoverable through origin evidence"; and (4) canonical results carry the ruling's freshness evidence or do not warm at all.

The layering this node owns is the ruling's **canonical semantic types** layer, which owns "universal algebraic laws and reusable facts: `T | never = T`, `T | T = T`, flattening, provably disjoint scalar intersections to `never`, truthiness-domain classification" and may assume "raw evidence is complete. It may preserve source carriers through origin sidecars, but may not retain redundant algebra members merely to preserve provenance." It does NOT own the raw-flow-evidence layer (every return contributor, edge, provenance, freshness, feasibility), the TypeScript-compatible-inference layer (which syntactic contributors participate, widening, alias representative selection), or the display layer (ordering, spelling, truncation). Per the ruling: "Type-impossible contributors stay in raw evidence with provenance: algebraic `never` does not erase evidence, and evidence does not force `never` to remain a union constituent." Findings A (`never` in a conditional arm), B (idempotence through a `switch` join) and C (`{ x: string } & { x: number }`) land here; A's widening of `"a"` to `string` belongs to TS-compatible inference, not to this node. This charter accepts one authority-closure boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_semantic/src`, `crates/verter_session/src`.
- Named API/data boundaries: the live `SemanticQueryKey::NormalizeUnion` / `SemanticQueryKey::NormalizeIntersection` queries and their `ProjectSemanticDispatch` execution; the official union builder and its `absorb_union` law application; the lower-level `intern_normalized_union_or_intersection` interning path; `SemanticNodeData` and its manual equality rules; the node arena's sharded dedup indexes and the `NodeArena::node_scope(id)` sidecar; `NodeScopeId` and the `Global` scope of a derived composite; `ReadSetSignature` / observed self-roots / origin edges; and `structural_hash_of` as a PREHASH ONLY.
- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **None.** This node is foundational: nothing existing gates it, and it is independent of TA2 so the two are reviewable in parallel. It is an explicit predecessor of D2B.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- **Identity mechanism (verbatim constraint).** "'Scope-insensitive' means ignoring the ARENA SIDECAR scope (`NodeArena::node_scope(id)`) during comparison. It does NOT mean ignoring scope-bearing semantic payload fields — `BareRef` scope, declaration identity, value roots, infer-binder identity all remain identity. The canonical authority is a dedicated, exhaustive structural comparator whose identity is `SemanticNodeData`'s manual equality rules, recursively replacing child ordinals with child structural identity, ignoring only that sidecar scope. It returns `Equal | Distinct | Incomplete`; `Incomplete` preserves both arms and suppresses canonical warm admission."
- **`structural_hash_of` is not the authority.** "It may serve as a prehash for candidate narrowing and primary ordering only, and a hash match must be followed by exact cycle-safe comparison; collisions take a deterministic structural tie-break and never deduplicate." It "diverges from arena equality materially: it includes `TypeParam.display_name` which arena equality excludes, contributes only a variant tag for `DeferredCallable`, omits identity-bearing `SurfaceMember` fields such as `has_implementation_body` and `excess_origin`, and its depth and unresolved-child sentinels deliberately permit distinct structures to share a fingerprint. Its byte contract is not to be changed to serve this block; shared traversal plumbing may be extracted, but canonical identity and audit fingerprinting remain distinct policies."
- **Memoization bound.** "caching a completed ROOT fingerprint for `(store identity, node)` is sound only for a fresh depth-0 traversal with an empty ancestor path. Splicing a cached CHILD hash into another traversal is unsound, because cycle and depth encoding depend on the ancestor path. This block adds no unbounded global fingerprint or representative cache; a bounded per-normalization root cache is permitted, and a store-level cache requires measured justification and bounded retention."
- **Scope of a derived composite.** "A derived multi-arm composite interns under `Global`, as the existing lower-level builder already does. There is no scope join to invent for a multi-file composite: `File(A)` would falsely attribute B's contribution and vice versa, and a set of dependency roots is not a lexical scope. Contributors retain their own scopes; the multi-file dependency set belongs in `ReadSetSignature` / observed self-roots / origin edges, never in `NodeScopeId`. Singleton normalization returns its retained member unchanged, including that member's scope, and authored shells and ordered carriers retain their authored scope because they are not derived composites."
- **Freshness rule (verbatim).** "Freshness does not come from reclaiming arena nodes." "File-scoped nodes also remain forever in the append-only arena; invalidation removes only their dedup-index entries, so a canonical node outliving an edit is not by itself stale publication. The freshness rule is: normalization records every original contributor in the origin edge; it records self-roots for every file-scoped node whose structure the identity walk inspected, INCLUDING discarded duplicates; algebraic reads remain in the ordinary read set; and an incomplete or depth-budgeted comparison is `ReturnOnly`, never a warm canonical result. The existing direct-member rooting must be extended when canonical comparison descends through a `Global` intermediate: comparison may ignore those descendant scopes for equality, but cache evidence must retain them for validation."
- **Enforcement inclusion list (route through the canonical authority).** "path-walking and projection joins; mapped / keyof / conditional / utility reducers; substitution and post-substitution finalization; relation and call widening; object-spread projections; flow joins, SCC and fixed-point output, and the final pre-seal closure; and synthesized closed unions reaching component-meta or typeinfo publication."
- **Enforcement exclusion list, DEFINED BY CARRIER SEMANTICS rather than by named function.** "authored-syntax lowering and equivalent locator-shape shell lowering; display- and materialization-only `TypeExpr` union/intersection construction; and EVERY order-sensitive heritage or overload carrier — not only the merged-declaration reducer and the declaration-surface merge, but companion filters and rebuilders such as the own-body-last heritage reconstruction. Authored shells stay available for authored display; once such a shell participates in a DERIVED semantic union or intersection, the derived result goes through canonical algebra while the authored node remains recoverable through graph and origin evidence."
- **Enforcement shape.** "Enforcement is an explicit raw-constructor allowlist with bypass proof for authored and ordered carriers, with all other production semantic composite construction routed through the authority." Reconnaissance measured the surface at "34 production construction sites across 17 files, of which roughly 14 reach a published flow, component-meta or typeinfo surface"; that count is a scoping estimate, not an acceptance quota.
- **Forbidden normalization positions.** "Normalization is safe only at semantic construction and finalization boundaries: all pure union/intersection construction routes through the canonical authority, and one idempotent final closure runs after fixed-point convergence, substitution and TS inference but BEFORE the result is sealed. It must never run: before flow slice hashing — `FlowSliceHash` hashes the selected source graph, skeleton, demand and edges, not semantic result nodes; normalized result identity must not enter it; inside raw-evidence storage; inside `MergedDecl` or the other ordered merge carriers — that reducer emits order-sensitive intersection-shaped carriers for heritage precedence and overload groups, where commutative sorting or dedup would break own-body-last precedence and overload order; treat them as opaque; after `SealedFlowCompletion` / `CompleteFlowResult` is minted — the cache gate requires the proof's value to equal the admitted value exactly; in display."
- **Alias and policy discipline.** "Alias and source identity are preserved through carriers and origin edges; normalization never inlines aliases or whole declaration graphs, and normalizes only the already-demanded semantic portion. Reads performed by normalization use canonical dispatch and enter the fact read-set; unknown or budgeted results suppress admission; any policy version affecting output belongs in `ResultContractId` or the typed query context, never a global boolean and never `FlowSliceHash`."

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **TA1-AC1 — sole-owner outcome:** the existing `NormalizeUnion` / `NormalizeIntersection` authority remains the SOLE normalization owner, and construction is closed over it. Prove that every production semantic composite construction outside the carrier-semantics exclusion list reaches the authority, and that each remaining raw constructor is on the explicit allowlist with its bypass proof recorded. Prefer existing type, capability, dependency, compiler, or static enforcement over a new scanner. Add a negative or mutation leg only for a plausible critical fail-closed boundary: an inclusion-list site restored to raw construction must FAIL.
- **TA1-AC2 — positive contract:** the algebra enforces flattening, `T | T = T`, `T | never = T`, and provably disjoint scalar intersections to `never`, and the comparator returns `Equal | Distinct | Incomplete` with `Incomplete` preserving both arms. Findings A, B and C from the ruling are the discriminating fixtures — `declare const nothing: never; return { v: cond ? nothing : "a" }`, an evolving `let` joined through a `switch`, and `type C1 = { x: string }; type C2 = { x: number }; (v: C1 & C2) => v.x` — each measured through the live public boundary (`get_flow_return_type_with_audit`). A `Distinct` pair must NOT collapse, and an UNDECIDED disjointness must NOT reduce to `never`. Derived composites intern under `Global`; a singleton normalization returns its retained member with that member's own scope. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **TA1-AC3 — incremental equivalence:** prove incremental equals fresh and that an `Incomplete` or depth-budgeted comparison is `ReturnOnly`, never warm. Self-roots must be recorded for every file-scoped node the identity walk inspected INCLUDING discarded duplicates, so an edit to a discarded duplicate's file misses the warm read; a canonical result that descended through a `Global` intermediate must still retain the descendants' file roots as cache evidence.
- **TA1-AC4 — bounded work:** prove no hidden duplicate parse, resolve, comparison, allocation, or retained candidate — no unbounded global fingerprint or representative cache is introduced; any per-normalization root cache is bounded and depth-0/empty-ancestor-path only. Use applicable existing counters, inspection, or benchmarks; otherwise record a terse not-applicable rationale.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `crates/verter_semantic/tests`, `crates/verter_session/tests/cases`.

## Deletions and forbidden designs

- Delete or bypass-proof the direct raw construction sites in the flow-return join and path walking, each citing the authority route that displaces it; absence of proof means preserve and allowlist explicitly.
- **Never build a second parallel type system** or a second normalization engine. The ruling is explicit: "Do not create a parallel normalization train. Amend the existing U2 owner."
- **Never inline aliases.** Normalization "never inlines aliases or whole declaration graphs, and normalizes only the already-demanded semantic portion".
- **Never apply subtype absorption.** Finding B is "canonical semantic idempotence, not subtype absorption"; `T | T = T` is identity-based, and a supertype arm never swallows a subtype arm here.
- **Never introduce general mutual-assignability.** Only PROVEN disjointness collapses an intersection; an undecided relation is never guessed.
- **Never expand into a native-checker or type-system rewrite.** "Scope is bounded to satisfying the existing canonical-node contract and A–D. This is NOT a general native-checker or type-system rewrite."
- Never change the `structural_hash_of` byte contract to serve this node, never deduplicate on a hash match without exact cycle-safe comparison, and never splice a cached CHILD hash into another traversal.
- Never normalize in a forbidden position: before flow slice hashing, inside raw-evidence storage, inside `MergedDecl` or any other ordered merge carrier, after `SealedFlowCompletion` / `CompleteFlowResult` is minted, or in display.
- Never invent a scope join for a multi-file composite, and never put a dependency set into `NodeScopeId`.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, or a global policy boolean; a policy version affecting output belongs in `ResultContractId` or the typed query context.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 1,200 production LOC, 12 production files, 2 related crates/packages.
- Mandatory rescope above 2,000 production LOC, 18 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing. An `Incomplete` comparison never warms.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a soak solely to satisfy this charter.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary — in particular if `NormalizeUnion` / `NormalizeIntersection` are not the live authority, or if arena interning is not content-`Eq` over `(payload, scope)` as the ruling's premise correction records — or if the complete diff will not fit one review context.
- Stop if closing an inclusion-list site requires changing the meaning of an order-sensitive heritage or overload carrier; that carrier is excluded by carrier semantics, and a conflict is an amendment, not a local decision.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_semantic -p verter_session`
2. Run every final command in the bound `targeted-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `semantic-3`: 3 fresh distinct harness tasks covering exactly the profile's assigned lenses. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch adds one `[[implemented]]` row to `authority/state/implemented.toml` with the node ID, planned squash commit message, approximate date with timezone, and optional pull-request number. Row presence is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.
