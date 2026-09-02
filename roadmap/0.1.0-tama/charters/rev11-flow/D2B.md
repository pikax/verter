<!-- unified-charter-v2
id=D2B
name=Atomic public flow-proof cutover and distributed-admission retirement
phase=rev11
train=rev11.flow
product=rev11
kind=implementation
semantic_role=delivery
class=foundational-atomic
predecessors=D2A,TA1B,TA2,D2D
owner=rev11.flow:sole shared flow authority
conflict_domains=public_protocol,semantic_authority
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
charter=charters/rev11-flow/D2B.md
max_production_loc=3262
max_production_files=20
max_related_packages=1
rescope_loc=3262
rescope_files=20
rescope_unrelated_packages=3
-->

# D2B — Atomic public flow-proof cutover and distributed-admission retirement

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

Atomic public flow-proof cutover — one indivisible accepted cutover per the flow-completeness contract §6 (historical evidence at `0e121f964`), standing on D1's repaired private foundation and D2A's canonical demand/proof substrate (codex D2 scope ruling, `decisions/2026-08-29-rev11-flow-d2-split.md`). The existing flow-return evaluator remains the sole value-computation backend; `flow_solve`'s finalizer becomes the sole positive authority allowed to mint warm-admissible flow results, while storage safety remains a negative veto fence that may never promote a proofless result. The indivisible cutover steps are: (1) the evaluator emits a typed `FlowDischargeReport` — which planned domains, graph facts, calls, and relations it actually completed — applied centrally through `ObligationRuntime::apply_flow_discharge_report(handle, report)` in deterministic `FlowDemandPlan::work_order`, with structural obligations discharging only against the exact retained selection/lowered IR and call/relation obligations discharging only from their existing typed decided results (`AdmissibleCallResult`; relation `Unknown`/`BudgetExceeded` already non-admissible); (2) finalization wiring — `discharge_flow_component_to_fixed_point` returns real `FlowConvergenceEvidence { policy, iterations, stable }`, finalization runs only after the component fixed point, literal widening, and per-key substitution, every member finalizes before constructing `CompletedFlowReturnMember` or returning the root, and the outcome preserves the normative three-way result `Complete(CompleteFlowResult)` / `Partial { value, proof }` / `NoValue { failure, proof }` so degraded successes stay usable by tolerant consumers; `CompleteFlowResult` remains privately constructible only inside `finalize_flow_solve`; (3) root proof admission — `QueryBuildOutput.flow_completion: Option<CompleteFlowResult>` is required by the family memo whenever the prepared key is `FlowReturn`; (4) SCC proof admission — `CompletedFlowReturnMember.result` and `PendingFlowReturnMember.result` carry `CompleteFlowResult`, and `publish_scc_members_fenced` extracts the payload from the token while retaining root-witness fencing, atomic member publication, capacity checks, and flight retirement; (5) pending typed gaps — `FlowNarrowingAt` and `ContextualTypeAt` route through `flow_solve::typed_gap_for_pending_root` (validating the registry row, emitting the operation-specific `FlowGap`, mapping to `Error(Miss)`, marking partial/ReturnOnly, never building a graph or plan), replacing the untyped generic `Miss`; and (6) deletion of all three distributed `FlowReturnResult` admission channels, each citing its source-verified displaced route (below). The cutover preserves every ratified Supported/Stable capability and failure contract and proves no second selectable evaluator remains. `crates/verter_session/src/flow_slice_content.rs` is PRESERVED in place (ruling §4): it is the content half of the existing substrate — graph reachability selects the slice elsewhere while this module lowers only selected expression content — the convergence occurs between `flow_solve` and `cache_runtime/flow_slice_node.rs`, and no replacement for its value-lowering responsibility exists. The current and final owner is the **sole shared flow authority**: `ProjectSemanticDispatch` / `execute_function_return_source` (`crates/verter_session/src/project_semantic_dispatch/flow_return.rs`) over the flow substrate in `crates/verter_semantic/src/analysis/flow`. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_semantic/src`, `crates/verter_session/src`.
- Named API/data boundaries: `SemanticQueryKey`, `FlowSliceIR`, `FlowSliceHash`, `ResultContractId`, `FlowReturnResult` (whose three distributed admission channels this cutover retires), `CompleteFlowResult`, `QueryBuildOutput.flow_completion`, `CompletedFlowReturnMember` / `PendingFlowReturnMember` proof-typed results, and `flow_solve::typed_gap_for_pending_root`. The earlier `FlowSlice` / `ResultContract` names were disproven by source and are retracted; a live admission guard (`component_meta_flow_return_admission_tests.rs`) already forbids the string `FlowSlice` from leaking into published type surfaces.
- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **D2A:** implemented ledger row for “Canonical flow demand and proof substrate”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **D2D:** implemented ledger row for “Typed resolution outcome for every surface producer”; ledger presence alone satisfies the predecessor. D2D gates D2B re-certification because its typed outcome removes the raw empty-success warming chokepoint that violates D2B-AC2/AC3. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **TA1B and TA2:** implemented ledger rows for “Canonical composite payload and construction-site closure” and “Demand-scoped truthiness domain authority”; ledger presence alone satisfies each predecessor. Their commit messages, approximate timezone-bearing dates, and optional PRs are locator hints only. Both were added as explicit D2B predecessors by the canonical-type-algebra ruling (`decisions/2026-08-31-canonical-type-algebra-predecessor.md`), which holds that D2B does not land until both land, because `FlowReturnResult` promises a canonical whole-return node and D2B-AC2 pins exact identity. TA1B's predecessor TA1A ("Canonical algebra comparator builder and mint substrate") is reached transitively and is not listed directly.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- Deliver exactly “Atomic public flow-proof cutover and distributed-admission retirement” as the independently acceptable boundary; no neighboring authority is included.
- Deletion discipline: each deleted responsibility cites the source-verified displaced route it replaces; a route without that proof is preserved. The three distributed admission channels and their displaced routes are: (a) the root build's degradation decision and direct `cache_suppress` write in `build_flow_return` (`flow_return.rs`) — displaced by the finalizer outcome adapter translating a partial outcome once into `QueryBuildOutput.result_is_partial` / `partial_reasons` / `cache_suppress`, with the universal read funnel propagating those rails into enclosing builds; (b) the SCC batch's raw `result.degradation().is_some()` admission test (`semantic_query_memo/scc_publish.rs`) — displaced by proof typing that makes a degraded member unrepresentable at the publish boundary; (c) `ConsumerFold`, `FunctionReturnNode::consumer_fold`, the special call in `execute_function_return_source` (`flow_return.rs`), and `fold_flow_return_consumer_rails` (`project_semantic_dispatch/mod.rs`) — displaced by the universal consumer-rail fold. Also retired with their displaced routes: the pre-proof value-arm names `FlowReturnPendingOutcome::Complete` / `FlowRootClose::Complete` (renamed `EvaluatedValue` — completeness is claimed only by the proof), the `component_degraded` boolean as a completeness authority (its failure detection is preserved by recording gaps/failures on the ledger and branching on the finalized outcome), and the “three channels” module documentation in `semantic_query/flow_return_result.rs`. There is no legacy parallel flow resolver to delete — the A5 owner rows and the A6 lock record establish that none exists.
- Preserve: `evaluate_flow_return` / `FlowEvaluator` (return equation, coinductive holds, fixed-point join, literal widening, typed failure production — they compute values); `FlowReturnResult` as the value payload, whose degradation verdict the finalizer reads exactly once; `FlowReturnFailure`, `FlowReturnDegradation`, and `last_root_failure` as a typed caller-transport channel, never an admission decision; the generic `cache_suppress` / `result_is_partial` system for non-flow queries, fact instability, cancellation, and enclosing partial propagation; and `FlowSliceStores`, the graph bundle, lowered-IR cache, warm carrier validation, SCC root witness, retention capacity, atomic writes, reverse indexes, and inline-flight lifecycle, including the strict warm reader (`semantic_query_memo/flow_return_memo.rs`).
- Landing: D1, D2A, and D2B land as ONE atomic multi-node candidate; none of the three merges independently (codex D2 scope ruling, `decisions/2026-08-29-rev11-flow-d2-split.md`, extending the D1+D2 atomic-landing ruling of 2026-08-29). Per `contracts/github-control-plane.md`, D2A is the intentional unmapped exception; all three nodes keep distinct ledger rows, D1 and D2B retain their own issue mappings, and only those mapped nodes carry closing links. This node carries the rekeyed gh_issue 174 mapping.
- Discriminating cutover tests (all six required): `flow_root_publish_requires_complete_flow_proof` (a clean raw `FlowReturn` value with both boolean flags clear but no `CompleteFlowResult` yields zero candidates; the identical value with the finalizer token publishes and the second request is warm; deleting the proof check fails the negative leg); `production_missing_obligation_returns_only_and_recomputes` (a real production plan with one pending obligation returns its usable value twice, increments the cold-compute counter twice, and holds zero candidates); `flow_scc_publish_accepts_proof_tokens_only` (`PendingFlowReturnMember` constructible only with `CompleteFlowResult`; a structural/type guard proves `CompleteFlowResult` construction occurs only in `flow_solve.rs`; a trybuild fixture proves external construction fails); `degraded_flow_finalizer_never_warms_root_or_scc` (a natural degraded success returns its payload twice with two cold computations and zero candidates, root and SCC legs; the enclosing component test proves the universal rail replaces the deleted sealed-consumer fold); `flow_plan_runs_once_per_cold_demand_and_never_for_nonflow` (one cold demand plans exactly once; warm replay, ordinary non-flow queries, and pending typed-gap roots add zero graph builds, zero plans, zero obligation capacity); and `flow_result_contract_is_exact_identity` (changing any registry contract column changes `ResultContractId`; same key with different contract IDs compares unequal; a proof under one contract cannot finalize or publish under another). Two further named requirements (D1 adjudication ruling, 2026-08-29) own the production-provenance findings that D1's hermetic layer structurally cannot close: `production_flow_proof_has_evaluator_origin` (a non-test build can obtain discharge/convergence evidence only from the private, one-shot `FlowEvaluationOutcome` produced by `evaluate_flow_return`; raw per-obligation mutators and test helpers cannot reach production finalization) and `foreign_flow_value_provenance_is_rejected` (value, discharge report, convergence, and `FlowDemandHandle` carry the same opaque evaluation provenance, including semantic-store identity and arena epoch/request generation; a value from another demand, store, or stale generation is typed partial/ReturnOnly and produces zero root or SCC candidates). Retain the existing Supported/Stable flow-return corpus, plan-order tests, stale-basis tests, SCC order-independence tests, and component-meta flow guards; replace the old SCC unit test that directly exercises the deleted `result.degradation().is_some()` predicate.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **D2B-AC1 — sole-owner outcome:** the named final owner must be sole, and every deleted route must cite a source-verified displaced responsibility — absence of proof means preserve. The acceptance guard proves no second selectable evaluator remains and no distributed admission channel survives: warm admission of a `FlowReturn` result requires the `CompleteFlowResult` proof token, and storage safety may veto but never promote. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **D2B-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering — including exact `ResultContractId` identity on `FlowReturnKey` and deterministic discharge in `FlowDemandPlan::work_order`. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **D2B-AC3 — incremental equivalence:** prove incremental equals fresh and degraded outcomes cannot warm — a partial or no-value finalizer outcome never reaches root warm admission or the SCC publish queue, and its rails propagate through the universal read funnel. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **D2B-AC4 — bounded work:** prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate — one structural plan per cold demand, zero for warm replay, zero graph/plan/capacity for non-flow queries and pending typed-gap roots — using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `crates/verter_semantic/tests`, `crates/verter_session/tests/cases`.

