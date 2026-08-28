<!-- unified-charter-v2
id=CLI2
name=Verter-native `typecheck` command
phase=expansion
train=expansion.cli
product=cli
kind=adapter
semantic_role=delivery
class=successor
predecessors=CLI1,TIF0
conditional_predecessors=
owner=expansion.cli:one `verter` application service with thin command adapters
conflict_domains=cli_application
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
source_refs=source:successor-expansion.md:L1493
external_requirements=
activation_gate=ORC0
charter=charters/expansion-cli/CLI2.md
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# CLI2 — Verter-native `typecheck` command

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Verter-native `typecheck` command. The current owner is **separate package launchers and command-local project logic**. The final and sole owner is **one `verter` application service with thin command adapters**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `packages/binary-launcher`, `packages/verter-lsp`, `packages/verter-tsc`, `crates/verter_mcp_server/src`.
- Named API/data boundaries: `ApplicationServices`, `SelectionPlan`, `Reporter`, `WriteTransaction`, `CommandCapability`.
- Mutation boundary: bounded validation, named residual deletion, and/or one atomic route switch only; no new authority may be introduced.

## Exact predecessor contracts

- **CLI1:** exact current receipt ID and digest for “Shared application services, selection, invocation, reporters”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **TIF0:** exact current receipt ID and digest for “TypeInfo query/selector and authority-composition contract”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Revision:** 4 — supersedes the 251-charter all-verticals proposal
- **Prepared:** 2026-08-26
- **Repository basis:** program/architecture-lock at d1f3d50a948597f036868543b9bb21acacd730ff
- **Current-program condition:** maintainer work freeze; TCM0 = RESCOPE_REQUIRED; TCM1–TCM4 = LOCKED

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **CLI2-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **CLI2-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **CLI2-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **CLI2-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
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
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, a predecessor receipt is stale, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `pnpm --filter @verter/binary-launcher test`
2. Run every final command in the bound `ts-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the exact candidate tree, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact frozen worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; 3 fresh distinct harness review tasks for exactly `adversarial`, `conformance`, `architecture-specialist`, deterministic low|medium|high effort and exact author/task/agent/provider/model bindings; immutable-review-worktree and cleanup policy; and the required terse report-back schema. These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:successor-expansion.md:L1493`

## Reconciled source-plan contract

**Intent:** expose the composed Verter diagnostic plan as a non-emitting command distinct from the TypeScript-compatible driver.
**Predecessors:** `CLI1`, `TIF0`.
**Subblocks:** (1) select exact carrier/framework/project profiles; (2) compose only native/framework type diagnostics and certified TypeScript observations according to their owners; (3) return provenance/completeness/NeedInputs; (4) enforce zero filesystem writes and exclude lint/formatting; (5) project/reference/watch inputs; (6) incremental/fresh/differential/performance tests.
**Acceptance:** `verter typecheck` means Verter’s composed native/framework/TS diagnostic plan and writes nothing; it is not an alias for `tsc --noEmit`; unavailable owners produce truthful partial/NeedInputs results.
**Forbidden:** emit, CLI-owned diagnostics, creating a second TS program, silently selecting the first project, or collapsing partiality to success.
**Deletion/abort:** replace only the old typecheck shell after service parity; abort if any diagnostic lacks an exact owner/basis.

## Collapsed non-authoritative subblock disposition

The recovery candidate mechanically split this source-owned atomic node into the following labels: `CLI2-A`, `CLI2-B`, `CLI2-C`, `CLI2-D`, `CLI2-E`, `CLI2-F`. They have no separate dispatch, lease, receipt, migration manifest, deletion ownership, or review standing. Their useful source-described concerns are internal RED/GREEN checklist items of **CLI2**; CLI2 alone owns the complete migration population, exactly one final deletion/cutover, and atomic acceptance. Any quoted “suggested subblock” wording in transferred source text is non-authoritative planning context.

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-EXP-L1493-9D8415D01B7E

- Kind: `context`
- Source: `successor-expansion.md:1493-1493`
- Applicability: `CLI2`
- Exact text SHA-256: `9d8415d01b7eaa3440de6a70868034db0f62cc478394a4107a7e030cb8ceebc5`

~~~~markdown
### `CLI2.md` — Verter-native `typecheck` command
~~~~

### SRC-EXP-L1495-5096FB413805

- Kind: `forbidden`
- Source: `successor-expansion.md:1495-1500`
- Applicability: `CLI2`
- Exact text SHA-256: `5096fb41380518ce3b077d4b5e9faa67c797206cebf7a561995d1a2fa5048516`

~~~~markdown
**Intent:** expose the composed Verter diagnostic plan as a non-emitting command distinct from the TypeScript-compatible driver.
**Predecessors:** `CLI1`, `TIF0`.
**Subblocks:** (1) select exact carrier/framework/project profiles; (2) compose only native/framework type diagnostics and certified TypeScript observations according to their owners; (3) return provenance/completeness/NeedInputs; (4) enforce zero filesystem writes and exclude lint/formatting; (5) project/reference/watch inputs; (6) incremental/fresh/differential/performance tests.
**Acceptance:** `verter typecheck` means Verter’s composed native/framework/TS diagnostic plan and writes nothing; it is not an alias for `tsc --noEmit`; unavailable owners produce truthful partial/NeedInputs results.
**Forbidden:** emit, CLI-owned diagnostics, creating a second TS program, silently selecting the first project, or collapsing partiality to success.
**Deletion/abort:** replace only the old typecheck shell after service parity; abort if any diagnostic lacks an exact owner/basis.
~~~~

### SRC-EXISTING-NODE-AMENDMENT-CLI2

- Kind: `requirement`
- Source: `existing-node-amendments.md:209-220`
- Applicability: `CLI2`
- Exact text SHA-256: `0f3b77770cdf6bb1ea123621320f84747548717655a92cf057ef9de97d217be3`

~~~~markdown
## CLI2 — Verter-native typecheck

When opened after NCK7:

- consume `DiagnosticService` rather than constructing a checker/provider/project plan;
- select project/profile scope explicitly;
- write nothing;
- return exact provenance/completeness/NeedInputs;
- exclude lint and formatting unless the command explicitly composes them at a higher application-service layer;
- avoid aliasing `tsc --noEmit`.

Before NCK7, CLI2 may use external/native existing owners according to its current contract, but must not pre-empt NCK authority.
~~~~
