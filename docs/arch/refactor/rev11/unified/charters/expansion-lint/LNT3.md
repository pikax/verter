<!-- unified-charter-v2
id=LNT3
name=Initial lint packs, public parity, shared cutover, and promotion
phase=expansion
train=expansion.lint
product=lint
kind=terminal
semantic_role=delivery
class=successor
predecessors=LNT1,LNTV0,LNTS0,LNTCSS0,PUB0,PER0
conditional_predecessors=
owner=expansion.lint:demand-driven native lint service with explicit external fallback
conflict_domains=diagnostic_action_service,public_protocol,program_authority
resource_class=rust-mixed
review_profile=architecture-3
gate_profile=targeted-domain
size=S
dispatchable=true
optional=false
release_gating=product
source_refs=source:successor-expansion.md:L1464
external_requirements=
activation_gate=ORC0
charter=charters/expansion-lint/LNT3.md
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# LNT3 — Initial lint packs, public parity, shared cutover, and promotion

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Initial lint packs, public parity, shared cutover, and promotion. The current owner is **distributed diagnostics/fix rules**. The final and sole owner is **demand-driven native lint service with explicit external fallback**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_diagnostics/src`, `crates/verter_actions/src`, `crates/verter_session/src`.
- Named API/data boundaries: `RuleId`, `LintRequest`, `DiagnosticFact`, `FixTransaction`, `SuppressionProvenance`.
- Mutation boundary: bounded validation, named residual deletion, and/or one atomic route switch only; no new authority may be introduced.

## Exact predecessor contracts

- **LNT1:** exact current receipt ID and digest for “JS/TS and TypeScript-ESLint compatibility pack”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **LNTV0:** exact current receipt ID and digest for “Vue lint compatibility and Verter-native pack”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **LNTS0:** exact current receipt ID and digest for “Svelte lint compatibility and Verter-native pack”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **LNTCSS0:** exact current receipt ID and digest for “CSS and Stylelint compatibility pack”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **PUB0:** exact current receipt ID and digest for “Versioned public request/result and capability truth”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **PER0:** exact current receipt ID and digest for “Cache/backend identity, cancellation, budgets, and zero work”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Normative intent:** promote the initial JS/TS, Vue, Svelte, and CSS rule packs through one public/performance gate and remove the shared legacy registry exactly once.
- **Atomic boundary:** the production surfaces and named API/data boundaries above form this source-owned node's exclusive acceptance subset; this node owns its complete named migration population and exactly one deletion/cutover disposition.
- **Cutover evidence:** prove removal or structural rejection of **implicit ESLint/Stylelint authority**, **unsafe overlapping fix application** and satisfy the node-specific acceptance IDs below. A newly independent outcome requires an amendment and a new node before mutation.

## Acceptance IDs and discriminating proof

- **LNT3-AC1 — sole-owner proof:** add `lnt3_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **LNT3-AC2 — positive contract:** add `lnt3_publishes_exact_ruleid`; assert exact identities, provenance, completeness, and deterministic ordering.
- **LNT3-AC3 — incremental equivalence:** add `lnt3_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **LNT3-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
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

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three named independent reviewers with assigned lenses, model `gpt-5.6-sol`, effort `ultra`, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:successor-expansion.md:L1464`

## Reconciled source-plan contract

**Intent:** promote the initial JS/TS, Vue, Svelte, and CSS rule packs through one public/performance gate and remove the shared legacy registry exactly once.
**Predecessors:** `LNT1`, `LNTV0`, `LNTS0`, `LNTCSS0`, `PUB0`, `PER0`.
**Subblocks:** (1) safe/suggested/unsafe exact-basis conflict composition across all initial packs; (2) LSP diagnostics/code actions and Rust/NAPI/WASM/MCP parity; (3) generated rule/capability/config matrices; (4) cold/warm/incremental/RSS/cancellation/external-process soak; (5) consume `UAK0` ledger, prove zero legacy callers/generated rows, and atomically delete the shared registry/invocation path; (6) repository dogfood and exact-candidate reviews.
**Acceptance:** every initial pack is revalidated on the same public/performance candidate; safe fixes are idempotent; stale/untrusted edits are refused; zero legacy registry callers remain; lint promotes independently of CLI/future verticals.
**Forbidden:** implementation fixes in the terminal, auto-applying external/unsafe edits, formatter side effects, success on timed-out rules, or future-framework release coupling.
**Deletion/abort:** this block solely deletes the shared legacy rule registry/invocation path; pack defects return to their exact owner and invalidate promotion. Later framework packs require the same per-pack public/performance terminal pattern.

## 14. Unified `verter` CLI train

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-EXP-L1464-C045337DA4A6

- Kind: `context`
- Source: `successor-expansion.md:1464-1464`
- Applicability: `LNT3`
- Exact text SHA-256: `c045337da4a65f266524c35f782f343f787b32fab8bf4a3124e9b0ab542dc141`

~~~~markdown
### `LNT3.md` — Initial lint packs, public parity, shared cutover, and promotion
~~~~

### SRC-EXP-L1466-0C0CD4BC68B8

- Kind: `forbidden`
- Source: `successor-expansion.md:1466-1471`
- Applicability: `LNT3`
- Exact text SHA-256: `0c0cd4bc68b8397708e526f8ccd1101fdf1deb7714f1661509fc2026ee35cc5f`

~~~~markdown
**Intent:** promote the initial JS/TS, Vue, Svelte, and CSS rule packs through one public/performance gate and remove the shared legacy registry exactly once.
**Predecessors:** `LNT1`, `LNTV0`, `LNTS0`, `LNTCSS0`, `PUB0`, `PER0`.
**Subblocks:** (1) safe/suggested/unsafe exact-basis conflict composition across all initial packs; (2) LSP diagnostics/code actions and Rust/NAPI/WASM/MCP parity; (3) generated rule/capability/config matrices; (4) cold/warm/incremental/RSS/cancellation/external-process soak; (5) consume `UAK0` ledger, prove zero legacy callers/generated rows, and atomically delete the shared registry/invocation path; (6) repository dogfood and exact-candidate reviews.
**Acceptance:** every initial pack is revalidated on the same public/performance candidate; safe fixes are idempotent; stale/untrusted edits are refused; zero legacy registry callers remain; lint promotes independently of CLI/future verticals.
**Forbidden:** implementation fixes in the terminal, auto-applying external/unsafe edits, formatter side effects, success on timed-out rules, or future-framework release coupling.
**Deletion/abort:** this block solely deletes the shared legacy rule registry/invocation path; pack defects return to their exact owner and invalidate promotion. Later framework packs require the same per-pack public/performance terminal pattern.
~~~~
