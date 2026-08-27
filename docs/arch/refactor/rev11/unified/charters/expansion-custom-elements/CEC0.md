<!-- unified-charter-v2
id=CEC0
name=Shared legacy Web Component schema/registry cutover
phase=expansion
train=expansion.custom-elements
product=custom_elements
kind=cutover
semantic_role=delivery
class=successor
predecessors=VCE0,SCE0
conditional_predecessors=
owner=expansion.custom-elements:standards model plus framework-specific producer/consumer adapters
conflict_domains=capability_catalog
resource_class=rust-mixed
review_profile=architecture-3
gate_profile=targeted-domain
size=S
dispatchable=true
optional=false
release_gating=none
source_refs=source:successor-expansion.md:L1204
external_requirements=
activation_gate=ORC0
charter=charters/expansion-custom-elements/CEC0.md
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# CEC0 — Shared legacy Web Component schema/registry cutover

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Shared legacy Web Component schema/registry cutover. The current owner is **shared legacy Web Component schema/registry**. The final and sole owner is **standards model plus framework-specific producer/consumer adapters**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_language/src`, `crates/verter_protocol/src`, `crates/verter_session/src`.
- Named API/data boundaries: `CustomElementDeclaration`, `CustomElementRegistration`, `CemModule`, `FrameworkCeAdapter`.
- Mutation boundary: bounded validation, named residual deletion, and/or one atomic route switch only; no new authority may be introduced.

## Exact predecessor contracts

- **VCE0:** exact current receipt ID and digest for “Vue Custom Element producer and consumer retrofit”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **SCE0:** exact current receipt ID and digest for “Svelte Custom Element producer and consumer retrofit”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Revision:** 4 — supersedes the 251-charter all-verticals proposal
- **Prepared:** 2026-08-26
- **Repository basis:** program/architecture-lock at d1f3d50a948597f036868543b9bb21acacd730ff
- **Current-program condition:** maintainer work freeze; TCM0 = RESCOPE_REQUIRED; TCM1–TCM4 = LOCKED

## Acceptance IDs and discriminating proof

- **CEC0-AC1 — sole-owner proof:** add `cec0_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **CEC0-AC2 — positive contract:** add `cec0_publishes_exact_customelementdeclaration`; assert exact identities, provenance, completeness, and deterministic ordering.
- **CEC0-AC3 — incremental equivalence:** add `cec0_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **CEC0-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
- Test homes: `crates/verter_language/tests`, `crates/verter_protocol/tests`.

## Deletions and forbidden designs

- Delete or structurally reject: **legacy shared CE schema**.
- Delete or structurally reject: **unqualified global registry**.
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

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three named independent reviewers with assigned lenses, model `gpt-5.6-sol`, effort `ultra`, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:successor-expansion.md:L1204`

## Reconciled source-plan contract

**Intent:** remove the displaced shared WCP authority only after neutral, Vue, and Svelte consumers have migrated to the standards owner.
**Predecessors:** `VCE0`, `SCE0`.
**Subblocks:** (1) consume the exact `UAK0` deletion-unit/consumer ledger; (2) verify neutral standards rows use `HWC3`; (3) verify Vue rows use `VCE0`; (4) verify Svelte rows use `SCE0`; (5) search native/generated/public consumers and serialized schema references; (6) atomically delete the old shared schema/registry and run landing-equivalence tests.
**Acceptance:** zero callers/generated references remain; all supported consumers resolve through the `CEF0` contract, HWC3-produced standards facts, and vertical-owned evidence; deletion lands on the exact reviewed tree.
**Forbidden:** semantic implementation, fixing profile behavior, deleting before zero-consumer proof, or keeping a compatibility schema as a second authority.
**Deletion/abort:** this is the sole owner of shared WCP schema/registry deletion; any remaining consumer returns to its profile migration and aborts cutover.

## Collapsed non-authoritative subblock disposition

The recovery candidate mechanically split this source-owned atomic node into the following labels: `CEC0-A`, `CEC0-B`, `CEC0-C`, `CEC0-D`, `CEC0-E`, `CEC0-F`. They have no separate dispatch, lease, receipt, migration manifest, deletion ownership, or review standing. Their useful source-described concerns are internal RED/GREEN checklist items of **CEC0**; CEC0 alone owns the complete migration population, exactly one final deletion/cutover, and atomic acceptance. Any quoted “suggested subblock” wording in transferred source text is non-authoritative planning context.

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-EXP-L1204-B83BC7952C0E

- Kind: `context`
- Source: `successor-expansion.md:1204-1204`
- Applicability: `CEC0`
- Exact text SHA-256: `b83bc7952c0e9b93903d91125b3ec9ea3615ec7952aa2827494741f2bf75105c`

~~~~markdown
### `CEC0.md` — Shared legacy Web Component schema/registry cutover
~~~~

### SRC-EXP-L1206-17AD618EB257

- Kind: `forbidden`
- Source: `successor-expansion.md:1206-1211`
- Applicability: `CEC0`
- Exact text SHA-256: `17ad618eb257d840d2c325619b1fa1b680f84e964e662f743e629d5a63037700`

~~~~markdown
**Intent:** remove the displaced shared WCP authority only after neutral, Vue, and Svelte consumers have migrated to the standards owner.
**Predecessors:** `VCE0`, `SCE0`.
**Subblocks:** (1) consume the exact `UAK0` deletion-unit/consumer ledger; (2) verify neutral standards rows use `HWC3`; (3) verify Vue rows use `VCE0`; (4) verify Svelte rows use `SCE0`; (5) search native/generated/public consumers and serialized schema references; (6) atomically delete the old shared schema/registry and run landing-equivalence tests.
**Acceptance:** zero callers/generated references remain; all supported consumers resolve through the `CEF0` contract, HWC3-produced standards facts, and vertical-owned evidence; deletion lands on the exact reviewed tree.
**Forbidden:** semantic implementation, fixing profile behavior, deleting before zero-consumer proof, or keeping a compatibility schema as a second authority.
**Deletion/abort:** this is the sole owner of shared WCP schema/registry deletion; any remaining consumer returns to its profile migration and aborts cutover.
~~~~
