<!-- unified-charter-v2
id=DX0
name=Cross-surface feature preservation and exposure constitution
predecessors=ORC0,STP0
phase=expansion
train=expansion.product-observability
product=dx_product
kind=constitution
semantic_role=delivery
class=successor
owner=expansion.product-observability:Product observability, feature exposure and reproducibility
conflict_domains=public_protocol,capability_catalog,product_inspection
resource_class=docs-light
gate_profile=docs-domain
review_profile=architecture-3
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=low
verification_effort_default=low
confirmation_effort_min=medium
confirmation_effort_default=medium
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/expansion-product-observability/DX0.md
size=M
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=600
rescope_files=6
rescope_unrelated_packages=3
-->

# DX0 - Cross-surface feature preservation and exposure constitution

## Independently acceptable outcome

Ratify the complete producer-to-surface obligation model and capture existing desktop, playground and editor behavior before adding consumers.

This is a contract/inventory outcome. No production implementation belongs in this node.

## Binding architecture and responsibility

Read `contracts/product-experience.md` and this train's plan in `plans/expansion-product-observability.md`. All current source identity, semantic authority, TypeScript projection, mapping, lifetime and authored-edit contracts remain binding. This charter adds consumer/execution requirements; it is not permission to weaken existing Vue/Svelte or web-language support. Breaking changes are allowed with explicit migration and one final owner.

- Production surfaces: `crates/verter_protocol/src/inspection`, `crates/verter_session/src/inspection`, `packages/language-shared/src/inspection`
- Placement notes: crates/verter_protocol/src/inspection/ (new adapter vocabulary); crates/verter_session/src/inspection/ (new facade); packages/language-shared/src/inspection/ (new bindings). These identify ownership and intended new modules, not a claim that every listed subdirectory already exists. Verify exact paths and symbols on the implementation candidate before mutation.
- Final owner: `expansion.product-observability` for this product outcome; semantic algorithms, project resolution, format/lint logic, TypeScript projection and edit transactions stay with their existing producing owners.
- Conflict domains: `public_protocol`, `capability_catalog`, `product_inspection`. Register narrow real roots before parallel mutation; no new orchestration system is introduced.

## Exact predecessors and received outcomes

- **ORC0**: Trusted implementation-ledger cutover. Consume its accepted outcome; implementation state is resolved by the existing PM/ledger, not inferred from this document.
- **STP0**: Projection constitution and current-feature preservation contract. Consume its accepted outcome; implementation state is resolved by the existing PM/ledger, not inferred from this document.

These are contract dependencies, not resource-capacity locks. Do not wait for a whole UI product where an accepted shared producer is sufficient. A discovered missing producer/API is a receiving-owner amendment before code, not a local substitute or an unrecorded dependency.

## Interfaces and delivered products

- `FeatureExposureContract: operation/profile/version, producer owner, maturity and RequiredCurrent obligation`
- `HostExecutionClass: Portable, NativeOnly, ExternalOwner; no hidden replacement engine`
- `ProductReceiptBasis: source revisions, project/configuration, engine and host identity`

The names above identify the contracts to produce or adapt, not assumed existing APIs. Reuse the canonical outcome, identity and capability vocabulary instead of introducing synonymous types. Each exposed product binds source/project/configuration/profile, execution host and engine version, its completeness state and its invalidation/lifetime rules.

## Scoped implementation sequence

### DX0.1

Inventory the actual current commands, analyses, compiler outputs, lint/format operations, TypeInfo queries, semantic/flow facts, mappings, provider modes and editor clients. Existing Lapce/Neovim/Helix/Zed integrations must be checked before proposing replacements.

### DX0.2

Define a normal semantic rail, an inspection rail and an explicitly selected comparison rail. TypeScript remains the authority for strict Vue/Svelte projection typing. Verter Flow/native-checker inspection is separately labelled and cannot supply replacement TypeScript answers.

### DX0.3

Select one architecture: shared producer services with typed consumer adapters. Reject independent playground semantics and a universal browser proxy that silently uploads projects. Admit pure-browser and explicit local-native execution as distinct products.

### DX0.4

Required exposure means an executable operation, source-linked explanation, reproducible scenario and observable error state; a disabled button or JSON screenshot is not completion. Missing host feasibility remains an open obligation.

At preflight, map each step to the smallest owning source/test files and identify the existing characterisation it preserves. Record only meaningful scope or placement amendments. Setup, documentation and configuration belong with the outcome that needs them; independent semantic algorithms or platform subsystems need separate nodes.

## Snapshot, cancellation and boundary rules

Use the canonical immutable observation basis. Reject old-source, old-project, old-engine, old-worker and wrong-result-handle outcomes; never publish stale results as current. Cancellation prevents additional irrelevant work and releases retained handles. Partial, pending, unsupported, ambiguous and failed are distinct from complete-empty. Cross-file or browser/native edits are authored, version-checked and atomic through the existing transaction authority.

