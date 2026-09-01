<!-- unified-charter-v2
id=D3I
name=Complete stable binding identity
phase=rev11
train=rev11.flow
product=rev11
kind=implementation
semantic_role=delivery
class=foundational
predecessors=D3R
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
charter=charters/rev11-flow/D3I.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# D3I — Complete stable binding identity

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

Complete stable binding identity (codex D3 scope ruling, `decisions/2026-08-30-rev11-flow-d3-split.md`). Stable identities nominally exist — `FlowSlot.binding` carries `SkeletonBindingId` (`crates/verter_semantic/src/analysis/flow/flow_ir.rs`) and `FlowBindingIdentity` claims `(defining_function, binding_slot)` identity (`crates/verter_semantic/src/analysis/function_program.rs`) — but the implementation does not honor them end to end. This node: (1) removes the adjacent same-name/same-kind `bindings.dedup_by` in `function_program.rs`, which contradicts its own full-inventory/no-deduplication contract; (2) extends `FunctionBindingKind` across all value-bearing `SkeletonBindingKind`s, ending the refusal of `Class`, `CatchParam`, `Enum`, `Namespace`, and `ImportEquals`; (3) indexes every destructured bound identifier with a real slot — no fabricated out-of-range slots — fixing `flow_solve.rs::resolve_binding_identities`, which today correlates by name and kind, reuses a previous slot for collapsed twins, and fabricates destructured slots as `inventory.len() + skeleton ordinal`; (4) introduces `FlowBindingRef::{Local(SkeletonBindingId), Captured(FlowBindingIdentity)}` and an exact `FlowBindingMap` whose mapping is by declaration identity/span/source slot, never name fallback; and (5) converts `SliceStatement::Binding`, `SliceExpr::Local`, `SliceNarrowRoot`, and the capture authorities to carry resolved binding references — names remain display fields for lexical lookup and diagnostics only. The result is a bijective binding inventory: every value-bearing skeleton binding has one real stable identity, shadowed twins never collapse, and destructured identifiers never receive synthetic slots. The current and final owner is the **sole shared flow authority**: `ProjectSemanticDispatch` / `execute_function_return_source` (`crates/verter_session/src/project_semantic_dispatch/flow_return.rs`) over the flow substrate in `crates/verter_semantic/src/analysis/flow`. This charter accepts one authority boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_semantic/src`, `crates/verter_session/src`.
- Production files: `verter_semantic/src/analysis/function_program.rs`, `verter_semantic/src/analysis/flow/mod.rs`, `verter_semantic/src/analysis/flow/flow_ir.rs`, `verter_session/src/flow_slice_content.rs`, `verter_session/src/project_semantic_dispatch/flow_solve.rs`.
- Named API/data boundaries: `FunctionBindingKind`, `FlowBindingIdentity`, `FlowBindingRef::{Local, Captured}`, `FlowBindingMap`, `SkeletonBindingId`, `SliceStatement::Binding`, `SliceExpr::Local`, `SliceNarrowRoot`.
- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **D3R:** implemented ledger row for “Nominal relation authority”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- Deliver exactly “Complete stable binding identity” as the independently acceptable boundary; no neighboring authority is included.
- Named work (ruling §3): remove binding deduplication; extend `FunctionBindingKind` across all value-bearing `SkeletonBindingKind`s; index every destructured bound identifier; introduce `FlowBindingRef::{Local(SkeletonBindingId), Captured(FlowBindingIdentity)}`; introduce an exact `FlowBindingMap` (declaration identity/span/source slot, never name fallback); convert `SliceStatement::Binding`, `SliceExpr::Local`, `SliceNarrowRoot`, and capture authorities to carry resolved binding references with names remaining display-only.
- After lowering resolves a reference, semantic state keys only on `SkeletonBindingId` locally and `FlowBindingIdentity` across frames; names remain permitted for lexical lookup and diagnostics only.
- Discriminating tests (all three required): `function_binding_inventory_preserves_every_stable_slot` (adjacent same-name/same-kind bindings in different scopes, parameter/`var` redeclaration, destructured parameters/locals, catch parameters, local class/enum, and nested function declarations — real, unique slots with no fabricated tail range); `flow_binding_map_is_bijective_for_value_bindings` (every value-bearing `SkeletonBindingId` maps exactly once to a `FlowBindingIdentity`; type-only bindings map to no value product); `binding_products_do_not_alias_shadowed_twins` (two same-name bindings receive independent declared/reaching/assignment products; writing or joining one cannot change the other).
- Landing: D3R, D3I, D3P, and D3C land as ONE atomic multi-node candidate; none of the four merges independently (codex D3 scope ruling, `decisions/2026-08-30-rev11-flow-d3-split.md`, extending the D1+D2A+D2B atomic-landing pattern of `decisions/2026-08-29-rev11-flow-d2-split.md`). Per `contracts/github-control-plane.md`, each node in the shared candidate keeps its own issue mapping, ledger row, and closing link; D3I intentionally carries no GitHub issue mapping (the pre-existing D3 issue mapping, gh_issue 175, was rekeyed to D3C — the maintainer freeze on issue churn creates no new issues for the substrate nodes).

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **D3I-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected — no name/kind correlation, collapsed-twin slot reuse, or fabricated destructured slot survives in binding identity resolution. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **D3I-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering — the binding inventory is bijective and the `FlowBindingMap` never falls back to names. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **D3I-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **D3I-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `crates/verter_semantic/tests`, `crates/verter_session/tests/cases`.

## Deletions and forbidden designs

- Delete the binding deduplication (`bindings.dedup_by` in `function_program.rs`) and the name/kind-correlation, collapsed-twin slot reuse, and fabricated destructured slots in `flow_solve.rs::resolve_binding_identities` — each cites the displaced route it replaces.
- Never introduce a name-fallback binding map, a second binding identity type, or a second flow engine, planner, or resolver; never route around the sole dispatch (`ProjectSemanticDispatch` / `execute_function_return_source`).
- Do not touch the residual wrong-complete fallback: the A3 wrong-complete retraction is landed, and the residual non-call fabricated-`any` fallback is recorded debt (RESIDUAL-NON-CALL-ANY-FABRICATION) owned by U6.VALUE_INFERENCE (the shallow pass's per-expression fallback) plus U6.ASYNC_GENERATOR and U6.CALL_RESOLVE (`await x`) — not by D6 or D8 (debt record, `crates/verter_session/tests/cases/manifest_data/typeinfo_guard_registry.rs:599`).
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 800 production LOC, 8 production files, 2 related crates/packages (ruling target: 500–750 production LOC, at most 6 files, 2 crates).
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

Before squashing or review, the implementation patch transitions this node's predeclared row in `authority/state/implemented.toml` from `status = "pending"` to `status = "implemented"` with the planned squash commit message, approximate date with timezone, and optional pull-request number. The transitioned row is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.
