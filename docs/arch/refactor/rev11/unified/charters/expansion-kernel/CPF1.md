<!-- unified-charter-v2
id=CPF1
name=Carrier frontend registration and Vue/Svelte cutover
phase=expansion
train=expansion.kernel
product=kernel
kind=cutover
semantic_role=delivery
class=successor
predecessors=CPF0,CAT0
conditional_predecessors=
owner=expansion.kernel:typed immutable universal catalog and demand-selected kernel services
conflict_domains=carrierprofileid,vue_product,svelte_product
resource_class=rust-mixed
review_profile=architecture-3
gate_profile=targeted-domain
size=S
dispatchable=true
optional=false
release_gating=none
source_refs=source:successor-expansion.md:L806
external_requirements=
activation_gate=ORC0
charter=charters/expansion-kernel/CPF1.md
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# CPF1 — Carrier frontend registration and Vue/Svelte cutover

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Carrier frontend registration and Vue/Svelte cutover. The current owner is **framework-shaped host/session registries and untagged public boundaries**. The final and sole owner is **typed immutable universal catalog and demand-selected kernel services**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_language/src`, `crates/verter_session/src`, `crates/verter_protocol/src`, `crates/verter_identity/src`.
- Named API/data boundaries: `CarrierProfileId`, `FrameworkProfileId`, `ProjectProfileId`, `CatalogSnapshot`, `DemandPlan`, `TypeInfoRequest`.
- Mutation boundary: bounded validation, named residual deletion, and/or one atomic route switch only; no new authority may be introduced.

## Exact predecessor contracts

- **CPF0:** exact current receipt ID and digest for “Carrier frontend/compiler-backend separation proof”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **CAT0:** exact current receipt ID and digest for “Immutable typed catalog snapshot and static registration”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Revision:** 4 — supersedes the 251-charter all-verticals proposal
- **Prepared:** 2026-08-26
- **Repository basis:** program/architecture-lock at d1f3d50a948597f036868543b9bb21acacd730ff
- **Current-program condition:** maintainer work freeze; TCM0 = RESCOPE_REQUIRED; TCM1–TCM4 = LOCKED

## Acceptance IDs and discriminating proof

- **CPF1-AC1 — sole-owner proof:** add `cpf1_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **CPF1-AC2 — positive contract:** add `cpf1_publishes_exact_carrierprofileid`; assert exact identities, provenance, completeness, and deterministic ordering.
- **CPF1-AC3 — incremental equivalence:** add `cpf1_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **CPF1-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
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

- `source:successor-expansion.md:L806`

## Reconciled source-plan contract

**Intent:** atomically install the frontend/backend split and migrate current carriers.
**Predecessors:** `CPF0`, `CAT0`.
**Subblocks:** (1) add `CarrierFrontendRegistry`; (2) add optional `CarrierCompilerBackendRegistry`; (3) migrate Vue/Svelte parse, source-unit, IDE-projection, fact, and compile routes; (4) replace central `CarrierGrammarConfig::{Vue,Svelte}` with owner-local typed configs; (5) update generated client and capability guards; (6) delete the combined registry/trait.
**Acceptance:** Vue/Svelte authored bytes, parse facts, recovery, IDE projection, maps, compilation, cache hits, and public outputs are equivalent on pinned corpora; “all carriers have a frontend, only compile-capable carriers require a backend” is mechanically exhaustive.
**Forbidden:** dual-running registries, public erased artifacts, central grammar switches, or a compatibility bridge that becomes an authority.
**Deletion/abort:** combined compiler registry/trait and stale guards are deleted atomically; abort on unexplained output/map/performance divergence.

## Collapsed non-authoritative subblock disposition

The recovery candidate mechanically split this source-owned atomic node into the following labels: `CPF1-A`, `CPF1-B`, `CPF1-C`, `CPF1-D`, `CPF1-E`, `CPF1-F`. They have no separate dispatch, lease, receipt, migration manifest, deletion ownership, or review standing. Their useful source-described concerns are internal RED/GREEN checklist items of **CPF1**; CPF1 alone owns the complete migration population, exactly one final deletion/cutover, and atomic acceptance. Any quoted “suggested subblock” wording in transferred source text is non-authoritative planning context.

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-COMP-L1901-98B1EF372E16

- Kind: `context`
- Source: `compiler-proposal.md:1901-1901`
- Applicability: `CPF1`
- Exact text SHA-256: `98b1ef372e16ee003894d2d3e34c8dc96c86c5c2198e7ff3ac57c117c47bacdd`

~~~~markdown
## 11.2 `CPF1`
~~~~

### SRC-COMP-L1903-ABE8AD244987

- Kind: `deletion`
- Source: `compiler-proposal.md:1903-1903`
- Applicability: `CPF1`
- Exact text SHA-256: `abe8ad244987cba3a4d39f2dde5ec8149676a9c7004a63074428543c7fec0f07`

~~~~markdown
Make `CPF1` the successor catalog integration and temporary-bridge deletion owner, not a second carrier split.
~~~~

### SRC-COMP-L1905-62223B1E2571

- Kind: `context`
- Source: `compiler-proposal.md:1905-1905`
- Applicability: `CPF1`
- Exact text SHA-256: `62223b1e25718add41f2fd094f5bb595ce9cce321a9500976c31119be2656df0`

~~~~markdown
Typed tables should include:
~~~~

### SRC-COMP-L1907-BBDD00122625

- Kind: `context`
- Source: `compiler-proposal.md:1907-1913`
- Applicability: `CPF1`
- Exact text SHA-256: `bbdd001226256fa5eb1d6f25bc6ba282bb969a1c8ff630055ec615225ff53f2e`

~~~~markdown
```text
carrier_frontends
framework_semantic_authorities
projection_backends
runtime_compilers
framework_host_integrations
```
~~~~

### SRC-COMP-L1915-B175F5D7AFBB

- Kind: `requirement`
- Source: `compiler-proposal.md:1915-1915`
- Applicability: `CPF1`
- Exact text SHA-256: `b175f5d7afbb9fcd687a1161fa87298a9f6e707aa4c3a07c663a47f49566f9b7`

~~~~markdown
Only one immutable catalog authority is permitted.
~~~~

### SRC-EXP-L806-6610DA9294BB

- Kind: `context`
- Source: `successor-expansion.md:806-806`
- Applicability: `CPF1`
- Exact text SHA-256: `6610da9294bb730afc4a7df299718b925fedb54ab88f2f4e2440a010d43ccdd9`

~~~~markdown
### `CPF1.md` — Carrier frontend registration and Vue/Svelte cutover
~~~~

### SRC-EXP-L808-EED3D39EE390

- Kind: `forbidden`
- Source: `successor-expansion.md:808-813`
- Applicability: `CPF1`
- Exact text SHA-256: `eed3d39ee390f8a0844f58128cdd0957591d1a02b51bd0977756040537bfd8ab`

~~~~markdown
**Intent:** atomically install the frontend/backend split and migrate current carriers.
**Predecessors:** `CPF0`, `CAT0`.
**Subblocks:** (1) add `CarrierFrontendRegistry`; (2) add optional `CarrierCompilerBackendRegistry`; (3) migrate Vue/Svelte parse, source-unit, IDE-projection, fact, and compile routes; (4) replace central `CarrierGrammarConfig::{Vue,Svelte}` with owner-local typed configs; (5) update generated client and capability guards; (6) delete the combined registry/trait.
**Acceptance:** Vue/Svelte authored bytes, parse facts, recovery, IDE projection, maps, compilation, cache hits, and public outputs are equivalent on pinned corpora; “all carriers have a frontend, only compile-capable carriers require a backend” is mechanically exhaustive.
**Forbidden:** dual-running registries, public erased artifacts, central grammar switches, or a compatibility bridge that becomes an authority.
**Deletion/abort:** combined compiler registry/trait and stale guards are deleted atomically; abort on unexplained output/map/performance divergence.
~~~~
