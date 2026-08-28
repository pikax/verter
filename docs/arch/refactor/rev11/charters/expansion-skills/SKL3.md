<!-- unified-charter-v2
id=SKL3
name=Maintainer-ratified atomic workflow activation
phase=expansion
train=expansion.skills
product=skills
kind=cutover
semantic_role=delivery
class=successor
predecessors=SKL2
conditional_predecessors=
owner=expansion.skills:manifest-derived progressive vertical planning/implementation skills
conflict_domains=semantic_authority,performance_evidence
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
size=S
dispatchable=true
optional=false
release_gating=workflow
source_refs=source:successor-expansion.md:L1085
external_requirements=
activation_gate=ORC0
charter=charters/expansion-skills/SKL3.md
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# SKL3 — Maintainer-ratified atomic workflow activation

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Maintainer-ratified atomic workflow activation. The current owner is **current agent workflow references**. The final and sole owner is **manifest-derived progressive vertical planning/implementation skills**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `.claude/skills`, `AGENTS.md`, `docs/arch/refactor/rev11`.
- Named API/data boundaries: `vertical manifest`, `planning route`, `implementation route`, `receipt binding`.
- Mutation boundary: bounded validation, named residual deletion, and/or one atomic route switch only; no new authority may be introduced.

## Exact predecessor contracts

- **SKL2:** exact current receipt ID and digest for “Skill forward tests and independent review receipt”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Revision:** 4 — supersedes the 251-charter all-verticals proposal
- **Prepared:** 2026-08-26
- **Repository basis:** program/architecture-lock at d1f3d50a948597f036868543b9bb21acacd730ff
- **Current-program condition:** maintainer work freeze; TCM0 = RESCOPE_REQUIRED; TCM1–TCM4 = LOCKED

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **SKL3-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **SKL3-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **SKL3-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **SKL3-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `scripts`, `docs/arch/refactor/rev11/fixtures`.

## Deletions and forbidden designs

- Delete or structurally reject: **duplicate agent-specific authority**.
- Delete or structurally reject: **enabled skill before forward tests**.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 300 production LOC, 3 production files, 1 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, a predecessor receipt is stale, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `node docs/arch/refactor/rev11/tools/validate-program-dag.mjs --strict`
2. Run every final command in the bound `targeted-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the exact candidate tree, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact frozen worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; 3 fresh distinct harness review tasks for exactly `adversarial`, `conformance`, `architecture-specialist`, deterministic low|medium|high effort and exact author/task/agent/provider/model bindings; immutable-review-worktree and cleanup policy; and the required terse report-back schema. These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:successor-expansion.md:L1085`

## Reconciled source-plan contract

**Intent:** switch repository routing to the reviewed skills atomically, with no interval containing zero or two active integration workflows.
**Predecessors:** `SKL2`.
**Subblocks:** (1) verify the `SKL2` semantic/test receipt; (2) stage the complete skills+AGENTS+discovery+old-workflow-retirement cutover candidate; (3) run fresh routing/negative tests and independent Codex Architect review on that exact tree; (4) obtain explicit maintainer adoption over the reviewed digest; (5) land one equivalent atomic commit; (6) verify landing equivalence and rollback restoration.
**Acceptance:** exactly one lifecycle-paired workflow is active before and after cutover; review and adoption both bind the complete cutover tree; any fix invalidates both receipts; rollback restores the old routing atomically.
**Forbidden:** self-ratification, activation before review, deletion before replacement, two competing active entry points, or manual post-landing edits.
**Deletion/abort:** retire only the old invocable entry point and duplicate routing after zero-consumer proof; abort and keep the old workflow active on any digest/routing mismatch.

## 10. First architecture implementation: HTML + Custom Elements

## Collapsed non-authoritative subblock disposition

The recovery candidate mechanically split this source-owned atomic node into the following labels: `SKL3-A`, `SKL3-B`, `SKL3-C`, `SKL3-D`, `SKL3-E`, `SKL3-F`. They have no separate dispatch, lease, receipt, migration manifest, deletion ownership, or review standing. Their useful source-described concerns are internal RED/GREEN checklist items of **SKL3**; SKL3 alone owns the complete migration population, exactly one final deletion/cutover, and atomic acceptance. Any quoted “suggested subblock” wording in transferred source text is non-authoritative planning context.

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-EXP-L1085-C7DE94B69FDB

- Kind: `context`
- Source: `successor-expansion.md:1085-1085`
- Applicability: `SKL3`
- Exact text SHA-256: `c7de94b69fdbd3c27ffab4864ace128e23a8cea7d8f5218ed8b6396e45905c67`

~~~~markdown
### `SKL3.md` — Maintainer-ratified atomic workflow activation
~~~~

### SRC-EXP-L1087-92E645D3EA33

- Kind: `forbidden`
- Source: `successor-expansion.md:1087-1092`
- Applicability: `SKL3`
- Exact text SHA-256: `92e645d3ea333022641dfe58fef8147951bf8ab88a6974645ef3216e243b7626`

~~~~markdown
**Intent:** switch repository routing to the reviewed skills atomically, with no interval containing zero or two active integration workflows.
**Predecessors:** `SKL2`.
**Subblocks:** (1) verify the `SKL2` semantic/test receipt; (2) stage the complete skills+AGENTS+discovery+old-workflow-retirement cutover candidate; (3) run fresh routing/negative tests and independent Codex Architect review on that exact tree; (4) obtain explicit maintainer adoption over the reviewed digest; (5) land one equivalent atomic commit; (6) verify landing equivalence and rollback restoration.
**Acceptance:** exactly one lifecycle-paired workflow is active before and after cutover; review and adoption both bind the complete cutover tree; any fix invalidates both receipts; rollback restores the old routing atomically.
**Forbidden:** self-ratification, activation before review, deletion before replacement, two competing active entry points, or manual post-landing edits.
**Deletion/abort:** retire only the old invocable entry point and duplicate routing after zero-consumer proof; abort and keep the old workflow active on any digest/routing mismatch.
~~~~
