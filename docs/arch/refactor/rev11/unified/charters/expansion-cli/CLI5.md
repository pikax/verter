<!-- unified-charter-v2
id=CLI5
name=Base packaging, watch mode, compatibility wrappers, and promotion
phase=expansion
train=expansion.cli
product=cli
kind=terminal
semantic_role=delivery
class=successor
predecessors=CLI2,CLITS0,CLIC0,CLI4,PER0
conditional_predecessors=
owner=expansion.cli:one `verter` application service with thin command adapters
conflict_domains=cli_application,program_authority
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
release_gating=product
source_refs=source:successor-expansion.md:L1529
external_requirements=
activation_gate=ORC0
charter=charters/expansion-cli/CLI5.md
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# CLI5 — Base packaging, watch mode, compatibility wrappers, and promotion

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Base packaging, watch mode, compatibility wrappers, and promotion. The current owner is **separate package launchers and command-local project logic**. The final and sole owner is **one `verter` application service with thin command adapters**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `packages/binary-launcher`, `packages/verter-lsp`, `packages/verter-tsc`, `crates/verter_mcp_server/src`.
- Named API/data boundaries: `ApplicationServices`, `SelectionPlan`, `Reporter`, `WriteTransaction`, `CommandCapability`.
- Mutation boundary: bounded validation, named residual deletion, and/or one atomic route switch only; no new authority may be introduced.

## Exact predecessor contracts

- **CLI2:** exact current receipt ID and digest for “Verter-native `typecheck` command”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **CLITS0:** exact current receipt ID and digest for “TypeScript-compatible `tsc` command”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **CLIC0:** exact current receipt ID and digest for “Registered carrier `compile` command”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **CLI4:** exact current receipt ID and digest for “`type-info`, `lsp`, and `mcp` command adapters”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **PER0:** exact current receipt ID and digest for “Cache/backend identity, cancellation, budgets, and zero work”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Normative intent:** package and independently promote the base executable without waiting for formatter, lint, or future verticals.
- **Atomic boundary:** the production surfaces and named API/data boundaries above form this source-owned node's exclusive acceptance subset; this node owns its complete named migration population and exactly one deletion/cutover disposition.
- **Cutover evidence:** prove removal or structural rejection of **command-local semantic engine**, **non-atomic multi-file writes**, **ambiguous offset encoding** and satisfy the node-specific acceptance IDs below. A newly independent outcome requires an amendment and a new node before mutation.

## Acceptance IDs and discriminating proof

- **CLI5-AC1 — sole-owner proof:** add `cli5_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **CLI5-AC2 — positive contract:** add `cli5_publishes_exact_applicationservices`; assert exact identities, provenance, completeness, and deterministic ordering.
- **CLI5-AC3 — incremental equivalence:** add `cli5_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **CLI5-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
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

- `source:successor-expansion.md:L1529`

## Reconciled source-plan contract

**Intent:** package and independently promote the base executable without waiting for formatter, lint, or future verticals.
**Predecessors:** `CLI2`, `CLITS0`, `CLIC0`, `CLI4`, `PER0`.
**Subblocks:** (1) native platform package matrix and integrity/provenance; (2) npm `@verter/cli` install/dispatch; (3) bounded watch/incremental session reuse; (4) convert named old binaries to thin wrappers over the same executable/service registry; (5) retain wrappers for one explicitly named published release with telemetry/deprecation receipt; (6) cold/warm/RSS/cancellation/signal/CI tests, generated command matrix, docs, and exact-candidate reviews.
**Acceptance:** clean installs work on every locked platform; commands advertise only available services; watch equals repeated fresh results and plateaus memory; wrappers execute the same implementation; base CLI promotes independently of fmt/lint/Astro/Qwik/project profiles.
**Forbidden:** downloading unverified binaries, separate alias implementations, hidden daemon state, or withholding CLI release for incomplete Astro/Qwik/project profiles.
**Deletion/abort:** do not delete compatibility wrappers here; a later charter may delete them only after the named published-release receipt and zero-consumer/generated-reference proof. A failing platform remains explicitly unsupported rather than receiving an unverified fallback.

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-EXP-L1529-3E7B56FCB3D0

- Kind: `context`
- Source: `successor-expansion.md:1529-1529`
- Applicability: `CLI5`
- Exact text SHA-256: `3e7b56fcb3d0578173273fc016e95dd77ff9944857639063765e4c70f4da3fb7`

~~~~markdown
### `CLI5.md` — Base packaging, watch mode, compatibility wrappers, and promotion
~~~~

### SRC-EXP-L1531-32D48006AA6D

- Kind: `forbidden`
- Source: `successor-expansion.md:1531-1536`
- Applicability: `CLI5`
- Exact text SHA-256: `32d48006aa6d47838b5849eee1692825a88616525d33543027e69bf6af736160`

~~~~markdown
**Intent:** package and independently promote the base executable without waiting for formatter, lint, or future verticals.
**Predecessors:** `CLI2`, `CLITS0`, `CLIC0`, `CLI4`, `PER0`.
**Subblocks:** (1) native platform package matrix and integrity/provenance; (2) npm `@verter/cli` install/dispatch; (3) bounded watch/incremental session reuse; (4) convert named old binaries to thin wrappers over the same executable/service registry; (5) retain wrappers for one explicitly named published release with telemetry/deprecation receipt; (6) cold/warm/RSS/cancellation/signal/CI tests, generated command matrix, docs, and exact-candidate reviews.
**Acceptance:** clean installs work on every locked platform; commands advertise only available services; watch equals repeated fresh results and plateaus memory; wrappers execute the same implementation; base CLI promotes independently of fmt/lint/Astro/Qwik/project profiles.
**Forbidden:** downloading unverified binaries, separate alias implementations, hidden daemon state, or withholding CLI release for incomplete Astro/Qwik/project profiles.
**Deletion/abort:** do not delete compatibility wrappers here; a later charter may delete them only after the named published-release receipt and zero-consumer/generated-reference proof. A failing platform remains explicitly unsupported rather than receiving an unverified fallback.
~~~~
