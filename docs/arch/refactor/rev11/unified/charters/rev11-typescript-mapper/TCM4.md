<!-- unified-charter-v2
id=TCM4
name=Atomic activation and deletion
phase=rev11
train=rev11.typescript-mapper
product=rev11
kind=implementation
semantic_role=delivery
class=foundational
predecessors=TCM0R,TCM1,TCM2,TCM3,H2,B4R0
conditional_predecessors=
owner=rev11.typescript-mapper:ratified dual-plane mapper/snapshot/oracle identity contract
conflict_domains=certifiedtypeenginebinding
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
source_refs=live:docs/arch/refactor/rev11/charters/TCM4.md
external_requirements=
activation_gate=ORC0
charter=charters/rev11-typescript-mapper/TCM4.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# TCM4 — Atomic activation and deletion

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Atomic activation and deletion. The current owner is **rejected TCM0 closure package and string mapper plane**. The final and sole owner is **ratified dual-plane mapper/snapshot/oracle identity contract**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_type_runtime/src`, `crates/verter_session/src`, `packages/typescript-plugin/src`, `crates/verter_protocol/src`.
- Named API/data boundaries: `CertifiedTypeEngineBinding`, `InputBasisId`, `QueryIdentity`, `SemanticFlightKey`, `ContentMapper`.
- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **TCM0R:** exact current receipt ID and digest for “TypeScript dual-plane architecture and observation-identity rescope”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **TCM1:** exact current receipt ID and digest for “Compact mapping products inside CodeTransform”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **TCM2:** exact current receipt ID and digest for “Content-mapper projection plane (dormant until TCM4)”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **TCM3:** exact current receipt ID and digest for “TypeScript semantic capability closure (dormant until TCM4)”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **H2:** exact current receipt ID and digest for “Project-scoped ProviderHub bindings”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **B4R0:** exact current receipt ID and digest for “Stable SourceUnitId lineage repair”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- Deliver exactly “Atomic activation and deletion” as the independently acceptable boundary; no neighboring authority is included.

## Acceptance IDs and discriminating proof

- **TCM4-AC1 — sole-owner proof:** add `tcm4_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **TCM4-AC2 — positive contract:** add `tcm4_publishes_exact_certifiedtypeenginebinding`; assert exact identities, provenance, completeness, and deterministic ordering.
- **TCM4-AC3 — incremental equivalence:** add `tcm4_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **TCM4-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
- Test homes: `crates/verter_type_runtime/tests`, `crates/verter_session/tests/cases`.

## Deletions and forbidden designs

- Delete or structurally reject: **self-certified closure status**.
- Delete or structurally reject: **tracked Python/POSIX control**.
- Delete or structurally reject: **mapper callback into semantic oracle**.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 800 production LOC, 8 production files, 2 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing.
- Performance budget: equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance is 0.0% unless an owning-authority amendment supplies exact replacement thresholds; after warmup, retained bytes may not increase across 100 identical requests.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, a predecessor receipt is stale, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_type_runtime -p verter_session -p verter_protocol`
2. Run every final command in the bound `targeted-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Re-run the planted RED mutation, restore, then GREEN, and bind both outputs to the candidate SHA/tree.

## Review and lower-severity findings

Apply `public-3`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates the gate and all three review verdicts. Final acceptance requires three independent PASS verdicts on the exact candidate tree; clean 3/3 means no P0/P1 and no unauthorized or undispositioned P2.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three fresh distinct harness reviewers with assigned lenses, deterministic low|medium|high effort, exact task/provider/model bindings, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `live:docs/arch/refactor/rev11/charters/TCM4.md`

## TCM0 remediation receiving criteria

- `TCM4-RC-ACTIVATED-HANG-TOPOLOGY`: rerun the bounded attach/hang probe against the activated certified package and bind `TCM0-R-HANG-TOPOLOGY` without generalizing non-occurrence.
- `TCM4-RC-ACTIVATED-TOPOLOGY`: verify the TCM2 projection and TCM3 semantic selections against TCM0's locked harness/metrics and bind `TCM0-R-TOPOLOGY-SELECTION`.
- `TCM4-RC-ACTIVATED-BASELINE`: verify activated results against the pre-change comparison receipts accumulated by TCM1–TCM3 and bind `TCM0-R-IMPLEMENTATION-BASELINE`.
- Verify the accumulated current-tree plus introduced/orphaned deletion/survival manifest; do not rediscover TCM0's inventory. All three residue IDs must close, all 32 must-close finding IDs must be proven, no additional bounded claim may remain, and direct receiving-ID/charter/authority digest equality is required before atomic activation/deletion.

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

No clause targets this file directly. Applicable contract clauses are selected by the validated `applicable_nodes` ledger and embedded verbatim in cold packets.

## Live authority inputs

- `live:docs/arch/refactor/rev11/charters/TCM4.md` — 17266 bytes, SHA-256 `2ab8d379f94e407a62fafa2a24f216452b13cea867bdcb196c45f7acc1063115`
