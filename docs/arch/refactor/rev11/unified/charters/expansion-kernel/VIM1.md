<!-- unified-charter-v2
id=VIM1
name=Deterministic manifest compiler and conformance generator
phase=expansion
train=expansion.kernel
product=kernel
kind=implementation
semantic_role=delivery
class=successor
predecessors=VIM0,CEF0,COX0,LRA0,FMK0,PUB0,PER0
conditional_predecessors=
owner=expansion.kernel:typed immutable universal catalog and demand-selected kernel services
conflict_domains=compiler_execution
resource_class=rust-mixed
review_profile=architecture-3
gate_profile=targeted-domain
size=S
dispatchable=true
optional=false
release_gating=none
source_refs=source:successor-expansion.md:L995
external_requirements=
activation_gate=ORC0
charter=charters/expansion-kernel/VIM1.md
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# VIM1 — Deterministic manifest compiler and conformance generator

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Deterministic manifest compiler and conformance generator. The current owner is **framework-shaped host/session registries and untagged public boundaries**. The final and sole owner is **typed immutable universal catalog and demand-selected kernel services**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_language/src`, `crates/verter_session/src`, `crates/verter_protocol/src`, `crates/verter_identity/src`.
- Named API/data boundaries: `CarrierProfileId`, `FrameworkProfileId`, `ProjectProfileId`, `CatalogSnapshot`, `DemandPlan`, `TypeInfoRequest`.
- Mutation boundary: bounded validation, named residual deletion, and/or one atomic route switch only; no new authority may be introduced.

## Exact predecessor contracts

- **VIM0:** exact current receipt ID and digest for “Vertical conformance manifest schema”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **CEF0:** exact current receipt ID and digest for “Custom Element producer/consumer interoperability contract”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **COX0:** exact current receipt ID and digest for “Per-profile editor participation and coexistence”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **LRA0:** exact current receipt ID and digest for “Profile-scoped diagnostics, lint, fixes, and actions”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **FMK0:** exact current receipt ID and digest for “Formatter ownership, composition, and compatibility contract”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **PUB0:** exact current receipt ID and digest for “Versioned public request/result and capability truth”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **PER0:** exact current receipt ID and digest for “Cache/backend identity, cancellation, budgets, and zero work”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Revision:** 4 — supersedes the 251-charter all-verticals proposal
- **Prepared:** 2026-08-26
- **Repository basis:** program/architecture-lock at d1f3d50a948597f036868543b9bb21acacd730ff
- **Current-program condition:** maintainer work freeze; TCM0 = RESCOPE_REQUIRED; TCM1–TCM4 = LOCKED

## Acceptance IDs and discriminating proof

- **VIM1-AC1 — sole-owner proof:** add `vim1_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **VIM1-AC2 — positive contract:** add `vim1_publishes_exact_carrierprofileid`; assert exact identities, provenance, completeness, and deterministic ordering.
- **VIM1-AC3 — incremental equivalence:** add `vim1_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **VIM1-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
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

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three named independent reviewers with assigned lenses, model `gpt-5.6-sol`, effort `ultra`, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:successor-expansion.md:L995`

## Reconciled source-plan contract

**Intent:** make CI and agents enforce the same vertical rules through repository-owned tooling.
**Predecessors:** `VIM0`, `CEF0`, `COX0`, `LRA0`, `FMK0`, `PUB0`, `PER0`.
**Subblocks:** (1) `cargo xtask vertical new`; (2) `check`; (3) `matrix`; (4) `charters`; (5) `test-plan`; (6) generated descriptor/client/capability/test registration checks; (7) deterministic output and forbidden-dependency closure.
**Acceptance:** two clean runs are byte-identical; malformed/negative manifests fail for semantic reasons rather than keyword grep; generated charters contain all required cells but no semantic implementation; CI invokes the same validator API used by skills.
**Forbidden:** skill-local validation authority, source rewriting outside declared generated files, auto-ratification, or generating framework algorithms.
**Deletion/abort:** remove hand-maintained mirrors only after freshness guards prove replacement; abort if generation would require executing vertical code.

## Collapsed non-authoritative subblock disposition

The recovery candidate mechanically split this source-owned atomic node into the following labels: `VIM1-A`, `VIM1-B`, `VIM1-C`, `VIM1-D`, `VIM1-E`, `VIM1-F`, `VIM1-G`. They have no separate dispatch, lease, receipt, migration manifest, deletion ownership, or review standing. Their useful source-described concerns are internal RED/GREEN checklist items of **VIM1**; VIM1 alone owns the complete migration population, exactly one final deletion/cutover, and atomic acceptance. Any quoted “suggested subblock” wording in transferred source text is non-authoritative planning context.

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-EXP-L995-57BBEF470724

- Kind: `context`
- Source: `successor-expansion.md:995-995`
- Applicability: `VIM1`
- Exact text SHA-256: `57bbef470724c4c2bfe2b5649aeb71f7f98310d330219a085b211b7384214c8b`

~~~~markdown
### `VIM1.md` — Deterministic manifest compiler and conformance generator
~~~~

### SRC-EXP-L997-62B859F666AF

- Kind: `forbidden`
- Source: `successor-expansion.md:997-1002`
- Applicability: `VIM1`
- Exact text SHA-256: `62b859f666af52d2741baca22a0c1dc3613ddc2f2bc8e69d818e19c7f3907dd7`

~~~~markdown
**Intent:** make CI and agents enforce the same vertical rules through repository-owned tooling.
**Predecessors:** `VIM0`, `CEF0`, `COX0`, `LRA0`, `FMK0`, `PUB0`, `PER0`.
**Subblocks:** (1) `cargo xtask vertical new`; (2) `check`; (3) `matrix`; (4) `charters`; (5) `test-plan`; (6) generated descriptor/client/capability/test registration checks; (7) deterministic output and forbidden-dependency closure.
**Acceptance:** two clean runs are byte-identical; malformed/negative manifests fail for semantic reasons rather than keyword grep; generated charters contain all required cells but no semantic implementation; CI invokes the same validator API used by skills.
**Forbidden:** skill-local validation authority, source rewriting outside declared generated files, auto-ratification, or generating framework algorithms.
**Deletion/abort:** remove hand-maintained mirrors only after freshness guards prove replacement; abort if generation would require executing vertical code.
~~~~
