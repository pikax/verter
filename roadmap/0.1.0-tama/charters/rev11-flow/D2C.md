<!-- unified-charter-v2
id=D2C
name=Flow-return partiality audit projection
phase=rev11
train=rev11.flow
product=rev11
kind=implementation
semantic_role=delivery
class=foundational
predecessors=D2B
owner=rev11.flow:sole shared flow authority
conflict_domains=auditevent
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
size=S
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/rev11-flow/D2C.md
max_production_loc=300
max_production_files=4
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# D2C — Flow-return partiality audit projection

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

Project the flow-return evaluator's own typed partiality vocabulary — `FlowGap`, `FlowReturnDegradation` (the degraded-but-usable reason attached to an `Ok` outcome), and `FlowReturnFailure` (the no-value reason on the `Err` outcome) — onto the `FlowReturnInference` audit payload, so a caller inspecting `RequestAuditRecord::flow_return_inference_payload()` can see WHY a request was partial or no-value, not only THAT it was. Today's payload (`FlowReturnInferencePayload`, `crates/verter_audit/src/payloads/flow_return.rs`) carries `function_symbol` plus three occurrence counters — `cold_computes`, `budget_exceeded_events`, `cycle_reentry_holds` — and `.claude/skills/audit-infrastructure/SKILL.md` documents exactly those three; none of them names a reason.

This node is the disposed successor of the D2B round-16 architect ruling on finding (b) "Flow gaps absent from audit" (`.feedback/rev11/d2-intent-conformance-out.txt`), ratified in `decisions/2026-09-01-d2c-flow-return-audit-partiality-defer.md`: "This is an independently acceptable observability projection: it changes neither semantic value nor admission authority and lies outside D2B's declared production surfaces." The debt row this node discharges is `FLOW-RETURN-AUDIT-PARTIALITY`.

