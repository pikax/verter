<!-- unified-charter-v2
id=LRA0
name=Profile-scoped diagnostics, lint, fixes, and actions
phase=expansion
train=expansion.kernel
product=kernel
kind=contract
semantic_role=delivery
class=successor
predecessors=CFG0,TIF1,IDX0
conditional_predecessors=
owner=expansion.kernel:typed immutable universal catalog and demand-selected kernel services
conflict_domains=diagnostic_action_service
resource_class=docs-light
review_profile=architecture-3
gate_profile=docs-domain
size=M
dispatchable=true
optional=false
release_gating=none
source_refs=source:successor-expansion.md:L950
external_requirements=
activation_gate=ORC0
charter=charters/expansion-kernel/LRA0.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# LRA0 — Profile-scoped diagnostics, lint, fixes, and actions

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Profile-scoped diagnostics, lint, fixes, and actions. The current owner is **framework-shaped host/session registries and untagged public boundaries**. The final and sole owner is **typed immutable universal catalog and demand-selected kernel services**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_language/src`, `crates/verter_session/src`, `crates/verter_protocol/src`, `crates/verter_identity/src`.
- Named API/data boundaries: `CarrierProfileId`, `FrameworkProfileId`, `ProjectProfileId`, `CatalogSnapshot`, `DemandPlan`, `TypeInfoRequest`.
- Mutation boundary: authority/evidence bytes only; production LOC is zero.

## Exact predecessor contracts

- **CFG0:** exact current receipt ID and digest for “Declarative Verter and captured ecosystem configuration”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **TIF1:** exact current receipt ID and digest for “TypeInfo-first ComponentInfo and component-meta cutover”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **IDX0:** exact current receipt ID and digest for “Atomic semantic contributions and workspace index”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Normative intent:** lock rule/action registration and safety without prematurely implementing every ecosystem rule.
- **Atomic boundary:** the production surfaces and named API/data boundaries above form this source-owned node's exclusive acceptance subset; this node owns its complete named migration population and exactly one deletion/cutover disposition.
- **Cutover evidence:** prove removal or structural rejection of **central framework switch**, **untagged coordinate/public identity**, **duplicate component information authority** and satisfy the node-specific acceptance IDs below. A newly independent outcome requires an amendment and a new node before mutation.

## Acceptance IDs and discriminating proof

- **LRA0-AC1 — sole-owner proof:** add `lra0_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **LRA0-AC2 — positive contract:** add `lra0_publishes_exact_carrierprofileid`; assert exact identities, provenance, completeness, and deterministic ordering.
- **LRA0-AC3 — incremental equivalence:** add `lra0_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **LRA0-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
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

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three named independent reviewers with assigned lenses, model `gpt-5.6-sol`, effort `ultra`, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:successor-expansion.md:L950`

## Reconciled source-plan contract

**Intent:** lock rule/action registration and safety without prematurely implementing every ecosystem rule.
**Predecessors:** `CFG0`, `TIF1`, `IDX0`.
**Subblocks:** (1) rule/action manifest keyed by exact vertical release; (2) fact-demand and applicability contracts; (3) diagnostic identity/suppression/provenance; (4) safe/suggested/unsafe edit classes and authored transaction basis; (5) common-neutral versus vertical-owned rule separation; (6) migrate representative Vue/Svelte rules and fixes.
**Acceptance:** inapplicable rules perform zero work; two profiles may use different rule epochs without collision; stale or conflicting edits are rejected; migrated rule diagnostics/fixes remain equivalent on pinned fixtures.
**Forbidden:** Vue-shaped global rule table, format-as-fix, executing third-party rule code, duplicate native/external diagnostics, or actions without exact source/map basis.
**Deletion/abort:** delete only the named representative rows/adapters migrated here; profile rows belong to their packs and shared registry deletion belongs solely to `LNT3`; abort if a “common” rule requires framework branching instead of a neutral fact contract.

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-EXP-L950-737741E0B762

- Kind: `context`
- Source: `successor-expansion.md:950-950`
- Applicability: `LRA0`
- Exact text SHA-256: `737741e0b76239446c48a720239d65ee14568f9625c06d070cda9aa6c09bbdf3`

~~~~markdown
### `LRA0.md` — Profile-scoped diagnostics, lint, fixes, and actions
~~~~

### SRC-EXP-L952-905C4A67A74B

- Kind: `forbidden`
- Source: `successor-expansion.md:952-957`
- Applicability: `LRA0`
- Exact text SHA-256: `905c4a67a74ba946413ea373036731c150065c484773f6bebb2e463834213a58`

~~~~markdown
**Intent:** lock rule/action registration and safety without prematurely implementing every ecosystem rule.
**Predecessors:** `CFG0`, `TIF1`, `IDX0`.
**Subblocks:** (1) rule/action manifest keyed by exact vertical release; (2) fact-demand and applicability contracts; (3) diagnostic identity/suppression/provenance; (4) safe/suggested/unsafe edit classes and authored transaction basis; (5) common-neutral versus vertical-owned rule separation; (6) migrate representative Vue/Svelte rules and fixes.
**Acceptance:** inapplicable rules perform zero work; two profiles may use different rule epochs without collision; stale or conflicting edits are rejected; migrated rule diagnostics/fixes remain equivalent on pinned fixtures.
**Forbidden:** Vue-shaped global rule table, format-as-fix, executing third-party rule code, duplicate native/external diagnostics, or actions without exact source/map basis.
**Deletion/abort:** delete only the named representative rows/adapters migrated here; profile rows belong to their packs and shared registry deletion belongs solely to `LNT3`; abort if a “common” rule requires framework branching instead of a neutral fact contract.
~~~~
