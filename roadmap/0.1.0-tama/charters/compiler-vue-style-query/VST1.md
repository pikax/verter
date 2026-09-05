<!-- unified-charter-v2
id=VST1
name=Vue selector-to-template query engine
phase=compiler
train=compiler.vue-style-query
product=vue_style_query
kind=implementation
semantic_role=delivery
class=compiler
predecessors=VCP2,VST0
owner=compiler.vue-style-query:indexed selector-to-template query service
conflict_domains=style_semantics,vue_product
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
release_gating=non_release
external_requirements=
charter=charters/compiler-vue-style-query/VST1.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# VST1 — Vue selector-to-template query engine

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

Vue selector-to-template query engine. The current owner is **Vue selector/template scans**. The final and sole owner is **indexed selector-to-template query service**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src/framework/vue`, `crates/verter_semantic/src`.
- Named API/data boundaries: `SelectorQuery`, `TemplateCandidateIndex`, `MatchFact`.
- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **VCP2:** implemented ledger row for “Compact Vue compiler structure and canonical template topology”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **VST0:** implemented ledger row for “Vue framework style semantics and scope plan”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- **Intent:** provide a Vue-owned selector applicability service for tooling and future optimization without taxing default runtime compilation.
- **Problem:** CSS diagnostics/navigation/component analysis need selector-to-template relationships, but Vue default runtime compilation does not require selector pruning and should not pay for it.
- **Solution and architecture decisions:**
- consume J selector structure and VCP2 Vue template topology;

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **VST1-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **VST1-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **VST1-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **VST1-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `crates/verter_compiler/tests`.

## Deletions and forbidden designs

- Delete or structurally reject: **whole-template rescans**.
- Delete or structurally reject: **string-only selector inference**.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 800 production LOC, 8 production files, 2 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing.
- Performance acceptance: use the exact applicable metric rows and methodology from performance-gates.toml or the owning ratified product catalog, under contracts/resource-and-finalization.md (L2). Exact work invariants, statistical latency/RSS limits and bounded new-capability budgets are distinct. Missing required coverage needs an owning-contract amendment before measurement; no implicit 0.0% threshold or post-hoc rebaseline applies.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, an ancestor lacks an implemented ledger row, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_compiler`
2. Run every final command in the bound `targeted-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `semantic-3`: 2 fresh distinct harness tasks covering exactly `adversarial`, `conformance`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 2/2 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `targeted` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's predeclared row in `authority/state/implemented.toml` from `status = "pending"` to `status = "implemented"` with the planned squash commit message, approximate date with timezone, and optional pull-request number. The transitioned row is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.

## Reconciled source-plan contract

**Intent:** provide a Vue-owned selector applicability service for tooling and future optimization without taxing default runtime compilation.

**Problem:** CSS diagnostics/navigation/component analysis need selector-to-template relationships, but Vue default runtime compilation does not require selector pruning and should not pay for it.

**Solution and architecture decisions:**

- consume J selector structure and `VCP2` Vue template topology;
- derive a compact selector query plan only when demanded and cost-effective;
- use adaptive direct versus indexed matching;
- postings use only sound positive anchors; negated predicates never seed candidates;
- dynamic tags/classes/IDs/attributes and spreads enter explicit maybe buckets;
- exact Vue matcher returns `Yes | Maybe | No` and remains authoritative;
- produce `VueStyleMatchFacts` for diagnostics, navigation, component information and future `Optimized` consideration;
- `Default` runtime targets demand none of this work unless a separately locked correctness cell requires it;
- no pruning behavior is admitted by this block.

**Suggested predecessors:** `VCP2`, `VST0`.

**Normative source decomposition:** semantic contract, direct matcher, topology feature index, selector query plan, adaptive cost model, fact/witness publication and performance gates.

**Acceptance:** direct and indexed paths are semantically identical; candidate reduction has no false negatives; dynamic cases remain `Maybe`; default compiler ledgers show zero VST1 work; tooling consumers can request sparse witnesses without production overhead.

**Forbidden:** making VST1 a VCP7 predecessor, universal selector semantics, always building an index, or using `Maybe` to remove CSS.

**Deletion/abort:** no runtime compiler deletion; move shared mechanics only after measured neutral equivalence.

---

