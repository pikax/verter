<!-- unified-charter-v2
id=D3P
name=Product lattice substrate
phase=rev11
train=rev11.flow
product=rev11
kind=implementation
semantic_role=delivery
class=foundational
predecessors=D3R,D3I
owner=rev11.flow:sole shared flow authority
conflict_domains=flowslice
resource_class=rust-mixed
review_profile=architecture-3
gate_profile=targeted-domain
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=high
verification_effort_default=high
confirmation_effort_min=high
confirmation_effort_default=high
size=M
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/rev11-flow/D3P.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# D3P — Product lattice substrate

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

Product lattice substrate (codex D3 scope ruling, `decisions/2026-08-30-rev11-flow-d3-split.md`). The existing `FlowDomain` registry (`crates/verter_session/src/project_semantic_dispatch/flow_solve.rs`), `FlowDemandPlan`, and `FlowDischargeReport` describe obligations and evidence but hold no computed per-binding products and execute no semantic transfers. This node adds one internal product layer: a new `project_semantic_dispatch/flow_products.rs` defining `FlowProductKey`, `FlowProductValue`, `FlowProductStore` (keyed by `(FlowDomain, FlowNodeId)` over the bound `FunctionFlowGraph`, with binding nodes resolving to the stable binding identities D3I delivered), `ReachingValueProduct`, `DefiniteAssignment::{Unassigned, Assigned, MaybeAssigned}`, and `FlowTransferOutcome::{Unchanged, Changed, Gap, BudgetExceeded}`, with one exhaustive `transfer_product` and one exhaustive `join_product` route per live domain. It extends the closed `FlowDomain` registry with `DeclaredType` and `DefiniteAssignment` — no second domain enum; extending the closed registry changes `ResultContractId`, an intentional contract-version change that must exercise D2B's exact-contract tests (`flow_result_contract_is_exact_identity`). The layer is production-compiled but performs NO cutover: the existing `FlowEvaluator` state and `join_layer_states` remain the live value path; replacing them is D3C. The current and final owner is the **sole shared flow authority**: `ProjectSemanticDispatch` / `execute_function_return_source` (`crates/verter_session/src/project_semantic_dispatch/flow_return.rs`) over the flow substrate in `crates/verter_semantic/src/analysis/flow`. This charter accepts one substrate boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_session/src` only.
- Production files: new `project_semantic_dispatch/flow_products.rs`, `project_semantic_dispatch/flow_solve.rs`, `project_semantic_dispatch/dispatch_txn.rs`, `project_semantic_dispatch/mod.rs`.
- Named API/data boundaries: `FlowProductKey`, `FlowProductValue`, `FlowProductStore`, `ReachingValueProduct`, `DefiniteAssignment::{Unassigned, Assigned, MaybeAssigned}`, `FlowTransferOutcome::{Unchanged, Changed, Gap, BudgetExceeded}`, `transfer_product`, `join_product`, the closed `FlowDomain` registry (`DeclaredType`, `DefiniteAssignment` added), `ResultContractId`.
- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **D3R:** implemented ledger row for “Nominal relation authority”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **D3I:** implemented ledger row for “Complete stable binding identity”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- Deliver exactly “Product lattice substrate” as the independently acceptable boundary; no neighboring authority is included.
- Named types (ruling §3): `FlowProductKey`, `FlowProductValue`, `FlowProductStore`, `ReachingValueProduct`, `DefiniteAssignment::{Unassigned, Assigned, MaybeAssigned}`, `FlowTransferOutcome::{Unchanged, Changed, Gap, BudgetExceeded}`, exhaustive `transfer_product` and `join_product` per live domain. Extend the closed `FlowDomain` registry with `DeclaredType` and `DefiniteAssignment`; do not create a second domain enum.
- The layer compiles into production but no evaluator cutover occurs in this node: `FlowEvaluator`'s existing state maps and `join_layer_states` remain the live value path until D3C.
- Discriminating tests (substrate-level legs, all three required): `binding_domain_joins_are_domain_specific` (reaching definitions canonicalize as a set; reaching types union canonically — meaning `join_product` aggregates the flow-domain contributors and asks TA1A's `NormalizeUnion` to construct the semantic result, so product-state algebra is D3P's while semantic type algebra is not (`decisions/2026-08-31-canonical-type-algebra-predecessor.md`); definite assignment uses its declared lattice; narrowing facts survive a join only when valid on every incoming edge; each join is idempotent and permutation-stable); `flow_product_worklist_is_permutation_deterministic` (randomized equivalent edge-insertion/initial-ready order yields identical visitation order, products, discharge evidence, and result bytes at the substrate level); `flow_product_budget_boundary_is_exact_and_never_warm` (substrate-level legs: a solve stabilizing at the cap completes; one requiring another pass returns typed budget exhaustion and retains no candidate).
- Landing: D3R, D3I, D3P, and D3C land as ONE atomic multi-node candidate; none of the four merges independently (codex D3 scope ruling, `decisions/2026-08-30-rev11-flow-d3-split.md`, extending the D1+D2A+D2B atomic-landing pattern of `decisions/2026-08-29-rev11-flow-d2-split.md`). Per `contracts/github-control-plane.md`, each node in the shared candidate keeps its own issue mapping, ledger row, and closing link; D3P intentionally carries no GitHub issue mapping (the pre-existing D3 issue mapping, gh_issue 175, was rekeyed to D3C — the maintainer freeze on issue churn creates no new issues for the substrate nodes).

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **D3P-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected — one product store, one domain registry, one transfer/join route per domain; no second domain enum or private product join. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **D3P-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering — including the intentional `ResultContractId` contract-version change exercised by D2B's exact-contract tests. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **D3P-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm — the budget-exhaustion boundary never retains a warmable candidate. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **D3P-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `crates/verter_semantic/tests`, `crates/verter_session/tests/cases`.

