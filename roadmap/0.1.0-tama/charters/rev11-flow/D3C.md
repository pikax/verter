<!-- unified-charter-v2
id=D3C
name=Product worklist cutover
phase=rev11
train=rev11.flow
product=rev11
kind=implementation
semantic_role=delivery
class=foundational-atomic
predecessors=D3P
owner=rev11.flow:sole shared flow authority
conflict_domains=flowslice,semantic_authority
resource_class=rust-mixed
review_profile=public-3
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
charter=charters/rev11-flow/D3C.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# D3C — Product worklist cutover

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

Product/worklist cutover (codex D3 scope ruling, `decisions/2026-08-30-rev11-flow-d3-split.md`) — one indivisible cutover that moves the live value path of the flow evaluator onto D3P's product lattice. The `FlowEvaluator` today stores semantic state in separate `FxHashMap<String, …>`/`FxHashSet<String>` layers, parameter ordinals, and a name-rooted narrowing overlay, and `join_layer_states` is a FlowReturn-private product join. This node: (1) replaces the evaluator's `locals`, `var_locals`, declared-type maps, parameter-write maps, conditional-definition sets, and the narrowing overlay with `FlowProductStore`; (2) replaces `join_layer_states` with the domain joins; (3) executes selected transfers in `FlowDemandPlan` order until stable, using `max_iterations` and the selected obligation frontier as the connected budget — the existing `FlowTieBreak::DomainNodeEdgeSlot` and `FlowConvergencePolicy` order the worklist, and work exhaustion feeds the existing typed budget/ledger/finalizer path (no private constant or parallel budget authority); (4) flows product-domain discharge evidence through the existing `FlowDischargeReport`/finalizer, so a complete result requires product evidence; (5) deletes all runtime semantic state keyed by `String`; (6) rehomes the already-supported narrowing state onto the narrowing product without enlarging D4 semantics; and (7) lets literal-widening provenance ride the reaching-type product to preserve current behavior (it is not D5's capture-creation freshness/invalidation domain). This node adds NO public product query: `FlowReturn` remains the only proof-enabled root, `FlowNarrowingAt` and `ContextualTypeAt` remain pending typed gaps, and `ResolveCallKey.flow` remains sealed-empty. The current and final owner is the **sole shared flow authority**: `ProjectSemanticDispatch` / `execute_function_return_source` (`crates/verter_session/src/project_semantic_dispatch/flow_return.rs`) over the flow substrate in `crates/verter_semantic/src/analysis/flow`. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_session/src` only.
- Production files: `project_semantic_dispatch/flow_return.rs`, `project_semantic_dispatch/flow_products.rs`, `project_semantic_dispatch/flow_solve.rs`, `flow_slice_content.rs`, and optionally `project_semantic_dispatch/dispatch_txn.rs` if final evidence plumbing was not completed in D3P.
- Named API/data boundaries: `FlowProductStore`, `transfer_product`, `join_product`, `FlowDemandPlan`, `FlowDischargeReport`, `FlowConvergencePolicy`, `FlowTieBreak::DomainNodeEdgeSlot`, `max_iterations`, `finalize_flow_solve` (D2B's sole positive authority — assumed, never repaired or recreated here).
- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **D3P:** implemented ledger row for “Product lattice substrate”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- Deliver exactly “Product worklist cutover” as the independently acceptable boundary; no neighboring authority is included.
- Named work (ruling §3): replace `locals`, `var_locals`, declared-type maps, parameter-write maps, conditional-definition sets, and the narrowing overlay with `FlowProductStore`; replace `join_layer_states` with domain joins; execute selected transfers in `FlowDemandPlan` order until stable; use `max_iterations` and the selected obligation frontier as the connected budget; emit product-domain discharge evidence through D2B's report; delete all runtime semantic state keyed by `String`.
- D3C must not repair proof provenance, recreate admission predicates, or construct `CompleteFlowResult` directly — D2B's proof/admission contract is assumed intact.
- Discriminating tests (all required): `flow_discharge_requires_product_evidence` (omitting one required binding-domain product from an otherwise clean evaluation cannot mint `CompleteFlowResult`, at either root or SCC publication); `flow_product_worklist_is_permutation_deterministic` (end-to-end legs: identical visitation order, products, discharge evidence, result bytes, and warm candidate under randomized equivalent order); `flow_product_budget_boundary_is_exact_and_never_warm` (end-to-end legs: typed budget exhaustion retains no candidate and recomputes cold); and the successor-boundary controls — existing `GuardNarrowing`, `ClosureCapture`, and `AbruptCompletion` fixtures unrelated to nominal comparability must remain typed partial/cold; D3 must not make D4/D5/D6 tests pass by widening its scope.
- Landing: D3R, D3I, D3P, and D3C land as ONE atomic multi-node candidate; none of the four merges independently (codex D3 scope ruling, `decisions/2026-08-30-rev11-flow-d3-split.md`, extending the D1+D2A+D2B atomic-landing pattern of `decisions/2026-08-29-rev11-flow-d2-split.md`). The normal `contracts/github-control-plane.md` rule gives each mapped node its own issue and closing link. This maintainer-approved atomic candidate is the explicit exception: D3R, D3I, and D3P are intentionally unmapped substrate nodes, while D3C alone carries the rekeyed pre-existing D3 mapping (gh_issue 175). All four retain distinct implementation-ledger rows; only mapped D3C receives a closing link.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **D3C-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected — no runtime semantic state keyed by `String` survives, and `join_layer_states` is gone. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **D3C-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering — one deterministic worklist, product evidence feeding the existing report/finalizer, and unchanged D2B admission semantics. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **D3C-AC3 — incremental equivalence:** prove incremental equals fresh and degraded outcomes cannot warm — budget exhaustion and missing product evidence never mint `CompleteFlowResult` and recompute cold. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **D3C-AC4 — bounded work:** prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate — one connected budget (`max_iterations` plus the selected obligation frontier), no private constant or parallel budget authority — using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `crates/verter_semantic/tests`, `crates/verter_session/tests/cases`.

## Deletions and forbidden designs

- Delete the evaluator's `String`-keyed runtime semantic state (`locals`, `var_locals`, declared-type maps, parameter-write maps, conditional-definition sets, the name-rooted narrowing overlay) and the FlowReturn-private `join_layer_states` — each cites the displaced product-store/domain-join route that replaces it.
- Never introduce a second flow engine, planner, or resolver, and never route around the sole dispatch (`ProjectSemanticDispatch` / `execute_function_return_source`); the cutover must prove no second selectable evaluator remains. No shadow result comparison: the cutover swaps the value path atomically, never by running old and new side by side.
- Do not touch the residual wrong-complete fallback: the A3 wrong-complete retraction is landed, and the residual non-call fabricated-`any` fallback is recorded debt (RESIDUAL-NON-CALL-ANY-FABRICATION) owned by U6.VALUE_INFERENCE (the shallow pass's per-expression fallback) plus U6.ASYNC_GENERATOR and U6.CALL_RESOLVE (`await x`) — not by D6 or D8 (debt record, `crates/verter_session/tests/cases/manifest_data/typeinfo_guard_registry.rs:599`).
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity. Never add a public product query: `FlowReturn` remains the only proof-enabled root; `FlowNarrowingAt` / `ContextualTypeAt` remain pending; `ResolveCallKey.flow` remains sealed-empty.
- Do not add capture, effect, loop, completion, contextual-typing, or new narrowing behavior merely because those variants already appear in the D1 registry — those are D4–D7's named scopes. Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and rescope

- Planning reference: 800 production LOC, 8 production files, 2 related crates/packages (ruling estimate: 600–800 production LOC, at most 6 files, `verter_session` only).
- Numeric rescope signal: 1,500 production LOC or 12 files. Crossing it requires a scope-coherence investigation under `contracts/sizing.md`, not automatic rescope.
- Architect rescope remains mandatory when the candidate spans 3 unrelated crates/packages, or combines public/wire, unsafe, concurrency, or lifetime work with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing.
- Performance acceptance: use the exact applicable metric rows and methodology from performance-gates.toml or the owning ratified product catalog, under contracts/resource-and-finalization.md (L2). Exact work invariants, statistical latency/RSS limits and bounded new-capability budgets are distinct. Missing required coverage needs an owning-contract amendment before measurement; no implicit 0.0% threshold or post-hoc rebaseline applies.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, an ancestor lacks an implemented ledger row, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_semantic -p verter_session`
2. Run every final command in the bound `targeted-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `public-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `wire-public`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's
predeclared row in `authority/state/implemented.toml` from `status = "pending"`
to `status = "implemented"` with the planned squash commit message, approximate
date with timezone, and optional pull-request number. The transitioned row is the
implementation fact. Commit metadata is a loose locator only and is never resolved or
validated against Git or GitHub. Reviewers inspect the squashed candidate patch without
SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.
