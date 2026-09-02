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
owner=rev11.typescript-mapper:ratified dual-plane mapper/snapshot/oracle identity contract
conflict_domains=source_lineage
resource_class=docs-light
review_profile=architecture-3
gate_profile=docs-domain
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=high
verification_effort_default=high
confirmation_effort_min=high
confirmation_effort_default=high
size=S
dispatchable=true
optional=false
release_gating=none
external_requirements=maintainer_tcm0_rescope_ratification
charter=charters/rev11-typescript-mapper/TCM0R.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# TCM0R — TypeScript dual-plane architecture and observation-identity rescope

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

TypeScript dual-plane architecture and observation-identity rescope. The current owner is **rejected TCM0 closure package and string mapper plane**. The final and sole owner is **ratified dual-plane mapper/snapshot/oracle identity contract**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_type_runtime/src`, `crates/verter_session/src`, `packages/typescript-plugin/src`, `crates/verter_protocol/src`.
- Named API/data boundaries: `CertifiedTypeEngineBinding`, `InputBasisId`, `QueryIdentity`, `SemanticFlightKey`, `ContentMapper`.
- Mutation boundary: authority/evidence bytes only; production LOC is zero.

## Exact predecessor contracts

- **ORC0:** implemented ledger row for “Trusted implementation-ledger cutover”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **A6:** implemented ledger row for “Implementation Lock Record”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **B4R0:** implemented ledger row for “Stable SourceUnitId lineage repair”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirement maintainer_tcm0_rescope_ratification:** agents obtain the maintainer decision; tooling does not validate it.

## Source-specific scope

- **Revision:** 4 — supersedes the 251-charter all-verticals proposal
- **Prepared:** 2026-08-26
- **Repository basis:** program/architecture-lock at d1f3d50a948597f036868543b9bb21acacd730ff
- **Current-program condition:** maintainer work freeze; TCM0 = RESCOPE_REQUIRED; TCM1–TCM4 = LOCKED

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **TCM0R-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **TCM0R-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **TCM0R-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **TCM0R-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
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
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, an ancestor lacks an implemented ledger row, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_type_runtime -p verter_session -p verter_protocol`
2. Run every final command in the bound `docs-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's predeclared row in `authority/state/implemented.toml` from `status = "pending"` to `status = "implemented"` with the planned squash commit message, approximate date with timezone, and optional pull-request number. The transitioned row is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.

## Binding TCM0 remediation instrument

This block implements `ARCHITECT-RULING-2026-08-26-TCM0-REMEDIATION`; a summary or generic closure claim is not acceptance evidence.

- Atomize the charter into stable claim IDs. The input register contains claim IDs and proof references and schema-rejects any author-set `status`; the validator derives `OPEN`, `REFUSED`, `PROVEN-BOUNDED`, or `PROVEN` and generates the human view.
- Each claim uses an allowlisted claim-specific adapter. Its ordinary proof record names fixture inputs and the exact command and proves a terminal summary, nonzero selected work, zero unexpected skips, and internally consistent counters. It is not bound to a repository SHA, tree, or digest.
- Canonical negative controls prove unique newly-applied mutations. Covered atoms come only from applicable proof records. The validator computes `claim atoms - covered atoms`; disclosed limits or partial/sample-only proof force bounded status. A bounded claim is never acceptance-admissible without an approved transfer.
- Every remainder binds a stable residue ID, authorized non-circular owner, direct receiving-criterion ID, and resolution gate. Missing, stale, or contradictory evidence is `OPEN` or `REFUSED`; charter or authority digests are not lifecycle inputs.
- The only allowed residues are `TCM0-R-HANG-TOPOLOGY` (C2 and AD4; TCM3 then TCM4), `TCM0-R-TOPOLOGY-SELECTION` (C7; TCM2 projection and TCM3 semantic topology), and `TCM0-R-IMPLEMENTATION-BASELINE` (C9; TCM1–TCM3 pre-change comparisons and TCM4 activated verification). No fourth residue ID or other bounded row is admissible.
- Reconcile all 36 finding entries exactly: must-close `C1,C3,C4,C5,C6,C8,AR1,AR2,AR3,AR4,AR5,AR6,AR7,AR8,AR9,AR10,AR11,AR12,AR13,AR14,AR15,AD1,AD2,AD3,AD5,AD6,AD7,AD8,AD9,AD10,AD11,AD12`; residue entries `C2,C7,C9,AD4`; none are “not findings”.
- Rebuild the closure register/validator, claim universe, receiving coverage, lexical scanners, ownership ledger, transcript, gaps/summary, and downstream narratives. Delete tracked Python/POSIX controls and name-keyed scanner guards. Preserve only repaired package/binary provenance, mapper captures, semantic probes, stale-snapshot characterization, cache/lifecycle contract, acyclic test specification, five projection classes, ratified ownership decisions, concrete deletion/survivor rows, and consolidated probe 10.

The successful pre-review state is `READY_FOR_REVIEW`, never `ADMISSIBLE`. The serial gates are: (1) instrument repair alone, with current evidence `REFUSED`; (2) approve atomic claims and three residue transfers; (3) rebuild recursive subject inventory, one-method/one-capability ownership and portable controls; (4) complete package/semantic probes in parallel with (5) architectural contracts/handoffs; (6) reconcile 32 closures plus four entries in three residues; (7) assemble one complete candidate patch; (8) run three blind independent reviews, all clean PASS; (9) land the patch with its implementation-ledger row already included. A substantive change or nonzero final finding triggers proportionate fresh review, not identity restamping.

Mandatory controls cover omitted claims, forbidden input status, removed residue/owner, missing dependency, stale or inapplicable evidence, irrelevant existing proof, zero selected work, skipped work, inconsistent counters, unapplied mutation, disclosed limit, and the former bounded-to-proven laundering class.