## Deletions and forbidden designs

- Never introduce a second domain enum, a second product store, a private product join, or a public product query; the one-product-join rule is a rule about PRODUCT-STATE joins and is never permission for a flow-private semantic union or intersection reducer — every semantic composite a join produces is constructed through TA1A's canonical algebra; `FlowReturn` remains the only proof-enabled root and `FlowNarrowingAt` / `ContextualTypeAt` remain pending typed gaps.
- Never introduce a second flow engine, planner, or resolver, and never route around the sole dispatch (`ProjectSemanticDispatch` / `execute_function_return_source`). No parallel flow resolver exists to delete — A5 owner rows and the A6 implementation lock record establish `crates/verter_session/src/flow_slice_content.rs` and `crates/verter_semantic/src/analysis/flow` as the single existing flow pipeline.
- Do not touch the residual wrong-complete fallback: the A3 wrong-complete retraction is landed, and the residual non-call fabricated-`any` fallback is recorded debt (RESIDUAL-NON-CALL-ANY-FABRICATION) owned by U6.VALUE_INFERENCE (the shallow pass's per-expression fallback) plus U6.ASYNC_GENERATOR and U6.CALL_RESOLVE (`await x`) — not by D6 or D8 (debt record, `crates/verter_session/tests/cases/manifest_data/typeinfo_guard_registry.rs:599`).
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity. No shadow result comparison: the substrate is added dark and the cutover in D3C swaps authority atomically, never by running old and new side by side.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 800 production LOC, 8 production files, 2 related crates/packages (ruling target: 450–650 production LOC, at most 5 files, `verter_session` only).
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, an ancestor lacks an implemented ledger row, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_semantic -p verter_session`
2. Run every final command in the bound `targeted-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch adds one `[[implemented]]` row to `authority/state/implemented.toml` with the node ID, planned squash commit message, approximate date with timezone, and optional pull-request number. Row presence is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.
