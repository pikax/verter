<!-- unified-charter-v2
id=ORC0
name=Orchestration v2 cutover and immutable-receipt migration
phase=governance
train=governance.governance
product=governance
kind=activation
semantic_role=delivery
class=governance
predecessors=C1,J1
conditional_predecessors=
owner=governance.governance:static DAG authority plus immutable receipts and ephemeral leases
conflict_domains=program_authority
resource_class=docs-light
review_profile=public-3
gate_profile=docs-domain
size=S
dispatchable=true
optional=false
release_gating=none
source_refs=source:orchestration-findings.md:L1422
external_requirements=maintainer_unified_v2_activation
activation_gate=none
charter=charters/governance-governance/ORC0.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# ORC0 — Orchestration v2 cutover and immutable-receipt migration

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Orchestration v2 cutover and immutable-receipt migration. The current owner is **mutable orchestration ledger and mixed authority/evidence**. The final and sole owner is **static DAG authority plus immutable receipts and ephemeral leases**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `docs/arch/refactor/rev11/unified`.
- Named API/data boundaries: `activation receipt`, `acceptance receipt`, `lease`, `dispatch packet`.
- Mutation boundary: authority/evidence bytes only; production LOC is zero.

## Exact predecessor contracts

- **C1:** exact current receipt ID and digest for “ModuleResolverCore convergence and non-flow semantic basis”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **J1:** exact current receipt ID and digest for “CSS owner reconciliation”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody maintainer_unified_v2_activation:** require the exact immutable static slot at dispatch and the finalized-candidate-bound authorization before evidence or acceptance.

## Source-specific scope

- Deliver exactly “Orchestration v2 cutover and immutable-receipt migration” as the independently acceptable boundary; no neighboring authority is included.

## Acceptance IDs and discriminating proof

- **ORC0-AC1 — sole-owner proof:** add `orc0_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **ORC0-AC2 — positive contract:** add `orc0_publishes_exact_activation_receipt`; assert exact identities, provenance, completeness, and deterministic ordering.
- **ORC0-AC3 — incremental equivalence:** add `orc0_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **ORC0-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
- Test homes: `docs/arch/refactor/rev11/unified/fixtures`.

## Deletions and forbidden designs

- Delete or structurally reject: **mutable READY ledger**.
- Delete or structurally reject: **resource-capacity DAG edge**.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 0 production LOC, 0 production files, 0 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing.
- Performance budget: equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance is 0.0% unless an owning-authority amendment supplies exact replacement thresholds; after warmup, retained bytes may not increase across 100 identical requests.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, a predecessor receipt is stale, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `node docs/arch/refactor/rev11/unified/tools/validate-negative-controls.mjs`
2. Run every final command in the bound `docs-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Re-run the planted RED mutation, restore, then GREEN, and bind both outputs to the candidate SHA/tree.

## Review and lower-severity findings

Apply `public-3`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates the gate and all three review verdicts. Final acceptance requires three independent PASS verdicts on the exact candidate tree; clean 3/3 means no P0/P1 and no unauthorized or undispositioned P2.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three named independent reviewers with assigned lenses, model `gpt-5.6-sol`, effort `ultra`, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:orchestration-findings.md:L1422`

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

No clause targets this file directly. Applicable contract clauses are selected by the validated `applicable_nodes` ledger and embedded verbatim in cold packets.
