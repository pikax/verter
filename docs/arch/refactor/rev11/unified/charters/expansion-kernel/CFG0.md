<!-- unified-charter-v2
id=CFG0
name=Declarative Verter and captured ecosystem configuration
phase=expansion
train=expansion.kernel
product=kernel
kind=contract
semantic_role=delivery
class=successor
predecessors=CAT0
conditional_predecessors=
owner=expansion.kernel:typed immutable universal catalog and demand-selected kernel services
conflict_domains=carrierprofileid
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
source_refs=source:successor-expansion.md:L941
external_requirements=
activation_gate=ORC0
charter=charters/expansion-kernel/CFG0.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# CFG0 — Declarative Verter and captured ecosystem configuration

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Declarative Verter and captured ecosystem configuration. The current owner is **framework-shaped host/session registries and untagged public boundaries**. The final and sole owner is **typed immutable universal catalog and demand-selected kernel services**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_language/src`, `crates/verter_session/src`, `crates/verter_protocol/src`, `crates/verter_identity/src`.
- Named API/data boundaries: `CarrierProfileId`, `FrameworkProfileId`, `ProjectProfileId`, `CatalogSnapshot`, `DemandPlan`, `TypeInfoRequest`.
- Mutation boundary: authority/evidence bytes only; production LOC is zero.

## Exact predecessor contracts

- **CAT0:** exact current receipt ID and digest for “Immutable typed catalog snapshot and static registration”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Normative intent:** establish the hermetic base configuration/read-set authority without depending on downstream lint-rule or formatter-option schemas.
- **Atomic boundary:** the production surfaces and named API/data boundaries above form this source-owned node's exclusive acceptance subset; this node owns its complete named migration population and exactly one deletion/cutover disposition.
- **Cutover evidence:** prove removal or structural rejection of **central framework switch**, **untagged coordinate/public identity**, **duplicate component information authority** and satisfy the node-specific acceptance IDs below. A newly independent outcome requires an amendment and a new node before mutation.

## Acceptance IDs and discriminating proof

- **CFG0-AC1 — sole-owner proof:** add `cfg0_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **CFG0-AC2 — positive contract:** add `cfg0_publishes_exact_carrierprofileid`; assert exact identities, provenance, completeness, and deterministic ordering.
- **CFG0-AC3 — incremental equivalence:** add `cfg0_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **CFG0-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
- Test homes: `crates/verter_language/tests`, `crates/verter_session/tests/cases`, `crates/verter_protocol/tests`.

## Deletions and forbidden designs

- Delete or structurally reject: **central framework switch**.
- Delete or structurally reject: **untagged coordinate/public identity**.
- Delete or structurally reject: **duplicate component information authority**.
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

1. `cargo nextest run -p verter_language -p verter_protocol -p verter_session`
2. Run every final command in the bound `docs-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Re-run the planted RED mutation, restore, then GREEN, and bind both outputs to the candidate SHA/tree.

## Review and lower-severity findings

Apply `architecture-3`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates the gate and all three review verdicts. Final acceptance requires three independent PASS verdicts on the exact candidate tree; clean 3/3 means no P0/P1 and no unauthorized or undispositioned P2.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three fresh distinct harness reviewers with assigned lenses, deterministic low|medium|high effort, exact task/provider/model bindings, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:successor-expansion.md:L941`

## Reconciled source-plan contract

**Intent:** establish the hermetic base configuration/read-set authority without depending on downstream lint-rule or formatter-option schemas.
**Predecessors:** `CAT0`.
**Subblocks:** (1) versioned `verter.config.jsonc` envelope; (2) root/extends/override/profile precedence and provenance; (3) typed opaque product-config sections whose schemas remain downstream-owned; (4) unknown top-level/cycle/trust/NeedInputs outcomes; (5) config read sets and invalidation; (6) NAPI/WASM prepared-input contracts.
**Acceptance:** precedence is deterministic across monorepo/nested configs; unknown framework release and top-level fields fail closed; product payloads retain exact source/provenance for later translators; changing irrelevant config does not invalidate unrelated profiles.
**Forbidden:** arbitrary JS execution in core, ambient home/global config, one flat framework section, silent option dropping, or conflating config translation with external tool execution.
**Deletion/abort:** migrate only base/profile readers; product readers are deleted by their downstream translator cutovers; rescope executable ecosystem configuration behind the separately trusted host boundary.

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-EXP-L941-B5B0A0D1C8C8

- Kind: `context`
- Source: `successor-expansion.md:941-941`
- Applicability: `CFG0`
- Exact text SHA-256: `b5b0a0d1c8c800105458c14bc4e481778319bedbe3db7480e0d422ff602a5e7f`

~~~~markdown
### `CFG0.md` — Declarative Verter and captured ecosystem configuration
~~~~

### SRC-EXP-L943-7B6DAF3DA386

- Kind: `forbidden`
- Source: `successor-expansion.md:943-948`
- Applicability: `CFG0`
- Exact text SHA-256: `7b6daf3da3869fb80859a6a847b32a87d95b4f8a54cd9055e07310c4b759d1a4`

~~~~markdown
**Intent:** establish the hermetic base configuration/read-set authority without depending on downstream lint-rule or formatter-option schemas.
**Predecessors:** `CAT0`.
**Subblocks:** (1) versioned `verter.config.jsonc` envelope; (2) root/extends/override/profile precedence and provenance; (3) typed opaque product-config sections whose schemas remain downstream-owned; (4) unknown top-level/cycle/trust/NeedInputs outcomes; (5) config read sets and invalidation; (6) NAPI/WASM prepared-input contracts.
**Acceptance:** precedence is deterministic across monorepo/nested configs; unknown framework release and top-level fields fail closed; product payloads retain exact source/provenance for later translators; changing irrelevant config does not invalidate unrelated profiles.
**Forbidden:** arbitrary JS execution in core, ambient home/global config, one flat framework section, silent option dropping, or conflating config translation with external tool execution.
**Deletion/abort:** migrate only base/profile readers; product readers are deleted by their downstream translator cutovers; rescope executable ecosystem configuration behind the separately trusted host boundary.
~~~~
