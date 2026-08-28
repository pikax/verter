<!-- unified-charter-v2
id=ENCL0
name=LSP and editor coordinate-boundary cutover
phase=expansion
train=expansion.kernel
product=kernel
kind=cutover
semantic_role=delivery
class=successor
predecessors=ENC0
conditional_predecessors=
owner=expansion.kernel:typed immutable universal catalog and demand-selected kernel services
conflict_domains=mapping_geometry,lsp_publication
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
release_gating=none
source_refs=source:successor-expansion.md:L833
external_requirements=
activation_gate=ORC0
charter=charters/expansion-kernel/ENCL0.md
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# ENCL0 — LSP and editor coordinate-boundary cutover

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

LSP and editor coordinate-boundary cutover. The current owner is **framework-shaped host/session registries and untagged public boundaries**. The final and sole owner is **typed immutable universal catalog and demand-selected kernel services**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_language/src`, `crates/verter_session/src`, `crates/verter_protocol/src`, `crates/verter_identity/src`.
- Named API/data boundaries: `CarrierProfileId`, `FrameworkProfileId`, `ProjectProfileId`, `CatalogSnapshot`, `DemandPlan`, `TypeInfoRequest`.
- Mutation boundary: bounded validation, named residual deletion, and/or one atomic route switch only; no new authority may be introduced.

## Exact predecessor contracts

- **ENC0:** exact current receipt ID and digest for “UTF-8 internal coordinate constitution and audit”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Revision:** 4 — supersedes the 251-charter all-verticals proposal
- **Prepared:** 2026-08-26
- **Repository basis:** program/architecture-lock at d1f3d50a948597f036868543b9bb21acacd730ff
- **Current-program condition:** maintainer work freeze; TCM0 = RESCOPE_REQUIRED; TCM1–TCM4 = LOCKED

## Acceptance IDs and discriminating proof

- **ENCL0-AC1 — sole-owner proof:** add `encl0_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **ENCL0-AC2 — positive contract:** add `encl0_publishes_exact_carrierprofileid`; assert exact identities, provenance, completeness, and deterministic ordering.
- **ENCL0-AC3 — incremental equivalence:** add `encl0_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **ENCL0-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
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

- `source:successor-expansion.md:L833`

## Reconciled source-plan contract

**Intent:** make the editor boundary negotiate and convert coordinates exactly once while Rust core remains UTF-8-byte-only.
**Predecessors:** `ENC0`.
**Subblocks:** (1) LSP position-encoding handshake and capability truth; (2) ingress validation and UTF-16/UTF-32→UTF-8 conversion; (3) egress range/edit/location conversion; (4) line-index lifetime and incremental update rules; (5) astral/combining/ZWJ/CRLF/overflow property corpus; (6) UTF-8 fast-path allocation and latency benchmarks.
**Acceptance:** every admitted LSP encoding round-trips exactly; invalid boundaries are typed failures; UTF-8 requests allocate no conversion buffer; editor encoding never enters semantic/cache identity.
**Forbidden:** conversion inside parsers/resolvers/indexes, implicit UTF-16 defaults, saturation, or cached requester-encoding mirrors.
**Deletion/abort:** delete fixed-UTF-16 editor contracts only after all callers migrate; abort on an untagged editor range.

## Collapsed non-authoritative subblock disposition

The recovery candidate mechanically split this source-owned atomic node into the following labels: `ENCL0-A`, `ENCL0-B`, `ENCL0-C`, `ENCL0-D`, `ENCL0-E`, `ENCL0-F`. They have no separate dispatch, lease, receipt, migration manifest, deletion ownership, or review standing. Their useful source-described concerns are internal RED/GREEN checklist items of **ENCL0**; ENCL0 alone owns the complete migration population, exactly one final deletion/cutover, and atomic acceptance. Any quoted “suggested subblock” wording in transferred source text is non-authoritative planning context.

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-EXP-L833-57F6CB36508B

- Kind: `context`
- Source: `successor-expansion.md:833-833`
- Applicability: `ENCL0`
- Exact text SHA-256: `57f6cb36508b5f64d3361fa2cfc7177317d79e4c6582b3c8b488ab01374e4480`

~~~~markdown
### `ENCL0.md` — LSP and editor coordinate-boundary cutover
~~~~

### SRC-EXP-L835-10E360311C14

- Kind: `forbidden`
- Source: `successor-expansion.md:835-840`
- Applicability: `ENCL0`
- Exact text SHA-256: `10e360311c149d6dec33077662260accb8dd6b2310af33f536e47b1832913505`

~~~~markdown
**Intent:** make the editor boundary negotiate and convert coordinates exactly once while Rust core remains UTF-8-byte-only.
**Predecessors:** `ENC0`.
**Subblocks:** (1) LSP position-encoding handshake and capability truth; (2) ingress validation and UTF-16/UTF-32→UTF-8 conversion; (3) egress range/edit/location conversion; (4) line-index lifetime and incremental update rules; (5) astral/combining/ZWJ/CRLF/overflow property corpus; (6) UTF-8 fast-path allocation and latency benchmarks.
**Acceptance:** every admitted LSP encoding round-trips exactly; invalid boundaries are typed failures; UTF-8 requests allocate no conversion buffer; editor encoding never enters semantic/cache identity.
**Forbidden:** conversion inside parsers/resolvers/indexes, implicit UTF-16 defaults, saturation, or cached requester-encoding mirrors.
**Deletion/abort:** delete fixed-UTF-16 editor contracts only after all callers migrate; abort on an untagged editor range.
~~~~
