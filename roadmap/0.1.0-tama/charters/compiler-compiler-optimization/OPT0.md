<!-- unified-charter-v2
id=OPT0
name=Compiler optimization engine rescope and maintainer ratification
phase=compiler
train=compiler.compiler-optimization
product=compiler_optimization
kind=rescope
semantic_role=delivery
class=compiler
predecessors=CMP6,CPER3
owner=compiler.compiler-optimization:maintainer-ratified separately measured optimization engine scope
conflict_domains=compiler_execution
resource_class=docs-light
review_profile=architecture-3
gate_profile=docs-domain
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=high
verification_effort_default=high
confirmation_effort_min=high
confirmation_effort_default=high
size=M
dispatchable=false
optional=false
release_gating=non_release
external_requirements=
charter=charters/compiler-compiler-optimization/OPT0.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# OPT0 — Compiler optimization engine rescope and maintainer ratification

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

Compiler optimization engine rescope and maintainer ratification. The current owner is **deferred optimization proposal**. The final and sole owner is **maintainer-ratified separately measured optimization engine scope**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src`, `crates/verter_bench/benches`.
- Named API/data boundaries: `OptimizationPlan`, `OptimizationProof`.
- Mutation boundary: authority/evidence bytes only; production LOC is zero.

## Exact predecessor contracts

- **CMP6:** implemented ledger row for “Cross-framework compiler-engine falsification”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **CPER3:** implemented ledger row for “Cross-framework compiler soak and equivalent-work study”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- **Intent:** reserve the future optimization-engine decision point while explicitly preventing premature implementation.
- **Problem:** project-wide provenance, declaration/implementation inspection, proof/evidence storage, cost models and fallback policy may improve generated output, but designing a generalized engine now would be speculative and could delay correct default compi
- **Suggested predecessors:** CMP6, CPER3.
- **Required input for future rescope:** a maintainer-provided or maintainer-approved dedicated plan that addresses at least:

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **OPT0-AC1 — ownership contract:** every inventoried outcome, consumer and displaced route has exactly one declared implementation/deletion owner. Validate contract artifacts and their negative controls; production deletion is acceptance of the later owner, not this documentation-only node.
- **OPT0-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **OPT0-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **OPT0-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `crates/verter_compiler/tests`, `crates/verter_bench`.

## Deletions and forbidden designs

- Inventory and assign the later deletion/rejection owner for: **optimization hidden in Default**. This node changes no production route.
- Inventory and assign the later deletion/rejection owner for: **benchmark-specific code paths**. This node changes no production route.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 0 production LOC, 0 production files, 0 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing.
- Performance acceptance: use the exact applicable metric rows and methodology from performance-gates.toml or the owning ratified product catalog, under contracts/resource-and-finalization.md (L2). Exact work invariants, statistical latency/RSS limits and bounded new-capability budgets are distinct. Missing required coverage needs an owning-contract amendment before measurement; no implicit 0.0% threshold or post-hoc rebaseline applies.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, an ancestor lacks an implemented ledger row, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_compiler -p verter_bench`
2. Run every final command in the bound `docs-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's predeclared row in `authority/state/implemented.toml` from `status = "pending"` to `status = "implemented"` with the planned squash commit message, approximate date with timezone, and optional pull-request number. The transitioned row is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.

## Reconciled source-plan contract

**Status:** `RESCOPE_REQUIRED`; no implementation authority; no `OPT1+` block may be created from this proposal.

**Intent:** reserve the future optimization-engine decision point while explicitly preventing premature implementation.

**Problem:** project-wide provenance, declaration/implementation inspection, proof/evidence storage, cost models and fallback policy may improve generated output, but designing a generalized engine now would be speculative and could delay correct default compilers.

**Suggested predecessors:** `CMP6`, `CPER3`.

**Required input for future rescope:** a maintainer-provided or maintainer-approved dedicated plan that addresses at least:

- precise optimization goals and measurable benefit;
- Verter-native analysis only (`verter_analysis`, `type_info`, resolver);
- internal analysis-depth strategy behind public `Optimized`;
- `OptimizationRequestBasis` versus `OptimizationObservationSet`;
- exact read-set validation, invalidation, cancellation and budgets;
- evidence/provenance representation and whether a generalized proof system is justified;
- deterministic fallback to `Default`;
- artifact identity and reproducibility;
- security, filesystem/package boundaries and RSS;
- per-framework target admission;
- independent benchmarks proving compile-cost versus runtime/code-size benefit.

**Acceptance:** only a newly ratified plan and DAG amendment can close `OPT0` and create successors.

**Forbidden:** code, “temporary” project traversal, enabling `Optimized`, generic certificate/proof engines, or using ambient LSP facts.

**Deletion/abort:** none; remain `RESCOPE_REQUIRED` until maintainer action.

---

