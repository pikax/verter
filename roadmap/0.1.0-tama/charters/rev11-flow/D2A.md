<!-- unified-charter-v2
id=D2A
name=Canonical flow demand and proof substrate
phase=rev11
train=rev11.flow
product=rev11
kind=implementation
semantic_role=delivery
class=foundational-private-checkpoint
predecessors=D1
owner=rev11.flow:sole shared flow authority
conflict_domains=flowslice
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
charter=charters/rev11-flow/D2A.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# D2A — Canonical flow demand and proof substrate

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

Canonical flow demand and proof substrate that compiles into production but remains publicly unreachable, standing on D1's repaired private foundation (codex D2 scope ruling, `decisions/2026-08-29-rev11-flow-d2-split.md`). D1's repair (`62c0fabbd`) already delivered the store-minted `BoundFlowGraph` graph identity, the complete contract semantics hashed into `ResultContractId`, the obligation budget enforced at insertion, and the sealed `SealedFlowCompletion` artifact the finalizer consumes — this node builds on that substrate and redoes none of it. This node owns: (1) un-gating `flow_solve` and the `ObligationRuntime` flow state into production compilation (`EnabledHermetic` renamed `Enabled`; test fixtures and public test re-exports stay gated); (2) replacing the singleton `flow_basis + flow_obligations` ledger with a zero-capacity `Vec<InstalledFlowDemand>` indexed by an unforgeable `FlowDemandHandle`, with `flow_frame_open` storing the handle and popped pending members retaining it until the component closes; (3) a contract-bearing `FlowReturnKey` whose `result_contract: ResultContractId` is derived only in `flow_return_key_with_demand` — caller-selected contract IDs leave the production `FlowDemandRequest`; (4) ONE retained structural plan per cold demand — `FlowSliceHashOutcome::Planned` carries `Arc<PlannedFlowSlice>`, the lowered-body node reuses that plan via `lower_slice_plan`, and `build_flow_demand_plan` accepts the already-produced `ReturnSlicePlan`, deleting both duplicate `ReturnPathPeeker` replans; and (5) the convergence and evidence carrier types (`FlowDischargeReport` on `FlowEvaluationOutcome`, `FlowConvergenceEvidence { policy, iterations, stable }`) that D2B wires to real behavior. The no-flow allocation contract holds: a default `Vec` reserves no heap storage, ordinary queries and pending typed-gap roots install no demand, and the zero-capacity assertion (`crates/verter_session/tests/cases/flow_solve_completeness.rs`) is expanded to exercise real production dispatch. This node adds NO public admission route and NO shadow result comparison; production admission behavior is unchanged by this node alone. The current and final owner is the **sole shared flow authority**: `ProjectSemanticDispatch` / `execute_function_return_source` (`crates/verter_session/src/project_semantic_dispatch/flow_return.rs`) over the flow substrate in `crates/verter_semantic/src/analysis/flow`. This charter accepts one substrate boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_semantic/src`, `crates/verter_session/src`.
- Named API/data boundaries: `ObligationRuntime` (`crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs`) extended with `Vec<InstalledFlowDemand>` / `FlowDemandHandle`; `FlowReturnKey` / `flow_return_key_with_demand` (`crates/verter_session/src/semantic_query.rs`, `crates/verter_session/src/project_semantic_dispatch/flow_return.rs`) carrying `ResultContractId` (`crates/verter_identity/src/identity.rs`); `FlowSliceHashOutcome::Planned` / `Arc<PlannedFlowSlice>` / `FlowSliceLoweredBodyNode` (`crates/verter_session/src/cache_runtime/flow_slice_node.rs`); `build_flow_demand_plan` taking the retained `ReturnSlicePlan` (`crates/verter_session/src/project_semantic_dispatch/flow_solve.rs`); and the `FlowDischargeReport` / `FlowConvergenceEvidence` carriers. The earlier `FlowSlice` / `ResultContract` names were disproven by source and are retracted; a live admission guard (`component_meta_flow_return_admission_tests.rs`) already forbids the string `FlowSlice` from leaking into published type surfaces.
- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **D1:** implemented ledger row for “Private sole-solver foundation checkpoint”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- Deliver exactly “Canonical flow demand and proof substrate” as the independently acceptable boundary; no neighboring authority is included.
- The substrate is production-compiled but publicly unreachable: it publishes no new public/wire surface, opens no admission route, and runs no shadow comparison against the live evaluator's results. Public cutover is D2B's boundary, not this node's.
- Planning runs exactly once per cold FlowReturn demand: delete the lowered-node `ReturnPathPeeker` replan and D1's independent proof-plan replan, each citing the retained `Arc<PlannedFlowSlice>` plan as the source-verified displaced route.
- Landing: D2A never merges independently. It lands only as part of the atomic D1+D2A+D2B multi-node candidate, with internal D1 and D2A checkpoints inside that candidate (codex D2 scope ruling, `decisions/2026-08-29-rev11-flow-d2-split.md`). Per `contracts/github-control-plane.md`, D2A is the intentional unmapped exception; all three nodes keep distinct ledger rows, D1 and D2B retain their own issue mappings, and only those mapped nodes carry closing links.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **D2A-AC1 — sole-owner outcome:** the sole dispatch (`ProjectSemanticDispatch` / `execute_function_return_source`) and the existing flow substrate remain the only production authorities; the singleton ledger and `DemandAlreadyInstalled` model are structurally deleted, not bypassed; and no second graph, relation, or plan authority is introduced. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **D2A-AC2 — positive contract:** `ResultContractId` is exact production identity — two otherwise identical `FlowReturnKey`s with different contract IDs compare unequal, and the key derives the contract only through `flow_return_key_with_demand`. Per-demand handles discriminate nested frames and deferred SCC members. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **D2A-AC3 — incremental equivalence:** one cold FlowReturn demand plans exactly once (`ReturnPathPeeker::plan` counted once; lowering and demand-plan assembly add zero); a clean warm replay adds zero plans. Degraded outcomes remain non-warm through the unchanged legacy admission path until D2B; this node changes no admission behavior.
- **D2A-AC4 — bounded work:** the no-flow path allocates no graph, no plan, and no demand capacity — an ordinary non-flow query and each pending typed-gap root install zero obligations; the zero-capacity assertion is expanded to exercise real production dispatch, not merely `ObligationRuntime::default()`. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `crates/verter_semantic/tests`, `crates/verter_session/tests/cases`.

## Deletions and forbidden designs

- Delete only source-verified displaced responsibilities, each citing the displaced route it replaces; absence of proof means preserve. In this node that set is exactly: the singleton `flow_basis + flow_obligations` ledger and the `DemandAlreadyInstalled` model (displaced by `Vec<InstalledFlowDemand>` / `FlowDemandHandle`), and the two duplicate `ReturnPathPeeker` replans (displaced by the retained `Arc<PlannedFlowSlice>` plan).
- Never introduce a second flow engine, planner, or resolver, and never route around the sole dispatch (`ProjectSemanticDispatch` / `execute_function_return_source`). The memoized `FlowGraphBundle` remains the graph authority; `crates/verter_session/src/flow_slice_content.rs` is part of the existing flow substrate and is preserved in place (ruling §4).
- No public reachability and no shadow comparison: D2A is a private checkpoint that lands only inside the atomic D1+D2A+D2B candidate; nothing it builds is reachable from product entry points, and it never runs the new substrate alongside the live evaluator to compare results.
- Do not touch the residual wrong-complete fallback: the A3 wrong-complete retraction is landed, and the residual non-call fabricated-`any` fallback is recorded debt (RESIDUAL-NON-CALL-ANY-FABRICATION) owned by U6.VALUE_INFERENCE (the shallow pass's per-expression fallback) plus U6.ASYNC_GENERATOR and U6.CALL_RESOLVE (`await x`) — not by D6 or D8 (debt record, `crates/verter_session/tests/cases/manifest_data/typeinfo_guard_registry.rs:599`).
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 800 production LOC, 8 production files, 2 related crates/packages.
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

Apply `semantic-3`: 2 fresh distinct harness tasks covering exactly `adversarial`, `conformance`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 2/2 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `targeted` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's
predeclared row in `authority/state/implemented.toml` from `status = "pending"`
to `status = "implemented"` with the planned squash commit message, approximate
date with timezone, and optional pull-request number. The transitioned row is the
implementation fact. Commit metadata is a loose locator only and is never resolved or
validated against Git or GitHub. Reviewers inspect the squashed candidate patch without
SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.
