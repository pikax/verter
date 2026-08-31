<!-- unified-charter-v2
id=TA2
name=Demand-scoped truthiness domain authority
phase=rev11
train=rev11.type-algebra
product=rev11
kind=implementation
semantic_role=delivery
class=foundational-authority
predecessors=
owner=rev11.type-algebra:the single canonical semantic algebra authority
conflict_domains=semantic_authority
resource_class=rust-mixed
review_profile=semantic-3
gate_profile=targeted-domain
implementation_effort_min=medium
implementation_effort_default=medium
review_effort_min=medium
review_effort_default=medium
verification_effort_min=medium
verification_effort_default=medium
confirmation_effort_min=medium
confirmation_effort_default=medium
size=M
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/rev11-type-algebra/TA2.md
max_production_loc=600
max_production_files=8
max_related_packages=2
rescope_loc=1200
rescope_files=12
rescope_unrelated_packages=3
-->

# TA2 — Demand-scoped truthiness domain authority

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

Land the ruling's second prerequisite verbatim (`decisions/2026-08-31-canonical-type-algebra-predecessor.md`): "`ClassifyTruthinessDomain` lands as the demand-scoped truthiness authority, owned outside flow, absorbing rather than duplicating the existing per-arm truthiness helpers. Flow consumes the fact." The ruling states the requirement as "a shared demand-scoped `ClassifyTruthinessDomain` query returning whether a type has truthy and/or falsy inhabitants, owned outside flow and consumed by it".

Three properties define acceptance. (1) **Demand-scoped query, owned outside flow.** `ClassifyTruthinessDomain` is a demand-scoped query answering, for a demanded type, whether it has truthy inhabitants and whether it has falsy inhabitants. It belongs to the ruling's **canonical semantic types** layer, which owns "universal algebraic laws and reusable facts: `T | never = T`, `T | T = T`, flattening, provably disjoint scalar intersections to `never`, truthiness-domain classification". It is not owned by, and does not live inside, the flow evaluator. (2) **Flow CONSUMES, never reimplements.** The flow narrowing frames read the fact; they do not carry a private truthiness rule, and they do not fork a second classification for their own arms. (3) **Absorption, not duplication.** The existing per-arm truthiness helpers are ABSORBED into the single authority; leaving them alongside it would be exactly the duplicate-authority defect this node exists to remove.

The known defect this node must fix is the ruling's finding **D**: "``type Tag = `item-${string}` | "none"`` under a truthiness guard — the checker excludes `""` from the falsy edge; the substrate keeps both arms, clean and warm." A template-literal type with a non-empty literal prefix has NO falsy inhabitant, so it must not survive a falsy edge. The ruling records why D is blocking for D2B at all: "Not because a value is wrong, but because `FlowReturnResult` contractually promises the whole-return node is canonical and D2B-AC2 pins exact identity: extensional equality does not satisfy a public canonical-node contract, and all four warm-admit."

**Fixing the falsy edge does NOT mean dropping a syntactically present return.** The ruling is explicit: "D is a certified falsy-edge fact defect: canonical semantics says the falsy restriction of `Tag` is `never`, while TS inference separately decides that the syntactic numeric return still contributes, so the final declaration may still read `{ v: Tag } | { v: number }`. The fix is not to drop the return." Contributor participation is the **TypeScript-compatible inference** layer's decision — that layer owns "which syntactic contributors participate, conditional/evolving-value widening, alias representative selection, context-specific subtype-to-supertype reunion" — and it "may assume canonical operations and relation facts are authoritative". So a TypeScript-compatible final declaration that legitimately keeps BOTH arms is a CORRECT outcome of this node, not a failure of it. Equally, "Type-impossible contributors stay in raw evidence with provenance: algebraic `never` does not erase evidence, and evidence does not force `never` to remain a union constituent."

