<!-- unified-charter-v2
id=IDX0
name=Atomic semantic contributions and workspace index
phase=expansion
train=expansion.kernel
product=kernel
kind=implementation
semantic_role=delivery
class=successor
predecessors=TIF1,DEM0
conditional_predecessors=
owner=expansion.kernel:typed immutable universal catalog and demand-selected kernel services
conflict_domains=semantic_authority,performance_evidence
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
source_refs=source:successor-expansion.md:L914
external_requirements=
activation_gate=ORC0
charter=charters/expansion-kernel/IDX0.md
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# IDX0 — Atomic semantic contributions and workspace index

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Atomic semantic contributions and workspace index. The current owner is **framework-shaped host/session registries and untagged public boundaries**. The final and sole owner is **typed immutable universal catalog and demand-selected kernel services**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_language/src`, `crates/verter_session/src`, `crates/verter_protocol/src`, `crates/verter_identity/src`.
- Named API/data boundaries: `CarrierProfileId`, `FrameworkProfileId`, `ProjectProfileId`, `CatalogSnapshot`, `DemandPlan`, `TypeInfoRequest`.
- Mutation boundary: bounded validation, named residual deletion, and/or one atomic route switch only; no new authority may be introduced.

## Exact predecessor contracts

- **TIF1:** exact current receipt ID and digest for “TypeInfo-first ComponentInfo and component-meta cutover”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **DEM0:** exact current receipt ID and digest for “Selection, two-stage activation, and demand planning”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Revision:** 4 — supersedes the 251-charter all-verticals proposal
- **Prepared:** 2026-08-26
- **Repository basis:** program/architecture-lock at d1f3d50a948597f036868543b9bb21acacd730ff
- **Current-program condition:** maintainer work freeze; TCM0 = RESCOPE_REQUIRED; TCM1–TCM4 = LOCKED

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **IDX0-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **IDX0-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **IDX0-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **IDX0-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
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
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, a predecessor receipt is stale, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_language -p verter_protocol -p verter_session`
2. Run every final command in the bound `targeted-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the exact candidate tree, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact frozen worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; 3 fresh distinct harness review tasks for exactly `adversarial`, `conformance`, `architecture-specialist`, deterministic low|medium|high effort and exact author/task/agent/provider/model bindings; immutable-review-worktree and cleanup policy; and the required terse report-back schema. These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:successor-expansion.md:L914`

## Reconciled source-plan contract

**Intent:** provide bounded cross-file/cross-framework discovery without turning an index into semantic authority.
**Predecessors:** `TIF1`, `DEM0`.
**Subblocks:** (1) define contribution identities, typed node/edge tables, and source bases; (2) define staged atomic deltas; (3) define dependency read sets and invalidation; (4) implement bounded candidate/name/component/link/registration indexes; (5) represent set-valued project memberships; (6) test cancellation, incomplete enumeration, incremental/fresh equivalence, and memory plateau.
**Acceptance:** rename/consumer/search queries obtain stable bounded candidates across Vue/Svelte fixtures while authoritative resolution remains downstream of the owning vertical/TypeInfo operation; cancelled or partial walks publish nothing cacheable.
**Forbidden:** checker APIs in index storage, global eager workspace crawling, negative admission after budget exhaustion, or opaque unversioned payloads.
**Deletion/abort:** consolidate displaced framework indexes only with query/result parity; abort if a stored fact cannot name its authority and invalidation basis.

## Collapsed non-authoritative subblock disposition

The recovery candidate mechanically split this source-owned atomic node into the following labels: `IDX0-A`, `IDX0-B`, `IDX0-C`, `IDX0-D`, `IDX0-E`, `IDX0-F`. They have no separate dispatch, lease, receipt, migration manifest, deletion ownership, or review standing. Their useful source-described concerns are internal RED/GREEN checklist items of **IDX0**; IDX0 alone owns the complete migration population, exactly one final deletion/cutover, and atomic acceptance. Any quoted “suggested subblock” wording in transferred source text is non-authoritative planning context.

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-EXP-L914-FAD6801A6656

- Kind: `context`
- Source: `successor-expansion.md:914-914`
- Applicability: `IDX0`
- Exact text SHA-256: `fad6801a6656389a90c5e6061402465ed3d05fee1e6d2c719bb1362c874f7bd6`

~~~~markdown
### `IDX0.md` — Atomic semantic contributions and workspace index
~~~~

