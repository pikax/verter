<!-- unified-charter-v2
id=CLIL0
name=Lint CLI adapter
phase=expansion
train=expansion.cli
product=cli
kind=adapter
semantic_role=delivery
class=successor
predecessors=CLI1,LNT3
conditional_predecessors=
owner=expansion.cli:one `verter` application service with thin command adapters
conflict_domains=diagnostic_action_service,cli_application
resource_class=ts-heavy
review_profile=architecture-3
gate_profile=ts-domain
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
source_refs=source:successor-expansion.md:L1547
external_requirements=
activation_gate=ORC0
charter=charters/expansion-cli/CLIL0.md
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# CLIL0 — Lint CLI adapter

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Lint CLI adapter. The current owner is **separate package launchers and command-local project logic**. The final and sole owner is **one `verter` application service with thin command adapters**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `packages/binary-launcher`, `packages/verter-lsp`, `packages/verter-tsc`, `crates/verter_mcp_server/src`.
- Named API/data boundaries: `ApplicationServices`, `SelectionPlan`, `Reporter`, `WriteTransaction`, `CommandCapability`.
- Mutation boundary: bounded validation, named residual deletion, and/or one atomic route switch only; no new authority may be introduced.

## Exact predecessor contracts

- **CLI1:** exact current receipt ID and digest for “Shared application services, selection, invocation, reporters”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **LNT3:** exact current receipt ID and digest for “Initial lint packs, public parity, shared cutover, and promotion”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Revision:** 4 — supersedes the 251-charter all-verticals proposal
- **Prepared:** 2026-08-26
- **Repository basis:** program/architecture-lock at d1f3d50a948597f036868543b9bb21acacd730ff
- **Current-program condition:** maintainer work freeze; TCM0 = RESCOPE_REQUIRED; TCM1–TCM4 = LOCKED

## Acceptance IDs and discriminating proof

- **CLIL0-AC1 — sole-owner proof:** add `clil0_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **CLIL0-AC2 — positive contract:** add `clil0_publishes_exact_applicationservices`; assert exact identities, provenance, completeness, and deterministic ordering.
- **CLIL0-AC3 — incremental equivalence:** add `clil0_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **CLIL0-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
- Test homes: `packages/binary-launcher/cli.spec.ts`, `crates/verter_mcp_server/tests`.

## Deletions and forbidden designs

- Delete or structurally reject: **command-local semantic engine**.
- Delete or structurally reject: **non-atomic multi-file writes**.
- Delete or structurally reject: **ambiguous offset encoding**.
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

1. `pnpm --filter @verter/binary-launcher test`
2. Run every final command in the bound `ts-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Re-run the planted RED mutation, restore, then GREEN, and bind both outputs to the candidate SHA/tree.

## Review and lower-severity findings

Apply `architecture-3`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates the gate and all three review verdicts. Final acceptance requires three independent PASS verdicts on the exact candidate tree; clean 3/3 means no P0/P1 and no unauthorized or undispositioned P2.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three fresh distinct harness reviewers with assigned lenses, deterministic low|medium|high effort, exact task/provider/model bindings, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:successor-expansion.md:L1547`

## Reconciled source-plan contract

**Intent:** add `verter lint` as a thin adapter over the independently promoted lint service and available rule packs.
**Predecessors:** `CLI1`, `LNT3`.
**Subblocks:** (1) file/project/stdin selection; (2) report/fix-policy flags; (3) native/external provenance and trust inputs; (4) human/JSON/SARIF reporters; (5) safe-fix preview/atomic write; (6) watch/performance/cancellation/failure tests.
**Acceptance:** process failure/timeout is not clean lint; `lint` writes only under an explicit safe-fix flag; available pack/capability truth is generated; CLI owns no rules.
**Forbidden:** arbitrary plugin execution in Rust, implicit fixes, duplicated diagnostics, formatter side effects, or CLI-owned suppression semantics.
**Deletion/abort:** delete standalone lint shells only after parity and zero consumers; disable external fallback unless its trusted-host gates pass.

## Collapsed non-authoritative subblock disposition

The recovery candidate mechanically split this source-owned atomic node into the following labels: `CLIL0-A`, `CLIL0-B`, `CLIL0-C`, `CLIL0-D`, `CLIL0-E`, `CLIL0-F`. They have no separate dispatch, lease, receipt, migration manifest, deletion ownership, or review standing. Their useful source-described concerns are internal RED/GREEN checklist items of **CLIL0**; CLIL0 alone owns the complete migration population, exactly one final deletion/cutover, and atomic acceptance. Any quoted “suggested subblock” wording in transferred source text is non-authoritative planning context.

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-EXP-L1547-6E35C43139AA

- Kind: `context`
- Source: `successor-expansion.md:1547-1547`
- Applicability: `CLIL0`
- Exact text SHA-256: `6e35c43139aafdaf28daa148ad512735c4b603561adcc6e7375dd3d2245d124a`

~~~~markdown
### `CLIL0.md` — Lint CLI adapter
~~~~

### SRC-EXP-L1549-01620671A9DC

- Kind: `forbidden`
- Source: `successor-expansion.md:1549-1554`
- Applicability: `CLIL0`
- Exact text SHA-256: `01620671a9dc912bd7b5e5abb8ed2dc89dfcabb5ece3bf851ff152cccc1a5725`

~~~~markdown
**Intent:** add `verter lint` as a thin adapter over the independently promoted lint service and available rule packs.
**Predecessors:** `CLI1`, `LNT3`.
**Subblocks:** (1) file/project/stdin selection; (2) report/fix-policy flags; (3) native/external provenance and trust inputs; (4) human/JSON/SARIF reporters; (5) safe-fix preview/atomic write; (6) watch/performance/cancellation/failure tests.
**Acceptance:** process failure/timeout is not clean lint; `lint` writes only under an explicit safe-fix flag; available pack/capability truth is generated; CLI owns no rules.
**Forbidden:** arbitrary plugin execution in Rust, implicit fixes, duplicated diagnostics, formatter side effects, or CLI-owned suppression semantics.
**Deletion/abort:** delete standalone lint shells only after parity and zero consumers; disable external fallback unless its trusted-host gates pass.
~~~~
