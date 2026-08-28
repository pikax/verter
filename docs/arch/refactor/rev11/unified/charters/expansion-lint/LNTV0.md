<!-- unified-charter-v2
id=LNTV0
name=Vue lint compatibility and Verter-native pack
phase=expansion
train=expansion.lint
product=lint
kind=implementation
semantic_role=delivery
class=successor
predecessors=LNT2
conditional_predecessors=
owner=expansion.lint:demand-driven native lint service with explicit external fallback
conflict_domains=diagnostic_action_service,vue_product
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
release_gating=none
source_refs=source:successor-expansion.md:L1437
external_requirements=
activation_gate=ORC0
charter=charters/expansion-lint/LNTV0.md
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# LNTV0 — Vue lint compatibility and Verter-native pack

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Vue lint compatibility and Verter-native pack. The current owner is **distributed diagnostics/fix rules**. The final and sole owner is **demand-driven native lint service with explicit external fallback**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_diagnostics/src`, `crates/verter_actions/src`, `crates/verter_session/src`.
- Named API/data boundaries: `RuleId`, `LintRequest`, `DiagnosticFact`, `FixTransaction`, `SuppressionProvenance`.
- Mutation boundary: bounded validation, named residual deletion, and/or one atomic route switch only; no new authority may be introduced.

## Exact predecessor contracts

- **LNT2:** exact current receipt ID and digest for “Demand-driven lint service and ecosystem fallback”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Revision:** 4 — supersedes the 251-charter all-verticals proposal
- **Prepared:** 2026-08-26
- **Repository basis:** program/architecture-lock at d1f3d50a948597f036868543b9bb21acacd730ff
- **Current-program condition:** maintainer work freeze; TCM0 = RESCOPE_REQUIRED; TCM1–TCM4 = LOCKED

## Acceptance IDs and discriminating proof

- **LNTV0-AC1 — sole-owner proof:** add `lntv0_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **LNTV0-AC2 — positive contract:** add `lntv0_publishes_exact_ruleid`; assert exact identities, provenance, completeness, and deterministic ordering.
- **LNTV0-AC3 — incremental equivalence:** add `lntv0_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **LNTV0-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
- Test homes: `crates/verter_diagnostics`, `crates/verter_actions`, `crates/verter_session/tests/cases`.

## Deletions and forbidden designs

- Delete or structurally reject: **implicit ESLint/Stylelint authority**.
- Delete or structurally reject: **unsafe overlapping fix application**.
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

1. `cargo nextest run -p verter_diagnostics -p verter_actions -p verter_session`
2. Run every final command in the bound `targeted-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Re-run the planted RED mutation, restore, then GREEN, and bind both outputs to the candidate SHA/tree.

## Review and lower-severity findings

Apply `architecture-3`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates the gate and all three review verdicts. Final acceptance requires three independent PASS verdicts on the exact candidate tree; clean 3/3 means no P0/P1 and no unauthorized or undispositioned P2.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three fresh distinct harness reviewers with assigned lenses, deterministic low|medium|high effort, exact task/provider/model bindings, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:successor-expansion.md:L1437`

## Reconciled source-plan contract

**Intent:** migrate and extend Vue-specific rules/fixes under one exact Vue release profile.
**Predecessors:** `LNT2`.
**Subblocks:** (1) pin eslint-plugin-vue cells; (2) migrate existing native Vue rules; (3) add TypeInfo/template high-value cells while CE-specific rules remain owned by `VCE0`; (4) config/suppression parity; (5) authored-map-safe fixes/actions; (6) SFC/custom-block differential and performance corpus.
**Acceptance:** admitted cells have exact release applicability and locked parity; Vue-only facts never enter common rules; userland same-spelling APIs do not activate rules.
**Forbidden:** family-wide Vue rules, HTML-standard authority, formatter side effects, or unproven TypeScript facts.
**Deletion/abort:** delete only named Vue rule rows after consumer parity; shared registry deletion belongs to `LNT3`; incompatible majors become separate profiles.

## Collapsed non-authoritative subblock disposition

The recovery candidate mechanically split this source-owned atomic node into the following labels: `LNTV0-A`, `LNTV0-B`, `LNTV0-C`, `LNTV0-D`, `LNTV0-E`, `LNTV0-F`. They have no separate dispatch, lease, receipt, migration manifest, deletion ownership, or review standing. Their useful source-described concerns are internal RED/GREEN checklist items of **LNTV0**; LNTV0 alone owns the complete migration population, exactly one final deletion/cutover, and atomic acceptance. Any quoted “suggested subblock” wording in transferred source text is non-authoritative planning context.

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-EXP-L1437-755673912491

- Kind: `context`
- Source: `successor-expansion.md:1437-1437`
- Applicability: `LNTV0`
- Exact text SHA-256: `755673912491b1112bde9951d297377ea617f7ff255b22afce0e93780973d91c`

~~~~markdown
### `LNTV0.md` — Vue lint compatibility and Verter-native pack
~~~~

### SRC-EXP-L1439-E87D2F5FFA85

- Kind: `forbidden`
- Source: `successor-expansion.md:1439-1444`
- Applicability: `LNTV0`
- Exact text SHA-256: `e87d2f5ffa85ddebee133e1616b8fe5f912c694c4b3133b4b56f9aa21b9ca9ac`

~~~~markdown
**Intent:** migrate and extend Vue-specific rules/fixes under one exact Vue release profile.
**Predecessors:** `LNT2`.
**Subblocks:** (1) pin eslint-plugin-vue cells; (2) migrate existing native Vue rules; (3) add TypeInfo/template high-value cells while CE-specific rules remain owned by `VCE0`; (4) config/suppression parity; (5) authored-map-safe fixes/actions; (6) SFC/custom-block differential and performance corpus.
**Acceptance:** admitted cells have exact release applicability and locked parity; Vue-only facts never enter common rules; userland same-spelling APIs do not activate rules.
**Forbidden:** family-wide Vue rules, HTML-standard authority, formatter side effects, or unproven TypeScript facts.
**Deletion/abort:** delete only named Vue rule rows after consumer parity; shared registry deletion belongs to `LNT3`; incompatible majors become separate profiles.
~~~~
