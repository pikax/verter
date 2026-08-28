<!-- unified-charter-v2
id=UAI0
name=Identity, carrier, parser, and coordinate contract lock
phase=expansion
train=expansion.kernel
product=kernel
kind=convergence
semantic_role=convergence
class=successor
predecessors=VID0,CAT0,CPF1,PAR0,ENC1
conditional_predecessors=
owner=expansion.kernel:typed immutable universal catalog and demand-selected kernel services
conflict_domains=source_lineage,mapping_geometry,carrier_parser
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
release_gating=contract
source_refs=source:successor-expansion.md:L1013
external_requirements=
activation_gate=ORC0
charter=charters/expansion-kernel/UAI0.md
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# UAI0 — Identity, carrier, parser, and coordinate contract lock

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Identity, carrier, parser, and coordinate contract lock. The current owner is **framework-shaped host/session registries and untagged public boundaries**. The final and sole owner is **typed immutable universal catalog and demand-selected kernel services**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_language/src`, `crates/verter_session/src`, `crates/verter_protocol/src`, `crates/verter_identity/src`.
- Named API/data boundaries: `CarrierProfileId`, `FrameworkProfileId`, `ProjectProfileId`, `CatalogSnapshot`, `DemandPlan`, `TypeInfoRequest`.
- Mutation boundary: bounded validation, named residual deletion, and/or one atomic route switch only; no new authority may be introduced.

## Exact predecessor contracts

- **VID0:** exact current receipt ID and digest for “Orthogonal identities and exact-release law”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **CAT0:** exact current receipt ID and digest for “Immutable typed catalog snapshot and static registration”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **CPF1:** exact current receipt ID and digest for “Carrier frontend registration and Vue/Svelte cutover”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **PAR0:** exact current receipt ID and digest for “Parser decision, ownership, reuse, and lineage contract”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **ENC1:** exact current receipt ID and digest for “Tagged boundary conversion convergence”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Normative intent:** independently ratify the identity/parser side of the kernel without waiting for TypeInfo, lint, formatter, public products, or manifests.
- **Atomic boundary:** the production surfaces and named API/data boundaries above form this source-owned node's exclusive acceptance subset; this node owns its complete named migration population and exactly one deletion/cutover disposition.
- **Cutover evidence:** prove removal or structural rejection of **central framework switch**, **untagged coordinate/public identity**, **duplicate component information authority** and satisfy the node-specific acceptance IDs below. A newly independent outcome requires an amendment and a new node before mutation.

## Acceptance IDs and discriminating proof

- **UAI0-AC1 — sole-owner proof:** add `uai0_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **UAI0-AC2 — positive contract:** add `uai0_publishes_exact_carrierprofileid`; assert exact identities, provenance, completeness, and deterministic ordering.
- **UAI0-AC3 — incremental equivalence:** add `uai0_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **UAI0-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
- Test homes: `crates/verter_language/tests`, `crates/verter_session/tests/cases`, `crates/verter_protocol/tests`.

## Deletions and forbidden designs

- Delete or structurally reject: **central framework switch**.
- Delete or structurally reject: **untagged coordinate/public identity**.
- Delete or structurally reject: **duplicate component information authority**.
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

1. `cargo nextest run -p verter_language -p verter_protocol -p verter_session`
2. Run every final command in the bound `targeted-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Re-run the planted RED mutation, restore, then GREEN, and bind both outputs to the candidate SHA/tree.

## Review and lower-severity findings

Apply `architecture-3`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates the gate and all three review verdicts. Final acceptance requires three independent PASS verdicts on the exact candidate tree; clean 3/3 means no P0/P1 and no unauthorized or undispositioned P2.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three fresh distinct harness reviewers with assigned lenses, deterministic low|medium|high effort, exact task/provider/model bindings, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:successor-expansion.md:L1013`

## Reconciled source-plan contract

**Intent:** independently ratify the identity/parser side of the kernel without waiting for TypeInfo, lint, formatter, public products, or manifests.
**Predecessors:** `VID0`, `CAT0`, `CPF1`, `PAR0`, `ENC1`.
**Subblocks:** (1) compare exact owner and predecessor ledgers; (2) revalidate stable source/carrier/profile/release identities; (3) revalidate frontend/backend separation; (4) run parser ownership/reuse/lineage negatives; (5) run Unicode/boundary/map conformance; (6) independent exact-candidate review.
**Acceptance:** one resolved parser owner exists per exact grammar contract; no compiler requirement, omni parser, conflated identity, implicit encoding, or duplicate coordinate owner remains.
**Forbidden:** implementation fixes inside the lock, waiting for product engines, or claiming universal semantics.
**Deletion/abort:** delete nothing; findings return to the exact identity/carrier/parser/encoding owner and invalidate this receipt.

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-EXP-L1013-96FD2E79E0DE

- Kind: `context`
- Source: `successor-expansion.md:1013-1013`
- Applicability: `UAI0`
- Exact text SHA-256: `96fd2e79e0dec9db0779ac5100e756970797d767972dc22e3e5c2adc0ba4129c`

~~~~markdown
### `UAI0.md` — Identity, carrier, parser, and coordinate contract lock
~~~~

### SRC-EXP-L1015-701A012DD5F3

- Kind: `forbidden`
- Source: `successor-expansion.md:1015-1020`
- Applicability: `UAI0`
- Exact text SHA-256: `701a012dd5f3e44f2fdca81b6e363d553aa05c8e694e49d4c01972f131bc07aa`

~~~~markdown
**Intent:** independently ratify the identity/parser side of the kernel without waiting for TypeInfo, lint, formatter, public products, or manifests.
**Predecessors:** `VID0`, `CAT0`, `CPF1`, `PAR0`, `ENC1`.
**Subblocks:** (1) compare exact owner and predecessor ledgers; (2) revalidate stable source/carrier/profile/release identities; (3) revalidate frontend/backend separation; (4) run parser ownership/reuse/lineage negatives; (5) run Unicode/boundary/map conformance; (6) independent exact-candidate review.
**Acceptance:** one resolved parser owner exists per exact grammar contract; no compiler requirement, omni parser, conflated identity, implicit encoding, or duplicate coordinate owner remains.
**Forbidden:** implementation fixes inside the lock, waiting for product engines, or claiming universal semantics.
**Deletion/abort:** delete nothing; findings return to the exact identity/carrier/parser/encoding owner and invalidate this receipt.
~~~~