D2C changes NO admission, resolution, or value-computation behavior. It is a strictly additive, read-only projection: it reads the SAME `FlowReturnResult::degradation()` / `FlowReturnFailure` value the finalizer and the `Err` arm already compute and certify inside `VerterHost::get_flow_return_type_with_audit`, and reduces the observed reason to a typed, wire-safe field on `FlowReturnInferencePayload`, regenerated into `packages/types/audit.generated.ts`. The current and final owner is the same **sole shared flow authority** as its D-train siblings. This charter accepts one projection boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_audit/src`, `crates/verter_session/src`.
- Named API/data boundaries: `FlowReturnInferencePayload`, `FlowGap`, `FlowReturnDegradation`, `FlowReturnFailure`, `VerterHost::get_flow_return_type_with_audit`, `RequestAuditRecord::flow_return_inference_payload()`.
- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **D2B:** implemented ledger row for "Atomic public flow-proof cutover and distributed-admission retirement"; ledger presence alone satisfies the predecessor. D2B's finalizer and evaluator are the producers of the typed `FlowGap` / `FlowReturnDegradation` / `FlowReturnFailure` vocabulary this node projects; D2C does not compute a new reason, it surfaces the one D2B already certifies.
- **External requirements:** none.

## Source-specific scope

- Deliver exactly "Flow-return partiality audit projection" as the independently acceptable boundary; no neighboring authority is included.
- Extend `FlowReturnInferencePayload` with a typed, wire-safe partiality field (or fields) covering: (a) the observed `FlowReturnDegradation` reason on a degraded-but-usable `Ok` outcome (`FlowReturnResult::degradation()`), reduced through its `FlowGap` variant when the degradation is `FlowReturnDegradation::FlowGap(_)`; and (b) the observed `FlowReturnFailure` reason on the `Err` outcome. Every existing `FlowGap` variant (`GuardNarrowing`, `NominalRelation`, `ClosureCapture`, `AbruptCompletion`, `UnmodeledExpression`) and every existing `FlowReturnDegradation` / `FlowReturnFailure` variant must have a distinct, typed wire representation — never a collapsed catch-all and never a `Debug`-formatted string.
- Wire the projection at the ONE producer, `VerterHost::get_flow_return_type_with_audit` (`crates/verter_session/src/host_flow_return_audit.rs`) — the sole audited flow-return entry-point named by the shared owner. No second flow-return audit entry-point is created, and no second projection path is introduced elsewhere.
- Update the owning documentation, `.claude/skills/audit-infrastructure/SKILL.md`'s `FlowReturnInference` section (today documents only the three counters), to describe the new typed partiality field(s) alongside them.
- Deletion discipline: this node adds fields; it deletes nothing. If a future consumer needs the three existing counters removed or reshaped, that is a separate amendment.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **D2C-AC1 — sole-projection outcome:** the audit payload's typed partiality field is the SOLE observability projection of `FlowGap` / `FlowReturnDegradation` / `FlowReturnFailure` onto the wire surface — no second ad-hoc audit-adjacent channel (a log line, a debug print, a side-channel counter) duplicates or can diverge from it. The projection reads the SAME typed reason the finalizer/evaluator already computed; it never re-derives, infers from text, or guesses one. Prefer existing type, capability, or static enforcement (an exhaustive match over the closed `FlowGap` / `FlowReturnDegradation` / `FlowReturnFailure` enums is the forcing function). Add a negative or mutation leg only for a plausible critical fail-closed/correctness boundary.
- **D2C-AC2 — positive contract:** `flow_return_audit_explains_partial_cold_recompute` (the ratified acceptance test) — a reachable production `FlowReturn` guard gap returns its usable partial value twice, both requests are cold, and each `FlowReturnInferencePayload` reports typed partiality plus `GuardNarrowing`; a complete control reports no partiality and its second request is warm. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **D2C-AC3 — incremental equivalence / no admission change:** prove the new payload field(s) carry OBSERVABILITY DATA ONLY, with zero effect on `FlowReturnResult` admission, `CompleteFlowResult` minting, warm/cold classification, or cache identity — a mutation test proves flipping or removing the new audit field(s) changes no admission outcome and no cache key.
- **D2C-AC4 — bounded work:** prove no new query, no new cache, and no additional evaluation pass — the projection reads data the evaluator/finalizer already computed once per request. Prove zero additional cold computation, allocation, or duplicate resolve introduced by the projection itself, using applicable existing counters, inspection, or benchmarks; otherwise record a terse not-applicable rationale.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `crates/verter_audit/tests/cases`, `crates/verter_session/tests/cases`.

## Deletions and forbidden designs

- This node deletes nothing; it is strictly additive. A future deletion of the three existing counters, or a reshape of the payload's counter fields, is out of scope and requires its own amendment.
- Never let the projection become a second admission signal — it is read-only telemetry over the existing typed degradation/failure reason, never consulted by `FlowReturnResult` admission, `CompleteFlowResult` minting, or any cache-validity gate.
- Never infer partiality from text, source slicing, or a name-suffix heuristic; classify exclusively from the typed `FlowGap` / `FlowReturnDegradation` / `FlowReturnFailure` enums.
- Never widen `RequestKindPayload`'s taxonomy shape beyond an additive field on the existing `FlowReturnInference` variant; no new `RequestKind` variant, no breaking change to an existing payload field's type or meaning.
- Never duplicate or replace the three existing counters' semantics (`cold_computes`, `budget_exceeded_events`, `cycle_reentry_holds`); the new field(s) are additive alongside them.
- Never create a second flow-return audit entry-point; the projection is wired at `VerterHost::get_flow_return_type_with_audit` only.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 300 production LOC, 4 production files, 2 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing. An admission-affecting side effect from the new field(s) is a correctness-budget violation, not a scope question.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a soak solely to satisfy this charter.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, an ancestor lacks an implemented ledger row, or the complete diff will not fit one review context.
- Stop if satisfying D2C-AC2 appears to require any change to `FlowReturnResult` admission, warm/cold classification, or a cache key; that would be an amendment to D2B's certified admission authority, not a local decision here.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_audit -p verter_session`
2. Run every final command in the bound `targeted-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `semantic-3`: 2 fresh distinct harness tasks covering exactly `adversarial`, `conformance`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 2/2 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `targeted` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch adds one `[[implemented]]` row to `authority/state/implemented.toml` with the node ID, planned squash commit message, approximate date with timezone, and optional pull-request number. Row presence is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.

## Resolution gate

Per the ratifying decision (`decisions/2026-09-01-d2c-flow-return-audit-partiality-defer.md`), D2C must land before `D3R` dispatch — encoded structurally as an explicit `D3R` predecessor in `authority/dag/rev11-flow.toml` — and may not survive to plan close.
