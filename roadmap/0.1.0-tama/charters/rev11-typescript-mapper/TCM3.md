<!-- unified-charter-v2
id=TCM3
name=TypeScript semantic capability closure (dormant until TCM4)
phase=rev11
train=rev11.typescript-mapper
product=rev11
kind=implementation
semantic_role=delivery
class=framework subsystem
predecessors=TCM0R,TCM1,G2
owner=rev11.typescript-mapper:ratified dual-plane mapper/snapshot/oracle identity contract
conflict_domains=semantic_authority,capability_catalog
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
charter=charters/rev11-typescript-mapper/TCM3.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# TCM3 — TypeScript semantic capability closure (dormant until TCM4)

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

TypeScript semantic capability closure (dormant until TCM4). The current owner is **rejected TCM0 closure package and string mapper plane**. The final and sole owner is **ratified dual-plane mapper/snapshot/oracle identity contract**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_type_runtime/src`, `crates/verter_session/src`, `packages/typescript-plugin/src`, `crates/verter_protocol/src`.
- Named API/data boundaries: `CertifiedTypeEngineBinding`, `InputBasisId`, `QueryIdentity`, `SemanticFlightKey`, `ContentMapper`.
- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **TCM0R:** implemented ledger row for “TypeScript dual-plane architecture and observation-identity rescope”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **TCM1:** implemented ledger row for “Compact mapping products inside CodeTransform”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **G2:** implemented ledger row for “FlightCell-owned same-key production”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- Deliver exactly “TypeScript semantic capability closure (dormant until TCM4)” as the independently acceptable boundary; no neighboring authority is included.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **TCM3-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **TCM3-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **TCM3-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **TCM3-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `crates/verter_type_runtime/tests`, `crates/verter_session/tests/cases`.

## Deletions and forbidden designs

- Delete or structurally reject: **self-certified closure status**.
- Delete or structurally reject: **tracked Python/POSIX control**.
- Delete or structurally reject: **mapper callback into semantic oracle**.
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

1. `cargo nextest run -p verter_type_runtime -p verter_session -p verter_protocol`
2. Run every final command in the bound `targeted-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `semantic-3`: 2 fresh distinct harness tasks covering exactly `adversarial`, `conformance`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 2/2 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `targeted` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's predeclared row in `authority/state/implemented.toml` from `status = "pending"` to `status = "implemented"` with the planned squash commit message, approximate date with timezone, and optional pull-request number. The transitioned row is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.

## TCM0 remediation receiving criteria

- `TCM3-RC-HANG-TOPOLOGY`: rerun the bounded attach/hang probe before selecting editor-attached topology. A non-occurrence is bounded evidence only; never call it reproduction or absence proof. Bind `TCM0-R-HANG-TOPOLOGY`.
- `TCM3-RC-SEMANTIC-TOPOLOGY`: select semantic topology using TCM0's locked candidates, metrics, harness, and selection rule; bind `TCM0-R-TOPOLOGY-SELECTION`.
- `TCM3-RC-IMPLEMENTATION-BASELINE`: capture current-path comparison evidence before semantic-path changes; bind `TCM0-R-IMPLEMENTATION-BASELINE` without machine-specific absolute thresholds.
- Own the actual project-wide-reference and auto-import compositions, including aliases/reexports, plus the applicable position-clamping test. Accumulate introduced/orphaned records and require direct stable-ID/digest equality.