This charter accepts one authority boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_semantic/src`, `crates/verter_session/src`.
- Named API/data boundaries: the new `ClassifyTruthinessDomain` demand-scoped query and its `ProjectSemanticDispatch` execution and typed query context; the flow narrowing truthiness frame as the CONSUMER of the fact; and the existing per-arm truthiness helpers, which this node absorbs.
- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **None.** This node is foundational and independent of TA1, so the two are reviewable in parallel. It is an explicit predecessor of D2B.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- The query is DEMAND-SCOPED: it classifies the demanded type, not a whole declaration graph. It "normalizes only the already-demanded semantic portion" and never inlines aliases or whole declaration graphs to reach an answer.
- The returned fact is a truthiness DOMAIN — whether truthy inhabitants exist and whether falsy inhabitants exist — not a narrowed type and not a flow frame. Producing the narrowed arm from the fact stays with the consumer.
- Reads performed while classifying "use canonical dispatch and enter the fact read-set; unknown or budgeted results suppress admission; any policy version affecting output belongs in `ResultContractId` or the typed query context, never a global boolean and never `FlowSliceHash`."
- An undecidable domain is reported as undecided and suppresses admission; it is never guessed in either direction, and a guessed answer that happens to match the checker is a defect.
- The classification must not run in a forbidden normalization position: not before flow slice hashing (`FlowSliceHash` "hashes the selected source graph, skeleton, demand and edges, not semantic result nodes"), not inside raw-evidence storage, not inside `MergedDecl` or the other ordered merge carriers, not after `SealedFlowCompletion` / `CompleteFlowResult` is minted, and not in display.
- Deletion discipline: each absorbed per-arm helper cites the authority route that displaces it; a helper without that proof is preserved rather than silently deleted.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **TA2-AC1 — sole-owner outcome:** `ClassifyTruthinessDomain` is the SOLE truthiness-domain authority; the per-arm helpers are absorbed, not paralleled, and flow holds no private truthiness rule. Prove no second classification path remains reachable from a production flow narrowing frame. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation leg only for a plausible critical fail-closed boundary.
- **TA2-AC2 — positive contract:** the falsy edge of ``type Tag = `item-${string}` | "none"`` under a truthiness guard is `never`, measured through the live public boundary (`get_flow_return_type_with_audit`). The DISCRIMINATING pair: the falsy restriction must reduce, AND the syntactically present return contributor must NOT be dropped — a final declaration that legitimately reads `{ v: Tag } | { v: number }` because TypeScript-compatible inference kept the contributor PASSES, while a run that silently deletes a syntactic return FAILS. A type that genuinely has both truthy and falsy inhabitants (for example a bare `string`, whose `""` inhabitant is falsy) must keep both arms. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **TA2-AC3 — incremental equivalence:** prove incremental equals fresh, that the fact's reads enter the ordinary read-set, and that an undecided or budgeted classification is `ReturnOnly` and never warm-admitted.
- **TA2-AC4 — bounded work:** prove no hidden duplicate parse, resolve, classification, allocation, or retained candidate — classification is demand-scoped, memoized under its own query identity, and never whole-materialises the classified type. Use applicable existing counters, inspection, or benchmarks; otherwise record a terse not-applicable rationale.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `crates/verter_semantic/tests`, `crates/verter_session/tests/cases`.

## Deletions and forbidden designs

- Delete the absorbed per-arm truthiness helpers in the same change, each citing the authority route that displaces it; a helper without that proof is preserved and its retention recorded.
- **Never build a second parallel type system**, and never let this node become a second normalization or relation engine. It classifies a domain; it does not reduce types.
- **Never inline aliases** or whole declaration graphs to reach a classification.
- **Never apply subtype absorption** and **never introduce general mutual-assignability** as the classification mechanism; an undecided domain stays undecided.
- **Never expand into a native-checker or type-system rewrite.** "Scope is bounded to satisfying the existing canonical-node contract and A–D. This is NOT a general native-checker or type-system rewrite."
- Never drop a syntactically present return contributor as a way of satisfying the falsy-edge fix; contributor participation belongs to the TypeScript-compatible inference layer, and the certified fact never erases raw evidence.
- Never place the authority inside the flow evaluator, and never let flow reimplement or shadow it.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, or a global policy boolean.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 600 production LOC, 8 production files, 2 related crates/packages.
- Mandatory rescope above 1,200 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing. An undecided classification never warms.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a soak solely to satisfy this charter.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary — in particular if the per-arm truthiness helpers cannot be absorbed without changing a consumer's public contract — or if the complete diff will not fit one review context.
- Stop if the falsy-edge fix cannot be made without deleting a syntactic return contributor; that is an amendment to the inference layer's ownership, not a local decision here.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_semantic -p verter_session`
2. Run every final command in the bound `targeted-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `semantic-3`: 3 fresh distinct harness tasks covering exactly the profile's assigned lenses. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch adds one `[[implemented]]` row to `authority/state/implemented.toml` with the node ID, planned squash commit message, approximate date with timezone, and optional pull-request number. Row presence is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.