## Amendments

Two architect rulings issued during D2B's round-16 review amend this charter's
acceptance boundary. They are recorded here verbatim because `.feedback/` review
packages and codex outputs are not authority — this charter is.

### Amendment: whole-control-position tri-state fail-closed boundary

Source: `.feedback/rev11/review/ARCHITECT-RULING.md` ("Architect ruling —
control-position narrowing acceptance boundary"), issued in response to a scope
consult asking whether the corrected D2B acceptance boundary is switch-only or the
whole control-position class. Verbatim ruling:

> **(b): the whole control-position class.** D2B may mint `Complete` only when
> every narrowing-capable control fact is either represented or positively proven
> irrelevant. Because `if` and ternary share `lower_guard`, rows 1–5 are the same
> acceptance-boundary defect as the switch cases—not successor debt.
>
> Architecturally, `SliceGuard::None` must mean **proved non-narrowing**, never
> "unsupported or unrecognized." Unsupported narrowing becomes one typed
> `GuardNarrowing` gap and therefore `ReturnOnly`.
>
> The common requirement is a tri-state disposition: **modeled guard / proved no
> narrowing / typed gap**. A binary `SliceGuard | None` result cannot express the
> D2B acceptance boundary safely.

This amends D2B-AC1/AC2/AC3: the guard-lowering authority
(`crates/verter_session/src/flow_slice_content.rs`) implements the tri-state
disposition as `GuardDisposition::{Modeled(SliceGuard), NoNarrowing,
Unexpressible}`, and every live control-position spelling the ruling's full text
names (both equality operators applied through boolean-wrapped/recursive
detection; `==`/`!=`; the `in` operator; nullish-coalescing narrowing; the
unwrap/assertion transparent-wrapper boundary; checker-eligible alias
preservation; switch discriminant/case relations; and the captured/free-subject
silence boundary) is represented or gapped — never silently dropped as an
"unsupported" form.

### Amendment: unclassifiable guard arms are retained and degraded, never dropped

Source: `.feedback/rev11/d2b-p2-disposition-consult.md` (the finding) and
`.feedback/rev11/d2b-p2-disposition-out.txt` (the ruling), disposing a
pre-existing wrong-complete defect in `arm_typeof_matches` / `narrow_arms_by`
(`crates/verter_session/src/project_semantic_dispatch/flow_return.rs`) surfaced by
the adversarial review lens during D2B round-16. Verbatim ruling:

> 1. **ADOPT-NOW.** D2B is not acceptable to land until this is repaired.
>
> The correctness budget binds D2B's certified public flow-return outcome—not
> every unrelated pre-existing defect in the tree, but also not merely lines
> authored by D2B. This defect is directly inside that outcome: the finalizer
> certifies and warms a wrong-complete `FlowReturn`. Provenance changes blame, not
> acceptance.
>
> The minimal fail-closed direction is correct, with one clarification:
>
> - Arm classification must distinguish `Match`, `NoMatch`, and `Unclassified`.
> - An unclassified arm remains possible on both edges, is retained, and records
>   `FlowGap::GuardNarrowing`.
> - `GuardNarrowing::Impossible` requires positive proof that no arm can inhabit
>   that edge.
> - Apply this to the reproduced `typeof` and `instanceof` paths; a `typeof`-only
>   repair would leave the measured class open.
> - Do not add exact narrowing capability. Exact results remain owned by
>   `U6.NARROW_TYPEOF` and `U6.NARROW_INSTANCEOF`. The safe `ReturnOnly` mirror
>   over-gap also stays there.
>
> This is an amendment to D2B-AC2/AC3, not a second independently acceptable
> outcome, so no new DAG node is required.

This amends D2B-AC2/AC3 exactly as ruled — no exact narrowing capability was
added; only fail-closed retention of unclassifiable arms. The required
discriminating regressions are `unclassifiable_guard_arms_remain_possible_degrade_and_never_warm`
(the `typeof` spelling) and `unclassifiable_in_guard_arms_remain_possible_degrade_and_never_warm`
(the `in` spelling) — both required alongside the six discriminating cutover
tests named above. The ruling's explicit `instanceof` requirement (a
`typeof`-only repair would leave the measured class open) is met separately by
`instanceof_narrows_by_the_checker_rule_and_gaps_only_unproven_arms`, which
carries the `instanceof` retention/gap rows.

