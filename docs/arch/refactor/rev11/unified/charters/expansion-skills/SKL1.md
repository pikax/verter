<!-- unified-charter-v2
id=SKL1
name=Planning and implementation workflow skills
phase=expansion
train=expansion.skills
product=skills
kind=implementation
semantic_role=delivery
class=successor
predecessors=SKL0,VIM1
conditional_predecessors=
owner=expansion.skills:manifest-derived progressive vertical planning/implementation skills
conflict_domains=semantic_authority,performance_evidence
resource_class=rust-mixed
review_profile=architecture-3
gate_profile=targeted-domain
size=S
dispatchable=true
optional=false
release_gating=none
source_refs=source:successor-expansion.md:L1067
external_requirements=
activation_gate=ORC0
charter=charters/expansion-skills/SKL1.md
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# SKL1 — Planning and implementation workflow skills

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Planning and implementation workflow skills. The current owner is **current agent workflow references**. The final and sole owner is **manifest-derived progressive vertical planning/implementation skills**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `.claude/skills`, `AGENTS.md`, `docs/arch/refactor/rev11`.
- Named API/data boundaries: `vertical manifest`, `planning route`, `implementation route`, `receipt binding`.
- Mutation boundary: bounded validation, named residual deletion, and/or one atomic route switch only; no new authority may be introduced.

## Exact predecessor contracts

- **SKL0:** exact current receipt ID and digest for “Existing skill audit and progressive-reference migration”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **VIM1:** exact current receipt ID and digest for “Deterministic manifest compiler and conformance generator”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Revision:** 4 — supersedes the 251-charter all-verticals proposal
- **Prepared:** 2026-08-26
- **Repository basis:** program/architecture-lock at d1f3d50a948597f036868543b9bb21acacd730ff
- **Current-program condition:** maintainer work freeze; TCM0 = RESCOPE_REQUIRED; TCM1–TCM4 = LOCKED

## Acceptance IDs and discriminating proof

- **SKL1-AC1 — sole-owner proof:** add `skl1_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **SKL1-AC2 — positive contract:** add `skl1_publishes_exact_vertical_manifest`; assert exact identities, provenance, completeness, and deterministic ordering.
- **SKL1-AC3 — incremental equivalence:** add `skl1_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **SKL1-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
- Test homes: `scripts`, `docs/arch/refactor/rev11/unified/fixtures`.

## Deletions and forbidden designs

- Delete or structurally reject: **duplicate agent-specific authority**.
- Delete or structurally reject: **enabled skill before forward tests**.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 300 production LOC, 3 production files, 1 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing.
- Performance budget: equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance is 0.0% unless an owning-authority amendment supplies exact replacement thresholds; after warmup, retained bytes may not increase across 100 identical requests.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, a predecessor receipt is stale, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `node docs/arch/refactor/rev11/unified/tools/validate-program-dag.mjs --strict`
2. Run every final command in the bound `targeted-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Re-run the planted RED mutation, restore, then GREEN, and bind both outputs to the candidate SHA/tree.

## Review and lower-severity findings

Apply `architecture-3`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates the gate and all three review verdicts. Final acceptance requires three independent PASS verdicts on the exact candidate tree; clean 3/3 means no P0/P1 and no unauthorized or undispositioned P2.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three named independent reviewers with assigned lenses, model `gpt-5.6-sol`, effort `ultra`, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:successor-expansion.md:L1067`

## Reconciled source-plan contract

**Intent:** install disabled candidate workflows split by lifecycle rather than “language” versus “framework.”
**Predecessors:** `SKL0`, `VIM1`.
**Subblocks:** (1) create lean disabled `plan-verter-vertical`; (2) create lean disabled `implement-verter-vertical`; (3) add geometry recipes for owned carrier, embedded language, attached language, semantic overlay, HTML attribute overlay, project profile, and CE producer/consumer; (4) bind both to `cargo xtask vertical`; (5) require exact SHA/manifest/charter/authority digests; (6) define false-premise/new-authority stop rules and independent review handoff.
**Acceptance:** planning is read-only and stops at ready-for-ratification; implementation accepts exactly one ratified bounded subblock and cannot redesign or accept it; neither duplicates validator logic; neither is reachable from AGENTS or normal skill discovery.
**Forbidden:** one plan-and-write skill, language/framework split, self-ratification, guessed version/oracle, or bypassing a failed manifest check.
**Deletion/abort:** remove nothing and preserve the old active workflow; abort if a candidate needs repository authority not represented by a ratified manifest/charter.

## Collapsed non-authoritative subblock disposition

The recovery candidate mechanically split this source-owned atomic node into the following labels: `SKL1-A`, `SKL1-B`, `SKL1-C`, `SKL1-D`, `SKL1-E`, `SKL1-F`. They have no separate dispatch, lease, receipt, migration manifest, deletion ownership, or review standing. Their useful source-described concerns are internal RED/GREEN checklist items of **SKL1**; SKL1 alone owns the complete migration population, exactly one final deletion/cutover, and atomic acceptance. Any quoted “suggested subblock” wording in transferred source text is non-authoritative planning context.

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-EXP-L1067-B95D137D7432

- Kind: `context`
- Source: `successor-expansion.md:1067-1067`
- Applicability: `SKL1`
- Exact text SHA-256: `b95d137d74325270a098d19cda9654d65267de6db7ed496beef599f0e7668455`

~~~~markdown
### `SKL1.md` — Planning and implementation workflow skills
~~~~

### SRC-EXP-L1069-326218D68C28

- Kind: `forbidden`
- Source: `successor-expansion.md:1069-1074`
- Applicability: `SKL1`
- Exact text SHA-256: `326218d68c28f4e5d3a796dd9e80350252290d4f3bd4a7e049e62d0966db7a2e`

~~~~markdown
**Intent:** install disabled candidate workflows split by lifecycle rather than “language” versus “framework.”
**Predecessors:** `SKL0`, `VIM1`.
**Subblocks:** (1) create lean disabled `plan-verter-vertical`; (2) create lean disabled `implement-verter-vertical`; (3) add geometry recipes for owned carrier, embedded language, attached language, semantic overlay, HTML attribute overlay, project profile, and CE producer/consumer; (4) bind both to `cargo xtask vertical`; (5) require exact SHA/manifest/charter/authority digests; (6) define false-premise/new-authority stop rules and independent review handoff.
**Acceptance:** planning is read-only and stops at ready-for-ratification; implementation accepts exactly one ratified bounded subblock and cannot redesign or accept it; neither duplicates validator logic; neither is reachable from AGENTS or normal skill discovery.
**Forbidden:** one plan-and-write skill, language/framework split, self-ratification, guessed version/oracle, or bypassing a failed manifest check.
**Deletion/abort:** remove nothing and preserve the old active workflow; abort if a candidate needs repository authority not represented by a ratified manifest/charter.
~~~~
