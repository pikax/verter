<!-- unified-charter-v2
id=LNT0
name=Native lint product and compatibility lock
phase=expansion
train=expansion.lint
product=lint
kind=lock
semantic_role=delivery
class=successor
predecessors=LRA0,CFG0
conditional_predecessors=
owner=expansion.lint:demand-driven native lint service with explicit external fallback
conflict_domains=diagnostic_action_service
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
size=M
dispatchable=true
optional=false
release_gating=none
source_refs=source:successor-expansion.md:L1401
external_requirements=
activation_gate=ORC0
charter=charters/expansion-lint/LNT0.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# LNT0 — Native lint product and compatibility lock

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Native lint product and compatibility lock. The current owner is **distributed diagnostics/fix rules**. The final and sole owner is **demand-driven native lint service with explicit external fallback**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_diagnostics/src`, `crates/verter_actions/src`, `crates/verter_session/src`.
- Named API/data boundaries: `RuleId`, `LintRequest`, `DiagnosticFact`, `FixTransaction`, `SuppressionProvenance`.
- Mutation boundary: authority/evidence bytes only; production LOC is zero.

## Exact predecessor contracts

- **LRA0:** exact current receipt ID and digest for “Profile-scoped diagnostics, lint, fixes, and actions”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **CFG0:** exact current receipt ID and digest for “Declarative Verter and captured ecosystem configuration”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Normative intent:** freeze the native/equivalent/external rule universe and product claims without inventing another lint engine.
- **Atomic boundary:** the production surfaces and named API/data boundaries above form this source-owned node's exclusive acceptance subset; this node owns its complete named migration population and exactly one deletion/cutover disposition.
- **Cutover evidence:** prove removal or structural rejection of **implicit ESLint/Stylelint authority**, **unsafe overlapping fix application** and satisfy the node-specific acceptance IDs below. A newly independent outcome requires an amendment and a new node before mutation.

## Acceptance IDs and discriminating proof

- **LNT0-AC1 — sole-owner proof:** add `lnt0_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **LNT0-AC2 — positive contract:** add `lnt0_publishes_exact_ruleid`; assert exact identities, provenance, completeness, and deterministic ordering.
- **LNT0-AC3 — incremental equivalence:** add `lnt0_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **LNT0-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
- Test homes: `crates/verter_diagnostics`, `crates/verter_actions`, `crates/verter_session/tests/cases`.

## Deletions and forbidden designs

- Delete or structurally reject: **implicit ESLint/Stylelint authority**.
- Delete or structurally reject: **unsafe overlapping fix application**.
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

1. `cargo nextest run -p verter_diagnostics -p verter_actions -p verter_session`
2. Run every final command in the bound `docs-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Re-run the planted RED mutation, restore, then GREEN, and bind both outputs to the candidate SHA/tree.

## Review and lower-severity findings

Apply `architecture-3`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates the gate and all three review verdicts. Final acceptance requires three independent PASS verdicts on the exact candidate tree; clean 3/3 means no P0/P1 and no unauthorized or undispositioned P2.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three fresh distinct harness reviewers with assigned lenses, deterministic low|medium|high effort, exact task/provider/model bindings, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:successor-expansion.md:L1401`

## Reconciled source-plan contract

**Intent:** freeze the native/equivalent/external rule universe and product claims without inventing another lint engine.
**Predecessors:** `LRA0`, `CFG0`.
**Subblocks:** (1) inventory current Verter rules and fixes; (2) pin ESLint, TypeScript-ESLint, eslint-plugin-vue, Svelte, Stylelint, and relevant framework rule versions; (3) classify NativeEquivalent/VerterOnly/ExternalOnly/Unsupported cells; (4) lock diagnostic/fix compatibility; (5) lock corpus/performance/zero-work gates; (6) ratify config and external-runner policy.
**Acceptance:** no blanket “ESLint compatible” claim; every rule ID has exact applicability, owner, fact demand, oracle, and fix safety.
**Forbidden:** running arbitrary plugins in core, claiming compatibility from similar names, or choosing easy rules after implementation.
**Deletion/abort:** no code; rescope incompatible semantic rules explicitly.

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-EXP-L1401-D279D4260805

- Kind: `context`
- Source: `successor-expansion.md:1401-1401`
- Applicability: `LNT0`
- Exact text SHA-256: `d279d42608057d0c725be38465501b7fd5150b21f7e9d3ef8e36ff0b25ee124c`

~~~~markdown
### `LNT0.md` — Native lint product and compatibility lock
~~~~

### SRC-EXP-L1403-50E84741BCC6

- Kind: `forbidden`
- Source: `successor-expansion.md:1403-1408`
- Applicability: `LNT0`
- Exact text SHA-256: `50e84741bcc68363f5b36c06513e30b8aab55646ac70ac9a9d2ddc75fd3747eb`

~~~~markdown
**Intent:** freeze the native/equivalent/external rule universe and product claims without inventing another lint engine.
**Predecessors:** `LRA0`, `CFG0`.
**Subblocks:** (1) inventory current Verter rules and fixes; (2) pin ESLint, TypeScript-ESLint, eslint-plugin-vue, Svelte, Stylelint, and relevant framework rule versions; (3) classify NativeEquivalent/VerterOnly/ExternalOnly/Unsupported cells; (4) lock diagnostic/fix compatibility; (5) lock corpus/performance/zero-work gates; (6) ratify config and external-runner policy.
**Acceptance:** no blanket “ESLint compatible” claim; every rule ID has exact applicability, owner, fact demand, oracle, and fix safety.
**Forbidden:** running arbitrary plugins in core, claiming compatibility from similar names, or choosing easy rules after implementation.
**Deletion/abort:** no code; rescope incompatible semantic rules explicitly.
~~~~
