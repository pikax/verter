<!-- unified-charter-v2
id=CLI0
name=`verter` command/package and semantic lock
phase=expansion
train=expansion.cli
product=cli
kind=lock
semantic_role=delivery
class=successor
predecessors=PUB0
conditional_predecessors=
owner=expansion.cli:one `verter` application service with thin command adapters
conflict_domains=cli_application,semantic_authority
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
source_refs=source:successor-expansion.md:L1475
external_requirements=
activation_gate=ORC0
charter=charters/expansion-cli/CLI0.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# CLI0 — `verter` command/package and semantic lock

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

`verter` command/package and semantic lock. The current owner is **separate package launchers and command-local project logic**. The final and sole owner is **one `verter` application service with thin command adapters**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `packages/binary-launcher`, `packages/verter-lsp`, `packages/verter-tsc`, `crates/verter_mcp_server/src`.
- Named API/data boundaries: `ApplicationServices`, `SelectionPlan`, `Reporter`, `WriteTransaction`, `CommandCapability`.
- Mutation boundary: authority/evidence bytes only; production LOC is zero.

## Exact predecessor contracts

- **PUB0:** exact current receipt ID and digest for “Versioned public request/result and capability truth”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Normative intent:** freeze one executable surface and distinct command semantics before building the shell.
- **Atomic boundary:** the production surfaces and named API/data boundaries above form this source-owned node's exclusive acceptance subset; this node owns its complete named migration population and exactly one deletion/cutover disposition.
- **Cutover evidence:** prove removal or structural rejection of **command-local semantic engine**, **non-atomic multi-file writes**, **ambiguous offset encoding** and satisfy the node-specific acceptance IDs below. A newly independent outcome requires an amendment and a new node before mutation.

## Acceptance IDs and discriminating proof

- **CLI0-AC1 — sole-owner proof:** add `cli0_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **CLI0-AC2 — positive contract:** add `cli0_publishes_exact_applicationservices`; assert exact identities, provenance, completeness, and deterministic ordering.
- **CLI0-AC3 — incremental equivalence:** add `cli0_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **CLI0-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
- Test homes: `packages/binary-launcher/cli.spec.ts`, `crates/verter_mcp_server/tests`.

## Deletions and forbidden designs

- Delete or structurally reject: **command-local semantic engine**.
- Delete or structurally reject: **non-atomic multi-file writes**.
- Delete or structurally reject: **ambiguous offset encoding**.
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

1. `pnpm --filter @verter/binary-launcher test`
2. Run every final command in the bound `docs-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Re-run the planted RED mutation, restore, then GREEN, and bind both outputs to the candidate SHA/tree.

## Review and lower-severity findings

Apply `architecture-3`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates the gate and all three review verdicts. Final acceptance requires three independent PASS verdicts on the exact candidate tree; clean 3/3 means no P0/P1 and no unauthorized or undispositioned P2.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three fresh distinct harness reviewers with assigned lenses, deterministic low|medium|high effort, exact task/provider/model bindings, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:successor-expansion.md:L1475`

## Reconciled source-plan contract

**Intent:** freeze one executable surface and distinct command semantics before building the shell.
**Predecessors:** `PUB0`.
**Subblocks:** (1) resolve `@verter/cli` package and `verter` binary naming, including private root-package collision; (2) lock command grammar/exit codes/stdout/stderr/machine schemas; (3) distinguish `typecheck`, `tsc`, `compile`, `type-info`, service-host, formatter, and lint command families; (4) normalize compiler disposition to `Supported | FutureSeparateTrain | NotApplicable`; (5) inventory existing binaries/packages and consumers; (6) lock one-release wrapper policy, later deletion receipt, and performance/security gates.
**Acceptance:** every command maps to an existing or separately planned service owner; no placeholder/no-op command is admitted; package ownership and cutover are explicit.
**Forbidden:** one “check” semantic hiding emit/mutation, CLI-owned analyzers, indefinite aliases, or unscoped package assumptions.
**Deletion/abort:** no code; omit any command lacking a truthful engine rather than ship a placeholder.

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-EXP-L1475-F0EEA8C93A02

- Kind: `context`
- Source: `successor-expansion.md:1475-1475`
- Applicability: `CLI0`
- Exact text SHA-256: `f0eea8c93a028d7e7c32e227867675746d91882f974861eac1073aaf1b553825`

~~~~markdown
### `CLI0.md` — `verter` command/package and semantic lock
~~~~

### SRC-EXP-L1477-B9AE364E57FA

- Kind: `forbidden`
- Source: `successor-expansion.md:1477-1482`
- Applicability: `CLI0`
- Exact text SHA-256: `b9ae364e57fa2d6ab7a59a7e759c4840f059dcde523247c11bc9dd98518670fe`

~~~~markdown
**Intent:** freeze one executable surface and distinct command semantics before building the shell.
**Predecessors:** `PUB0`.
**Subblocks:** (1) resolve `@verter/cli` package and `verter` binary naming, including private root-package collision; (2) lock command grammar/exit codes/stdout/stderr/machine schemas; (3) distinguish `typecheck`, `tsc`, `compile`, `type-info`, service-host, formatter, and lint command families; (4) normalize compiler disposition to `Supported | FutureSeparateTrain | NotApplicable`; (5) inventory existing binaries/packages and consumers; (6) lock one-release wrapper policy, later deletion receipt, and performance/security gates.
**Acceptance:** every command maps to an existing or separately planned service owner; no placeholder/no-op command is admitted; package ownership and cutover are explicit.
**Forbidden:** one “check” semantic hiding emit/mutation, CLI-owned analyzers, indefinite aliases, or unscoped package assumptions.
**Deletion/abort:** no code; omit any command lacking a truthful engine rather than ship a placeholder.
~~~~
