<!-- unified-charter-v2
id=D3R
name=Nominal relation authority
phase=rev11
train=rev11.flow
product=rev11
kind=implementation
semantic_role=delivery
class=foundational
predecessors=D2B,C1
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
charter=charters/rev11-flow/D3R.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# D3R — Nominal relation authority

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

Nominal relation authority (codex D3 scope ruling, `decisions/2026-08-30-rev11-flow-d3-split.md`). `RelationKind` (`crates/verter_session/src/semantic_query.rs`) declares `Assignable`, `Subtype`, `StrictSubtype`, `Identity`, and `Comparable`, but `relation.rs::reduce_relation` refuses everything except `Assignable`. This node makes the exact/canonical/nominal part of `RelationKind::Identity` live (bounded) and makes `RelationKind::Comparable` live with three internal outcomes — holds, does-not-hold, undecided — reusing the existing `RelationStep`/ReturnOnly rails with no public `Unknown` payload. It reuses `ValueDeclIdentityPart` and the existing unique-symbol lookup path — it does not mint a second unique-symbol identity type — and preserves unique-symbol identity through aliases, imports, and re-exports. It deletes `NodeDisjointness` and `nodes_provably_disjoint` from `flow_return.rs`: flow narrowing asks the shared `Comparable` relation, a negative decision proves disjointness, and an undecided result remains a typed `NominalRelation` gap that cannot warm. `Subtype` and `StrictSubtype` stay pending — D4–D6 do not require them, and implementing them would silently broaden this node. `call_resolve` stays on `Assignable` semantically unchanged; `Relate` remains the sole query tag. This node adds two live relation judgments — bounded `Identity` and tri-state `Comparable` — not another relation query family. The current and final owner is the **sole shared flow authority**: `ProjectSemanticDispatch` / `execute_function_return_source` (`crates/verter_session/src/project_semantic_dispatch/flow_return.rs`) over the flow substrate in `crates/verter_semantic/src/analysis/flow`. This charter accepts one authority boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_session/src` only.
- Production files: `semantic_query.rs`, `project_semantic_dispatch/relation.rs`, `project_semantic_dispatch/relation_predicates.rs`, `project_semantic_dispatch/flow_return.rs`, and `project_semantic_dispatch/lower.rs` only if carrier preservation is required.
- Named API/data boundaries: `RelationKind`, `SemanticQueryKey` (`Relate` remains the sole query tag), `RelateMemoKey`, `relation_nominal_identity`, `reduce_identity`, `reduce_comparable`.
- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **D2B:** implemented ledger row for “Atomic public flow-proof cutover and distributed-admission retirement”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **C1:** implemented ledger row for “ModuleResolverCore convergence and non-flow semantic basis”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- Deliver exactly “Nominal relation authority” as the independently acceptable boundary; no neighboring authority is included.
- Named work (ruling §3): `RelateMemoKey::{identity, comparable}` constructors or a kind-aware constructor; `relation_nominal_identity`; `reduce_identity`; `reduce_comparable`; delete `NodeDisjointness`/`nodes_provably_disjoint`; preserve `call_resolve` on `Assignable`.
- Discriminating tests (all five required): `relation_unique_symbol_identity_and_comparability_are_tristate` (table-test same unique identity, distinct unique identities, aliases, and an unresolved nominal subject — same is positive, distinct is negative, unresolved is undecided/ReturnOnly with zero candidate); `relation_nominal_identity_survives_import_and_reexport` (the declaring identity, not the consumer alias, controls both `Identity` and `Comparable`); `flow_g3_nominal_relation_gap_retracts_only_when_decided` (the existing `g3` unique-symbol discriminant fixture moves from `NominalRelation` partial/cold to the exact checker-compatible value and the second request is warm; an unresolved-symbol control remains partial/cold); `flow_has_no_private_relation_classifier` (structural guard: no `NodeDisjointness`, `nodes_provably_disjoint`, or direct primitive/literal disjointness table remains in `flow_return.rs`); `call_resolution_remains_assignability_only` (a call-applicability fixture proves the new identity/comparability routes do not alter `call_resolve` candidate selection, inference deposits, or the sealed-empty flow axis).
- Landing: D3R, D3I, D3P, and D3C land as ONE atomic multi-node candidate; none of the four merges independently (codex D3 scope ruling, `decisions/2026-08-30-rev11-flow-d3-split.md`, extending the D1+D2A+D2B atomic-landing pattern of `decisions/2026-08-29-rev11-flow-d2-split.md`). Per `contracts/github-control-plane.md`, each node in the shared candidate keeps its own issue mapping, ledger row, and closing link; D3R intentionally carries no GitHub issue mapping (the pre-existing D3 issue mapping, gh_issue 175, was rekeyed to D3C — the maintainer freeze on issue churn creates no new issues for the substrate nodes).

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **D3R-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected — no flow-private relation classifier survives outside the shared `Relate` reduction. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **D3R-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering — unique-symbol identity is preserved through aliases, imports, and re-exports, and `call_resolve` semantics are unchanged. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **D3R-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm — an undecided `Comparable` result remains a typed `NominalRelation` gap and cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **D3R-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `crates/verter_semantic/tests`, `crates/verter_session/tests/cases`.

## Deletions and forbidden designs

- Delete `NodeDisjointness` and `nodes_provably_disjoint` from `flow_return.rs` — the displaced route is the flow-private relation classifier that directly classifies primitive/literal/object tags and treats every `symbol` comparison as missing nominal identity (the source of the current `g3` `FlowGap::NominalRelation`); the shared `Comparable` relation replaces it.
- Never invent a public `Unknown` payload for relation outcomes, a second unique-symbol identity type, or a second relation query family; `Relate` remains the sole query tag and the existing `RelationStep`/ReturnOnly rails carry the undecided outcome.
- Leave `Subtype` and `StrictSubtype` pending; do not implement them here.
- Never introduce a second flow engine, planner, or resolver, and never route around the sole dispatch (`ProjectSemanticDispatch` / `execute_function_return_source`). No parallel flow resolver exists to delete — A5 owner rows and the A6 implementation lock record establish `crates/verter_session/src/flow_slice_content.rs` and `crates/verter_semantic/src/analysis/flow` as the single existing flow pipeline.
- Do not touch the residual wrong-complete fallback: the A3 wrong-complete retraction is landed, and the residual non-call fabricated-`any` fallback is recorded debt (RESIDUAL-NON-CALL-ANY-FABRICATION) owned by U6.VALUE_INFERENCE (the shallow pass's per-expression fallback) plus U6.ASYNC_GENERATOR and U6.CALL_RESOLVE (`await x`) — not by D6 or D8 (debt record, `crates/verter_session/tests/cases/manifest_data/typeinfo_guard_registry.rs:599`).
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 800 production LOC, 8 production files, 2 related crates/packages (ruling target: 450–650 production LOC, at most 6 files, `verter_session` only).
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