For UI/adapter work, preserve semantic meaning under reduced client capabilities. Rich information is fetched on demand; closed/disabled views create no background semantic work. A protocol smoke or screenshot is not real-editor responsiveness or semantic correctness proof. For contract-only nodes, specify these obligations without constructing a speculative implementation.

## Acceptance and discriminating proof

**DX0-AC1.** Catalog a current feature missing from the playground: promotion must fail until an executable route and test exist.

**DX0-AC2.** Mark a native-only feature as browser-local without a build: reject the capability claim.

**DX0-AC3.** Separate native Flow facts from TypeScript hover/diagnostics in the same source file.

**DX0-AC-OWNER.** Prove one final owner and the stated retirement obligation. Reuse/extend existing coverage; add negative tests only for a plausible wrong-complete, stale, unsafe or duplicated-authority failure.

**DX0-AC-BASIS.** Where this outcome changes caching, mapping, snapshots, result handles or incremental publication, compare incremental and fresh execution on the same basis. For a pure contract/inventory outcome, bind the corresponding downstream test owner instead of adding a meaningless runtime test.

**DX0-AC-RESOURCE.** Where hot paths or UI are touched, use WSP's equal-work corpus and the limits in the shared contract, with all required features enabled. Include actual client application/paint for a real-client claim, provider process costs, outbound bytes and retained memory. Explicitly label unavailable metrics. No general performance claim follows from a compiler microbenchmark.

**DX0-AC-EXPOSURE.** Every supported operation introduced here supplies its DX1 registration, executable playground adapter and replay case. Before the explorer is available, register the schema/case and keep product promotion blocked on executable exposure. Core implementation is not forced to wait for PG17; a generic typed explorer view is sufficient. Native-only functionality requires real explicitly selected native execution, not an inactive card.

## Migration, deletions and forbidden routes

Supersede informal feature lists as promotion evidence, not the canonical capability catalog or existing implementation state.

Characterise the existing user-visible feature first, route it through its surviving producer, verify consumers, then remove the named obsolete route. Do not remove useful existing behavior merely because the new presentation is not ready. Inert/Dormant adapters may land before the owning activation boundary, but a cutover cannot retain two active semantic authorities.

Forbidden: a second semantic/type/project engine inside a client; regex/name-based semantic truth; generated coordinates passed off as authored coordinates; hidden source uploads; arbitrary companion shell execution; dropped diagnostics or weakened typing to meet timing; guessed zeros for missing telemetry; sleeps as readiness; stale publication; unexplained UI feature removal; and declaring product superiority from installation or protocol smoke alone.

## Scope budget and rescope

Reference production budget: 0 LOC, 0 files and 0 related crates/packages. A contract-only node changes roadmap/contract/test-plan material only. Investigate material scope drift; split before combining unrelated outcomes or a substantial semantic algorithm with public-wire and concurrency/lifetime changes. The 600-LOC/6-file/3-unrelated-package rescope triggers are review boundaries, not permission to omit required behavior. Targeted tests and data fixtures are sized by discriminatory value, not a test-count quota.

The scope is aborted/replanned on unknown authority, unverified browser feasibility, destructive edits, an impossible shared-owner dependency or a performance result obtained by doing less required work. Product acceptance remains open until the documented behavior is achieved.

## Verification and review handoff

1. Inspect the actual predecessor APIs, current package scripts, canonical capabilities and existing tests on the implementation candidate. No script named only in this plan is presumed runnable.
2. For behavior changes, reproduce the relevant failing boundary before changing production code; extend existing tests rather than duplicating test permutations. Contract-only changes use schema/dependency/ownership checks.
3. Bind the concrete acceptance scenarios above to real test/harness cases and record exact commands, versions and outcomes. WSP1 owns new performance instrumentation; JBT1 owns the real JetBrains harness; client terminals require their actual clients.
4. Run the currently binding `docs-domain` final profile in the real repository. The supplied export's older retained source assets are not a substitute for the live authority. Structural delta validation is not a runtime or full-repository gate.
5. Complete the existing `architecture-3` independent review profile and resolve blocking findings. Product/security/concurrency/performance claims require the appropriate specialist lens and fresh evidence after changes.

Use the existing PM completion workflow and, where applicable, its authorised implementation-ledger projection. This charter creates no SHA/receipt/lease-based orchestration policy and no duplicate status store. Do not mark the node complete while only its charter, stub, scaffold or test harness exists. New milestones/releases are assigned separately; this node does not change the existing L4 release boundary.

## Roadmap review 2026-09-18

Inventory-heavy constitution: implementation stays high because DX0.1 inventories every current command, analysis and client; verification is a docs gate.

Sizing: size M, 0 production LOC across 0 files, rescope at 600/6; efforts implementation high, review high, verification low, confirmation medium. Effective predecessors: ORC0, STP0. The delta proposed one uniform budget (M, 800 LOC, 8 files, every effort high) for all 115 nodes; this review sized each node from its actual owned outcome.
