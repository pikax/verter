<!-- unified-charter-v2
id=BR0
name=Post-L4 successor product promotion
phase=governance
train=governance.governance
product=governance
kind=genesis
semantic_role=delivery
class=successor
predecessors=L4
owner=governance.governance:static DAG authority plus implemented-ledger rows and ordinary review evidence
conflict_domains=program_authority
resource_class=docs-light
review_profile=semantic-3
gate_profile=docs-domain
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=high
verification_effort_default=high
confirmation_effort_min=high
confirmation_effort_default=high
size=S
dispatchable=true
optional=false
release_gating=external
external_requirements=maintainer_rev11_repair_freeze_lift,maintainer_successor_genesis
charter=charters/governance-governance/BR0.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# BR0 — Post-L4 successor product promotion

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

Coordinate post-L4 successor promotion using the static DAG and trusted implementation ledger. The node's maintainer requirements remain planning instructions, not machine-validated authorization records.

## Concrete surfaces and APIs

- Production surfaces: `roadmap/0.1.0-tama`.
- Named API/data boundaries: `ImplementationLedger`, `programctl frontier`, and maintainer promotion coordination.
- Mutation boundary: authority/evidence bytes only; production LOC is zero.

## Exact predecessor contracts

- **L4:** implemented ledger row for “Final architecture lock”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirement maintainer_rev11_repair_freeze_lift:** agents obtain the maintainer decision; tooling does not validate it.
- **External requirement maintainer_successor_genesis:** agents obtain the maintainer decision; tooling does not validate it.

## Source-specific scope

- Deliver exactly “Post-L4 successor product promotion” as the independently acceptable boundary; no neighboring authority is included.
- Agents confirm `maintainer_rev11_repair_freeze_lift` and `maintainer_successor_genesis`; no authorization manifest or digest is created.
- BR0 is a shared ordinary predecessor, not a join of product completion. Each product terminal remains independently promotable once all of its transitive ancestor rows exist.
- Negative controls prove product-gated nodes remain blocked before the BR0 row and otherwise-independent product terminals can become READY concurrently afterward.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **BR0-AC1 — ownership contract:** enumerate every in-scope outcome, consumer and displaced production route. Each outcome/consumer has exactly one implementation owner; each displaced production route has exactly one later production-capable deletion/rejection owner. Bind owners to existing DAG node IDs, valid successor paths and receiving acceptance IDs under contracts/successor-charter-quality.md. The contract-owned schema/validator must reject missing inventory members, unknown/pathless owners and conflicting assignments before this node completes; production deletion remains acceptance of the later owner.
- **BR0-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **BR0-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **BR0-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `roadmap/0.1.0-tama/tools`.

## Deletions and forbidden designs

- Inventory and assign the later deletion/rejection owner for: **authorization receipts, commit/tree pins, digest chains, and mutable READY state**. This node changes no production route.
- Inventory and assign the later deletion/rejection owner for: **resource-capacity DAG edges**. This node changes no production route.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 0 production LOC, 0 production files, 0 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing.
- Performance acceptance: use the exact applicable metric rows and methodology from performance-gates.toml, the applicable MEM0 budget, or the owning ratified product catalog, under contracts/resource-and-finalization.md (L2). Exact work invariants, latency/allocation/RSS limits under their owning methodology, and bounded new-capability budgets are distinct. New capabilities and deliberate pressure policies declare bounded new work and replacement SLOs before measurement. Missing required coverage needs an owning-contract amendment before measurement; no implicit 0.0% threshold or post-hoc rebaseline applies.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, an ancestor lacks an implemented ledger row, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `node --test roadmap/0.1.0-tama/tools/implementation-ledger.test.mjs`
2. Run every final command in the bound `docs-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `semantic-3`: 2 fresh distinct harness tasks covering exactly `adversarial`, `conformance`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 2/2 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `targeted` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's predeclared row in `authority/state/implemented.toml` from `status = "pending"` to `status = "implemented"` with the planned squash commit message, approximate date with timezone, and optional pull-request number. The transitioned row is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.

## Current promotion contract

The static BR0 node records the post-L4 promotion boundary. After the L4 ledger row exists, agents confirm the two named maintainer requirements, implement the charter, add the BR0 ledger row before squash/review, run the owning review and docs gate, and land normally. The BR0 row then unlocks product-gated descendants. No SHA, tree, receipt, digest, or GitHub lookup participates in this transition.

## Collapsed non-authoritative subblock disposition

The recovery candidate mechanically split this source-owned atomic node into the following labels: `BR0P`. They have no separate dispatch, lease, receipt, migration manifest, deletion ownership, or review standing. Their useful source-described concerns are internal RED/GREEN checklist items of **BR0**; BR0 alone owns the complete migration population, exactly one final deletion/cutover, and atomic acceptance. Any quoted “suggested subblock” wording in transferred source text is non-authoritative planning context.
