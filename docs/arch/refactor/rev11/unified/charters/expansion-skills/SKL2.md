<!-- unified-charter-v2
id=SKL2
name=Skill forward tests and independent review receipt
phase=expansion
train=expansion.skills
product=skills
kind=verification
semantic_role=delivery
class=successor
predecessors=SKL1
conditional_predecessors=
owner=expansion.skills:manifest-derived progressive vertical planning/implementation skills
conflict_domains=program_authority
resource_class=docs-light
review_profile=semantic-3
gate_profile=docs-domain
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
source_refs=source:successor-expansion.md:L1076
external_requirements=
activation_gate=ORC0
charter=charters/expansion-skills/SKL2.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# SKL2 — Skill forward tests and independent review receipt

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Skill forward tests and independent review receipt. The current owner is **current agent workflow references**. The final and sole owner is **manifest-derived progressive vertical planning/implementation skills**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `.claude/skills`, `AGENTS.md`, `docs/arch/refactor/rev11`.
- Named API/data boundaries: `vertical manifest`, `planning route`, `implementation route`, `receipt binding`.
- Mutation boundary: authority/evidence bytes only; production LOC is zero.

## Exact predecessor contracts

- **SKL1:** exact current receipt ID and digest for “Planning and implementation workflow skills”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Normative intent:** prove the skills help fresh agents produce valid architecture rather than scaling mistakes faster.
- **Atomic boundary:** the production surfaces and named API/data boundaries above form this source-owned node's exclusive acceptance subset; this node owns its complete named migration population and exactly one deletion/cutover disposition.
- **Cutover evidence:** prove removal or structural rejection of **duplicate agent-specific authority**, **enabled skill before forward tests** and satisfy the node-specific acceptance IDs below. A newly independent outcome requires an amendment and a new node before mutation.

## Acceptance IDs and discriminating proof

- **SKL2-AC1 — sole-owner proof:** add `skl2_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **SKL2-AC2 — positive contract:** add `skl2_publishes_exact_vertical_manifest`; assert exact identities, provenance, completeness, and deterministic ordering.
- **SKL2-AC3 — incremental equivalence:** add `skl2_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **SKL2-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
- Test homes: `scripts`, `docs/arch/refactor/rev11/unified/fixtures`.

## Deletions and forbidden designs

- Delete or structurally reject: **duplicate agent-specific authority**.
- Delete or structurally reject: **enabled skill before forward tests**.
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

1. `node docs/arch/refactor/rev11/unified/tools/validate-program-dag.mjs --strict`
2. Run every final command in the bound `docs-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Re-run the planted RED mutation, restore, then GREEN, and bind both outputs to the candidate SHA/tree.

## Review and lower-severity findings

Apply `semantic-3`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates the gate and all three review verdicts. Final acceptance requires three independent PASS verdicts on the exact candidate tree; clean 3/3 means no P0/P1 and no unauthorized or undispositioned P2.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three fresh distinct harness reviewers with assigned lenses, deterministic low|medium|high effort, exact task/provider/model bindings, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:successor-expansion.md:L1076`

## Reconciled source-plan contract

**Intent:** prove the skills help fresh agents produce valid architecture rather than scaling mistakes faster.
**Predecessors:** `SKL1`.
**Subblocks:** (1) fresh-context planning tests for HTML owned carrier, React overlay, Lit embedding, Angular attachment, and Next project profile; (2) implementation dry run on a bounded synthetic vertical; (3) negative tests for Qwik1, omni parser, spelled-only `defineComponent`, missing CE/LSP/map/performance cells, and self-ratification; (4) review/fix/re-review; (5) exact-digest Codex Architect review receipt; (6) publish unresolved limitations without changing routing.
**Acceptance:** unseen agents reach the expected manifest/DAG or stop safely; deterministic validation catches every seeded violation; final exact artifacts receive independent PASS while candidates remain disabled.
**Forbidden:** testing only examples embedded in the skill, reviewer knowledge leakage, or accepting “mostly correct” output.
**Deletion/abort:** keep candidates disabled; rescope when agents consistently misinterpret a contract instead of weakening the validator.

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-EXP-L1076-F9C5AC027EE3

- Kind: `context`
- Source: `successor-expansion.md:1076-1076`
- Applicability: `SKL2`
- Exact text SHA-256: `f9c5ac027ee3965c575f99ba88991e0fee221de40f42af13a4214ca7bf04a244`

~~~~markdown
### `SKL2.md` — Skill forward tests and independent review receipt
~~~~

### SRC-EXP-L1078-78A40E878440

- Kind: `forbidden`
- Source: `successor-expansion.md:1078-1083`
- Applicability: `SKL2`
- Exact text SHA-256: `78a40e8784404d7a348da436b394ec90702c62e498bb25e2bc2cf18da7df5cf0`

~~~~markdown
**Intent:** prove the skills help fresh agents produce valid architecture rather than scaling mistakes faster.
**Predecessors:** `SKL1`.
**Subblocks:** (1) fresh-context planning tests for HTML owned carrier, React overlay, Lit embedding, Angular attachment, and Next project profile; (2) implementation dry run on a bounded synthetic vertical; (3) negative tests for Qwik1, omni parser, spelled-only `defineComponent`, missing CE/LSP/map/performance cells, and self-ratification; (4) review/fix/re-review; (5) exact-digest Codex Architect review receipt; (6) publish unresolved limitations without changing routing.
**Acceptance:** unseen agents reach the expected manifest/DAG or stop safely; deterministic validation catches every seeded violation; final exact artifacts receive independent PASS while candidates remain disabled.
**Forbidden:** testing only examples embedded in the skill, reviewer knowledge leakage, or accepting “mostly correct” output.
**Deletion/abort:** keep candidates disabled; rescope when agents consistently misinterpret a contract instead of weakening the validator.
~~~~
