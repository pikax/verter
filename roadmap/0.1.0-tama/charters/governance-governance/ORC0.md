<!-- unified-charter-v2
id=ORC0
name=Trusted implementation-ledger cutover
phase=governance
train=governance.governance
product=governance
kind=activation
semantic_role=delivery
class=governance
predecessors=C1,J1
owner=governance.governance:static DAG authority plus implemented-ledger rows and ordinary review evidence
conflict_domains=program_authority
resource_class=docs-light
review_profile=public-3
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
release_gating=none
external_requirements=maintainer_unified_v2_activation
charter=charters/governance-governance/ORC0.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# ORC0 — Trusted implementation-ledger cutover

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

This live charter intentionally supersedes the heavier control plane originally landed by ORC0. The triggering failure, maintainer ruling, and tradeoffs are preserved in `../../decisions/2026-08-28-trusted-implementation-ledger.md`.

## Independently acceptable outcome

Replace the SHA/tree/receipt lifecycle with the trusted implementation ledger. Static DAG and charter files define work; `authority/state/implemented.toml` alone defines which nodes are complete.

## Concrete surfaces and APIs

- Production surfaces: `roadmap/0.1.0-tama/authority`, `roadmap/0.1.0-tama/tools`, and owning orchestration documentation.
- Named API/data boundaries: `ImplementationLedger`, `commit_message`, `commit_date`, optional `pull_request`, and ancestor-row readiness.
- Mutation boundary: authority/evidence bytes only; production LOC is zero.

## Exact predecessor contracts

- **C1:** implemented ledger row for “ModuleResolverCore convergence and non-flow semantic basis”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **J1:** implemented ledger row for “CSS owner reconciliation”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirement maintainer_unified_v2_activation:** agent/maintainer coordination only; tooling does not validate it.

## Source-specific scope

- Delete the old lifecycle machinery and migrate implemented nodes to ledger rows.
- Make locator fields deliberately unvalidated and keep the ledger row inside the implementation patch before squash and review.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **ORC0-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **ORC0-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **ORC0-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **ORC0-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `roadmap/0.1.0-tama/tools`.

## Deletions and forbidden designs

- Delete or structurally reject: **commit-SHA/tree identity validation, receipts, runtime admission, leases, activation journals, and post-landing restamping**.
- Delete or structurally reject: **a second GitHub-backed implementation-state database**.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 0 production LOC, 0 production files, 0 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero completion source other than ledger-row presence and zero validation of locator hints.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, an ancestor lacks an implemented ledger row, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `node --test roadmap/0.1.0-tama/tools/implementation-ledger.test.mjs`
2. Run every final command in the bound `docs-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `public-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `wire-public`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's predeclared row in `authority/state/implemented.toml` from `status = "pending"` to `status = "implemented"` with the planned squash commit message, approximate date with timezone, and optional pull-request number. The transitioned row is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.
