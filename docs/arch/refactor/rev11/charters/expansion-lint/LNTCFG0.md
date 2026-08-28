<!-- unified-charter-v2
id=LNTCFG0
name=Verter lint configuration and ecosystem translators
phase=expansion
train=expansion.lint
product=lint
kind=translator
semantic_role=delivery
class=successor
predecessors=LNT0,LRA0,CFG0
conditional_predecessors=
owner=expansion.lint:demand-driven native lint service with explicit external fallback
conflict_domains=diagnostic_action_service
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
source_refs=source:successor-expansion.md:L1410
external_requirements=
activation_gate=ORC0
charter=charters/expansion-lint/LNTCFG0.md
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# LNTCFG0 — Verter lint configuration and ecosystem translators

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Verter lint configuration and ecosystem translators. The current owner is **distributed diagnostics/fix rules**. The final and sole owner is **demand-driven native lint service with explicit external fallback**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_diagnostics/src`, `crates/verter_actions/src`, `crates/verter_session/src`.
- Named API/data boundaries: `RuleId`, `LintRequest`, `DiagnosticFact`, `FixTransaction`, `SuppressionProvenance`.
- Mutation boundary: bounded validation, named residual deletion, and/or one atomic route switch only; no new authority may be introduced.

## Exact predecessor contracts

- **LNT0:** exact current receipt ID and digest for “Native lint product and compatibility lock”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **LRA0:** exact current receipt ID and digest for “Profile-scoped diagnostics, lint, fixes, and actions”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **CFG0:** exact current receipt ID and digest for “Declarative Verter and captured ecosystem configuration”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Revision:** 4 — supersedes the 251-charter all-verticals proposal
- **Prepared:** 2026-08-26
- **Repository basis:** program/architecture-lock at d1f3d50a948597f036868543b9bb21acacd730ff
- **Current-program condition:** maintainer work freeze; TCM0 = RESCOPE_REQUIRED; TCM1–TCM4 = LOCKED

## Acceptance IDs and discriminating proof

- **LNTCFG0-AC1 — sole-owner proof:** add `lntcfg0_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **LNTCFG0-AC2 — positive contract:** add `lntcfg0_publishes_exact_ruleid`; assert exact identities, provenance, completeness, and deterministic ordering.
- **LNTCFG0-AC3 — incremental equivalence:** add `lntcfg0_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **LNTCFG0-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
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

- `source:successor-expansion.md:L1410`

## Reconciled source-plan contract

**Intent:** own the Verter lint schema and translate captured ecosystem configuration after the exact rule vocabulary exists.
**Predecessors:** `LNT0`, `LRA0`, `CFG0`.
**Subblocks:** (1) versioned `lint` section in `verter.config.jsonc`; (2) exact per-language/per-framework rule namespaces and overrides; (3) static ESLint/TS-ESLint/Vue/Svelte/Stylelint translators; (4) suppression/severity/fix-policy provenance; (5) unknown/inapplicable/external-only/cycle/trust outcomes; (6) schema generation, invalidation, and differential config corpus.
**Acceptance:** Verter-only rules configure without pretending to be ecosystem rules; supported ecosystem configs translate deterministically; unknown rule/option fails closed; profile overrides do not leak across framework releases.
**Forbidden:** arbitrary JS config execution in Rust, silent fallback, a flat cross-framework rules map, translator logic in `CFG0`, or external plugin execution.
**Deletion/abort:** delete duplicate lint config readers only after all consumers move; executable config remains an explicit trusted-host input.

## Collapsed non-authoritative subblock disposition

The recovery candidate mechanically split this source-owned atomic node into the following labels: `LNTCFG0-A`, `LNTCFG0-B`, `LNTCFG0-C`, `LNTCFG0-D`, `LNTCFG0-E`, `LNTCFG0-F`. They have no separate dispatch, lease, receipt, migration manifest, deletion ownership, or review standing. Their useful source-described concerns are internal RED/GREEN checklist items of **LNTCFG0**; LNTCFG0 alone owns the complete migration population, exactly one final deletion/cutover, and atomic acceptance. Any quoted “suggested subblock” wording in transferred source text is non-authoritative planning context.

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-EXP-L1410-2160B340D728

- Kind: `context`
- Source: `successor-expansion.md:1410-1410`
- Applicability: `LNTCFG0`
- Exact text SHA-256: `2160b340d728ed65d6de1deb7a295488c48d7dc9d336e57cc09515dd4c8f5f16`

~~~~markdown
### `LNTCFG0.md` — Verter lint configuration and ecosystem translators
~~~~

### SRC-EXP-L1412-D05896206E2C

- Kind: `forbidden`
- Source: `successor-expansion.md:1412-1417`
- Applicability: `LNTCFG0`
- Exact text SHA-256: `d05896206e2ccedf2f2fd74563782fc31b54d216cbe926dbb1cbf7e668a01ebd`

~~~~markdown
**Intent:** own the Verter lint schema and translate captured ecosystem configuration after the exact rule vocabulary exists.
**Predecessors:** `LNT0`, `LRA0`, `CFG0`.
**Subblocks:** (1) versioned `lint` section in `verter.config.jsonc`; (2) exact per-language/per-framework rule namespaces and overrides; (3) static ESLint/TS-ESLint/Vue/Svelte/Stylelint translators; (4) suppression/severity/fix-policy provenance; (5) unknown/inapplicable/external-only/cycle/trust outcomes; (6) schema generation, invalidation, and differential config corpus.
**Acceptance:** Verter-only rules configure without pretending to be ecosystem rules; supported ecosystem configs translate deterministically; unknown rule/option fails closed; profile overrides do not leak across framework releases.
**Forbidden:** arbitrary JS config execution in Rust, silent fallback, a flat cross-framework rules map, translator logic in `CFG0`, or external plugin execution.
**Deletion/abort:** delete duplicate lint config readers only after all consumers move; executable config remains an explicit trusted-host input.
~~~~
