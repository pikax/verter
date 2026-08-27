<!-- unified-charter-v2
id=TCM0R
name=TypeScript dual-plane architecture and observation-identity rescope
phase=rev11
train=rev11.typescript-mapper
product=typescript_mapper
kind=rescope
semantic_role=delivery
class=foundational
predecessors=ORC0,A6,B4R0
conditional_predecessors=
owner=rev11.typescript-mapper:ratified dual-plane mapper/snapshot/oracle identity contract
conflict_domains=source_lineage
resource_class=docs-light
review_profile=architecture-3
gate_profile=docs-domain
size=S
dispatchable=true
optional=false
release_gating=none
source_refs=live:docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-26-TCM0-REMEDIATION.md,live:docs/arch/refactor/rev11/charters/TCM0.md
external_requirements=maintainer_tcm0_rescope_ratification
activation_gate=ORC0
charter=charters/rev11-typescript-mapper/TCM0R.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# TCM0R — TypeScript dual-plane architecture and observation-identity rescope

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

TypeScript dual-plane architecture and observation-identity rescope. The current owner is **rejected TCM0 closure package and string mapper plane**. The final and sole owner is **ratified dual-plane mapper/snapshot/oracle identity contract**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_type_runtime/src`, `crates/verter_session/src`, `packages/typescript-plugin/src`, `crates/verter_protocol/src`.
- Named API/data boundaries: `CertifiedTypeEngineBinding`, `InputBasisId`, `QueryIdentity`, `SemanticFlightKey`, `ContentMapper`.
- Mutation boundary: authority/evidence bytes only; production LOC is zero.

## Exact predecessor contracts

- **ORC0:** exact current receipt ID and digest for “Orchestration v2 cutover and immutable-receipt migration”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **A6:** exact current receipt ID and digest for “Implementation Lock Record”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **B4R0:** exact current receipt ID and digest for “Stable SourceUnitId lineage repair”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody maintainer_tcm0_rescope_ratification:** require the exact immutable static slot at dispatch and the finalized-candidate-bound authorization before evidence or acceptance.

## Source-specific scope

- **Revision:** 4 — supersedes the 251-charter all-verticals proposal
- **Prepared:** 2026-08-26
- **Repository basis:** program/architecture-lock at d1f3d50a948597f036868543b9bb21acacd730ff
- **Current-program condition:** maintainer work freeze; TCM0 = RESCOPE_REQUIRED; TCM1–TCM4 = LOCKED

## Acceptance IDs and discriminating proof

- **TCM0R-AC1 — sole-owner proof:** add `tcm0r_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **TCM0R-AC2 — positive contract:** add `tcm0r_publishes_exact_certifiedtypeenginebinding`; assert exact identities, provenance, completeness, and deterministic ordering.
- **TCM0R-AC3 — incremental equivalence:** add `tcm0r_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **TCM0R-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
- Test homes: `crates/verter_type_runtime/tests`, `crates/verter_session/tests/cases`.

## Deletions and forbidden designs

- Delete or structurally reject: **self-certified closure status**.
- Delete or structurally reject: **tracked Python/POSIX control**.
- Delete or structurally reject: **mapper callback into semantic oracle**.
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

1. `cargo nextest run -p verter_type_runtime -p verter_session -p verter_protocol`
2. Run every final command in the bound `docs-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Re-run the planted RED mutation, restore, then GREEN, and bind both outputs to the candidate SHA/tree.

## Review and lower-severity findings

Apply `architecture-3`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates the gate and all three review verdicts. Final acceptance requires three independent PASS verdicts on the exact candidate tree; clean 3/3 means no P0/P1 and no unauthorized or undispositioned P2.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three named independent reviewers with assigned lenses, model `gpt-5.6-sol`, effort `ultra`, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `live:docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-26-TCM0-REMEDIATION.md`
- `live:docs/arch/refactor/rev11/charters/TCM0.md`

## Binding TCM0 remediation instrument

This block implements `ARCHITECT-RULING-2026-08-26-TCM0-REMEDIATION`; a summary or generic closure claim is not acceptance evidence.