## Deletions and forbidden designs

- Delete only source-verified displaced responsibilities, each citing the displaced route it replaces; absence of proof means preserve. The deleted artifacts are the retired legacy ADMISSION path only: the three distributed `FlowReturnResult` admission channels named in the scope section, together with any state, flags, compatibility shims, and migration guards that existed solely to serve them. The evaluator backend itself is NOT deleted — `evaluate_flow_return` / `FlowEvaluator`, `FlowSliceStores`, and the cache lifecycle machinery remain in place per the Preserve line above.
- Never introduce a second flow engine, planner, or resolver, and never route around the sole dispatch (`ProjectSemanticDispatch` / `execute_function_return_source`); the cutover must prove no second selectable evaluator remains. `crates/verter_session/src/flow_slice_content.rs` is preserved in place as part of the existing flow substrate (ruling §4), not a predeclared second engine.
- Do not touch the residual wrong-complete fallback: the A3 wrong-complete retraction is landed, and the residual non-call fabricated-`any` fallback is recorded debt (RESIDUAL-NON-CALL-ANY-FABRICATION) owned by U6.VALUE_INFERENCE (the shallow pass's per-expression fallback) plus U6.ASYNC_GENERATOR and U6.CALL_RESOLVE (`await x`) — not by D6 or D8 (debt record, `crates/verter_session/tests/cases/manifest_data/typeinfo_guard_registry.rs:599`).
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity. No shadow result comparison: the cutover swaps admission authority atomically, never by running old and new side by side.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and rescope

- Recorded landed footprint: 3,262 added production LOC across 20 production files in one crate. These machine fields preserve the accepted candidate's comparison baseline; D2B has no numeric target ceiling or numeric rescope trigger.
- Architect rescope remains mandatory under `contracts/sizing.md` when a candidate spans 3 unrelated crates/packages, or combines public/wire, unsafe, concurrency, or lifetime work with another major concern.
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

Apply `public-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `wire-public`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's
predeclared row in `authority/state/implemented.toml` from `status = "pending"`
to `status = "implemented"` with the planned squash commit message, approximate
date with timezone, and optional pull-request number. The transitioned row is the
implementation fact. Commit metadata is a loose locator only and is never resolved or
validated against Git or GitHub. Reviewers inspect the squashed candidate patch without
SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.