### SRC-EXP-L916-28159D32BB8D

- Kind: `forbidden`
- Source: `successor-expansion.md:916-921`
- Applicability: `IDX0`
- Exact text SHA-256: `28159d32bb8db43c6f66c8f52f4f0f88486e90c232e58cf2b23847c877896e54`

~~~~markdown
**Intent:** provide bounded cross-file/cross-framework discovery without turning an index into semantic authority.
**Predecessors:** `TIF1`, `DEM0`.
**Subblocks:** (1) define contribution identities, typed node/edge tables, and source bases; (2) define staged atomic deltas; (3) define dependency read sets and invalidation; (4) implement bounded candidate/name/component/link/registration indexes; (5) represent set-valued project memberships; (6) test cancellation, incomplete enumeration, incremental/fresh equivalence, and memory plateau.
**Acceptance:** rename/consumer/search queries obtain stable bounded candidates across Vue/Svelte fixtures while authoritative resolution remains downstream of the owning vertical/TypeInfo operation; cancelled or partial walks publish nothing cacheable.
**Forbidden:** checker APIs in index storage, global eager workspace crawling, negative admission after budget exhaustion, or opaque unversioned payloads.
**Deletion/abort:** consolidate displaced framework indexes only with query/result parity; abort if a stored fact cannot name its authority and invalidation basis.
~~~~

### SRC-LEGACY-TRANSFER-ECAADB1B854E

- Kind: `requirement`
- Source: `legacy-architecture-transfers.md:243-248`
- Applicability: `IDX0`, `VIM0`, `VIM1`, `LSO2`, `LSO5`, `LSO9`, `NCF-JF-VUE`
- Exact text SHA-256: `c2db7c1b4ca376ed2a6d9ddb96840536a370594a539841365fa8d343435c98a6`

~~~~markdown
### LEGACY-TRANSFER-ECAADB1B854E

- Original path: `docs/arch/global-components-ide-typing.md`; Git blob: `ecaadb1b854e9b78d3190fbc134b28aa4afc1d3b`; exact source SHA-256: `91e2b3a92b783d36a858bea63e96f40ac0b4bf2ec9285d511275aa64c0d70208`.
- Exact retained source: `sources/legacy-architecture-transfers/global-components-ide-typing.md`.
- Applicable authority: `IDX0`, `VIM0`, `VIM1`, `LSO2`, `LSO5`, `LSO9`, `NCF-JF-VUE`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-5D71514A73D7

- Kind: `requirement`
- Source: `legacy-architecture-transfers.md:467-472`
- Applicability: `IDX0`, `LSO2`
- Exact text SHA-256: `146111eddec45ce0d2c17db3d5e0bd55996c8fccde9c44c9e4b999fea7ec8c74`

~~~~markdown
### LEGACY-TRANSFER-5D71514A73D7

- Original path: `docs/arch/relocation-severs-reachability.md`; Git blob: `5d71514a73d7a0744c504ef7c1be7e98a78f04f1`; exact source SHA-256: `00502de7b7cf9a6ba07df3159a85098f634699894489c5f039a8672f42a6c672`.
- Exact retained source: `sources/legacy-architecture-transfers/relocation-severs-reachability.md`.
- Applicable authority: `IDX0`, `LSO2`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-EXISTING-NODE-AMENDMENT-IDX0

- Kind: `requirement`
- Source: `existing-node-amendments.md:59-73`
- Applicability: `IDX0`
- Exact text SHA-256: `2825fccb2c5b9dc900820f9700b5523397f818c7e9da35a21c2b194190328045`

~~~~markdown
## IDX0 — Atomic semantic contributions and workspace index

Add:

- index entries may store target/contribution/occurrence candidates, typed memberships, dependency read sets, and authored source bases;
- indexes may not store checker verdicts, final navigation targets, rename plans, or public operation answers;
- incomplete/budget-exhausted enumeration cannot admit a negative complete result;
- framework global registrations, component links, aliases/reexports, and project memberships are set-valued, profile-qualified, and atomically versioned;
- target and occurrence planners must validate candidates downstream against the semantic owner.

Acceptance additions:

- `idx0_candidates_are_not_authoritative_targets_or_diagnostics`
- `idx0_partial_enumeration_never_negative_admits`
- `idx0_profile_qualified_registrations_do_not_alias`
~~~~