- Atomize the digest-bound charter into stable claim IDs. The input register contains claim IDs and proof references and schema-rejects any author-set `status`; the validator derives `OPEN`, `REFUSED`, `PROVEN-BOUNDED`, or `PROVEN` and generates the human view.
- Each claim uses an allowlisted claim-specific adapter. A proof receipt binds repository SHA/tree, package digest, fixture inputs, exact command, and instrument digest, and proves a terminal summary, nonzero selected work, zero unexpected skips, and internally consistent counters.
- Canonical negative controls prove unique newly-applied mutations. Covered atoms come only from receipts. The validator computes `claim atoms - covered atoms`; disclosed limits or partial/sample-only proof force bounded status. A bounded claim is never acceptance-admissible without a ratified transfer.
- Every remainder binds a stable residue ID, authorized non-circular owner, direct receiving-criterion ID, resolution gate, and matching charter/authority digests. Missing/stale/contradictory evidence is `OPEN` or `REFUSED`.
- The only allowed residues are `TCM0-R-HANG-TOPOLOGY` (C2 and AD4; TCM3 then TCM4), `TCM0-R-TOPOLOGY-SELECTION` (C7; TCM2 projection and TCM3 semantic topology), and `TCM0-R-IMPLEMENTATION-BASELINE` (C9; TCM1–TCM3 pre-change comparisons and TCM4 activated verification). No fourth residue ID or other bounded row is admissible.
- Reconcile all 36 finding entries exactly: must-close `C1,C3,C4,C5,C6,C8,AR1,AR2,AR3,AR4,AR5,AR6,AR7,AR8,AR9,AR10,AR11,AR12,AR13,AR14,AR15,AD1,AD2,AD3,AD5,AD6,AD7,AD8,AD9,AD10,AD11,AD12`; residue entries `C2,C7,C9,AD4`; none are “not findings”.
- Rebuild the closure register/validator, claim universe, receiving coverage, lexical scanners, ownership ledger, transcript, gaps/summary, and downstream narratives. Delete tracked Python/POSIX controls and name-keyed scanner guards. Preserve only repaired package/binary provenance, mapper captures, semantic probes, stale-snapshot characterization, cache/lifecycle contract, acyclic test specification, five projection classes, ratified ownership decisions, concrete deletion/survivor rows, and consolidated probe 10.

The successful pre-review state is `READY_FOR_MANDATES`, never `ADMISSIBLE`. The serial gates are: (1) instrument repair alone, with current evidence `REFUSED`; (2) ratify atomic claims and three residue transfers; (3) rebuild recursive subject manifest, one-method/one-capability ownership and portable controls; (4) complete package/semantic probes in parallel with (5) architectural contracts/handoffs; (6) reconcile 32 closures plus four entries in three residues and obtain byte-stable `READY_FOR_MANDATES`; (7) freeze one complete candidate `R`; (8) run three blind independent exact-`R` mandates concurrently, all clean PASS; (9) record maintainer acceptance in a ledger-only transition and prove downstream eligibility. Any substantive change after `R`, any nonzero final finding, or any extra bounded claim restarts the freeze/review sequence.

Mandatory controls cover omitted claims, forbidden input status, removed residue/owner, missing dependency, stale receipt, irrelevant existing proof, zero selected work, skipped work, inconsistent counters, unapplied mutation, disclosed limit, and the former bounded-to-proven laundering class.

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

No clause targets this file directly. Applicable contract clauses are selected by the validated `applicable_nodes` ledger and embedded verbatim in cold packets.

## Live authority inputs

- `live:docs/arch/refactor/rev11/charters/TCM0.md` — 6916 bytes, SHA-256 `2ea41dd85befd978e06364d952eb3b262c9b6edba1f1ac8ce37eba9845b91e97`
- `live:docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-26-TCM0-REMEDIATION.md` — 20062 bytes, SHA-256 `d5aca4b4b5c42a82bfb77f1cc9a91074004c6876a7532850306622b703ff66c7`
