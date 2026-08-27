<!-- unified-charter-v2
id=EAK1
name=Vue `defineComponent` embedded-template canary
phase=expansion
train=expansion.kernel
product=kernel
kind=canary
semantic_role=delivery
class=successor
predecessors=EMB0,TIF0
conditional_predecessors=
owner=expansion.kernel:typed immutable universal catalog and demand-selected kernel services
conflict_domains=carrierprofileid,vue_product
resource_class=rust-mixed
review_profile=semantic-3
gate_profile=targeted-domain
size=M
dispatchable=true
optional=false
release_gating=none
source_refs=source:successor-expansion.md:L1004
external_requirements=
activation_gate=ORC0
charter=charters/expansion-kernel/EAK1.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# EAK1 — Vue `defineComponent` embedded-template canary

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Vue `defineComponent` embedded-template canary. The current owner is **framework-shaped host/session registries and untagged public boundaries**. The final and sole owner is **typed immutable universal catalog and demand-selected kernel services**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_language/src`, `crates/verter_session/src`, `crates/verter_protocol/src`, `crates/verter_identity/src`.
- Named API/data boundaries: `CarrierProfileId`, `FrameworkProfileId`, `ProjectProfileId`, `CatalogSnapshot`, `DemandPlan`, `TypeInfoRequest`.
- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **EMB0:** exact current receipt ID and digest for “Embedded codecs and exact authored map chains”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **TIF0:** exact current receipt ID and digest for “TypeInfo query/selector and authority-composition contract”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Normative intent:** prove the hardest reusable embedding seam against a real current framework without routing ordinary TS through the mapper.
- **Atomic boundary:** the production surfaces and named API/data boundaries above form this source-owned node's exclusive acceptance subset; this node owns its complete named migration population and exactly one deletion/cutover disposition.
- **Cutover evidence:** prove removal or structural rejection of **central framework switch**, **untagged coordinate/public identity**, **duplicate component information authority** and satisfy the node-specific acceptance IDs below. A newly independent outcome requires an amendment and a new node before mutation.

## Acceptance IDs and discriminating proof

- **EAK1-AC1 — sole-owner proof:** add `eak1_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **EAK1-AC2 — positive contract:** add `eak1_publishes_exact_carrierprofileid`; assert exact identities, provenance, completeness, and deterministic ordering.
- **EAK1-AC3 — incremental equivalence:** add `eak1_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **EAK1-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
- Test homes: `crates/verter_language/tests`, `crates/verter_session/tests/cases`, `crates/verter_protocol/tests`.

## Deletions and forbidden designs

- Delete or structurally reject: **central framework switch**.
- Delete or structurally reject: **untagged coordinate/public identity**.
- Delete or structurally reject: **duplicate component information authority**.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 800 production LOC, 8 production files, 2 related crates/packages.
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

Apply `semantic-3`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates the gate and all three review verdicts. Final acceptance requires three independent PASS verdicts on the exact candidate tree; clean 3/3 means no P0/P1 and no unauthorized or undispositioned P2.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three named independent reviewers with assigned lenses, model `gpt-5.6-sol`, effort `ultra`, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:successor-expansion.md:L1004`

## Reconciled source-plan contract

**Intent:** prove the hardest reusable embedding seam against a real current framework without routing ordinary TS through the mapper.
**Predecessors:** `EMB0`, `TIF0`.
**Subblocks:** (1) exact Vue release/oracle lock; (2) source activation for direct/aliased/barrel/namespace/destructured/immutable alias paths; (3) object `template` extraction and codec maps; (4) Vue template parse/facts/scopes; (5) post-snapshot TypeInfo plus private-harness hover/completion/definition/diagnostic/safe-fix and authored-map feasibility; (6) negative/dynamic/stale/performance tests.
**Acceptance:** the user’s alias example has exact private-harness IDE behavior; userland/mutated/ambiguous cases remain plain TS; no mapper callback performs an oracle query; no public formatter/CLI authority is created; non-invertible literals report partiality.
**Forbidden:** name matching, whole-file virtual TSX, a second TS program, post-snapshot mutation of the current transform, or framework logic in `EmbeddedTextCodec`.
**Deletion/abort:** remove any superseded Vue bespoke literal path; abort if exact provenance or authored mapping cannot be proven.

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-EXP-L1004-C45FC368A174

- Kind: `context`
- Source: `successor-expansion.md:1004-1004`
- Applicability: `EAK1`
- Exact text SHA-256: `c45fc368a174982776060e00bd76b16d2239df1712a569a54646f1787855054d`

~~~~markdown
### `EAK1.md` — Vue `defineComponent` embedded-template canary
~~~~

### SRC-EXP-L1006-28E3D074265D

- Kind: `forbidden`
- Source: `successor-expansion.md:1006-1011`
- Applicability: `EAK1`
- Exact text SHA-256: `28e3d074265d52eaa84d7b63bf5fb3736e35c97cd140dfd4e8bf7804ca29fea6`

~~~~markdown
**Intent:** prove the hardest reusable embedding seam against a real current framework without routing ordinary TS through the mapper.
**Predecessors:** `EMB0`, `TIF0`.
**Subblocks:** (1) exact Vue release/oracle lock; (2) source activation for direct/aliased/barrel/namespace/destructured/immutable alias paths; (3) object `template` extraction and codec maps; (4) Vue template parse/facts/scopes; (5) post-snapshot TypeInfo plus private-harness hover/completion/definition/diagnostic/safe-fix and authored-map feasibility; (6) negative/dynamic/stale/performance tests.
**Acceptance:** the user’s alias example has exact private-harness IDE behavior; userland/mutated/ambiguous cases remain plain TS; no mapper callback performs an oracle query; no public formatter/CLI authority is created; non-invertible literals report partiality.
**Forbidden:** name matching, whole-file virtual TSX, a second TS program, post-snapshot mutation of the current transform, or framework logic in `EmbeddedTextCodec`.
**Deletion/abort:** remove any superseded Vue bespoke literal path; abort if exact provenance or authored mapping cannot be proven.
~~~~
