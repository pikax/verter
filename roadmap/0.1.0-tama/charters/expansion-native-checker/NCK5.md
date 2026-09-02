<!-- unified-charter-v2
id=NCK5
name=Framework diagnostic contribution ingress and profile isolation
predecessors=NCK1,NCK3,TIF1,IDX0,VIM1
phase=expansion
train=expansion.native-checker
product=native_checker
kind=implementation
semantic_role=delivery
class=successor
owner=expansion.native-checker:sole native semantic diagnostic authority and certified-family cutover
conflict_domains=semantic_authority,capability_catalog,vertical_manifest
resource_class=rust-mixed
gate_profile=targeted-domain
review_profile=architecture-3
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=high
verification_effort_default=high
confirmation_effort_min=high
confirmation_effort_default=high
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/expansion-native-checker/NCK5.md
size=M
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# NCK5 — Framework diagnostic contribution ingress and profile isolation

Readiness is derived only from implemented-ledger rows for the node ancestors. Commit message, approximate timezone-bearing date, and optional PR are loose locator hints; the CLI performs no Git, GitHub, SHA, tree, ancestry, receipt, lease, or digest validation.

## Independently acceptable outcome

Implement validated framework semantic and diagnostic contribution ingress, executable template regions, component-contract facts, profile isolation, and Vue/Svelte canaries over the same checker kernel. Core remains framework-neutral and generated TypeScript remains an interoperability surface, not semantic truth.

The current owner is **framework-specific generated TSX checks, component-meta adapters, template diagnostics, and incomplete typed contribution seams**. The final and sole owner is **profile-registered typed framework contributions admitted into the same ProgramAnalysisGraph, relation/call/flow facts, and NCK rule kernel**.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_language/src`, `crates/verter_compiler/src/framework`, `crates/verter_semantic`, `crates/verter_session`, `crates/verter_vue_conformance`, `crates/verter_svelte_conformance`, `crates/verter_protocol`.
- Pack production inventory:
- `crates/verter_language/src` and universal catalog/profile registration
- `crates/verter_compiler/src/framework` only for typed lowering outputs already owned by framework frontends
- `crates/verter_semantic` and `crates/verter_session` for validated contribution snapshots and checker ingress
- `crates/verter_vue_conformance` and `crates/verter_svelte_conformance` for canary fixtures
- `crates/verter_protocol`/TypeInfo only where component contracts are public through existing universal contracts

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.
- Named API/data boundaries:
- `FrameworkDiagnosticContributor` or catalog capability equivalent
- `FrameworkRegionContribution`, `TemplateScopeContribution`, and `ComponentContract`
- `InjectedBinding`, `InjectedNarrowingFact`, `InjectedContextualType`, and `InjectedRelationDemand`
- `ProfileSemanticEpoch`, `FrameworkContributionSnapshot`, and `ContributionValidationReceipt`
- `FrameworkDiagnosticRuleDescriptor` for intrinsic framework semantics only

## Exact predecessor contracts

- **NCK1:** implemented ledger row for “Executable-region and typed semantic-contribution contract”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **NCK3:** implemented ledger row for “Shared-proof semantic diagnostic rule kernel”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **TIF1:** implemented ledger row for “TypeInfo-first ComponentInfo and component-meta cutover”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **IDX0:** implemented ledger row for “Atomic semantic contributions and workspace index”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **VIM1:** implemented ledger row for “Deterministic manifest compiler and conformance generator”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

### Binding architecture

- Framework adapters lower syntax into typed contributions; they do not resolve types, run private relation/call algorithms, or call a framework checker.
- Template control flow and handlers become ExecutableRegionKind::FrameworkRegion with exact authored coordinates and profile identity.
- Generated TSX may remain an external-provider interoperability carrier but cannot be the native semantic fact source.
- Component contracts are framework-neutral: inputs/props, outputs/events, slots/children/content, exposed instance, refs, directives/actions, and lifecycle bindings.
- Framework-owned diagnostics are limited to intrinsic framework semantics. TypeScript-semantic diagnostics use common NCF rules over shared facts.
- Contributions are admitted atomically per profile/source basis and invalidated by exact read sets.
- No core branch on framework name is permitted; capability/catalog dispatch selects contributors.

### Internal subblocks

#### NCK5-SB1 - Profile contributor registration

**Independently testable outcome:** Framework semantic contribution capabilities are immutable catalog entries selected by exact profile.

**Architecture:**

- Register contributor, region kinds, component contract capabilities, and intrinsic diagnostic rules.
- Separate file discovery/indexing from contribution execution.
- Define Disabled/WorkspaceOnly/Full zero-work behavior with COX0.

**Expected changes:**

- Extend catalog descriptors and generated registration.
- Replace central framework switches in checker ingress.

**Discriminating proof:**

- Two profiles for the same file kind do not collide.
- Disabled and non-applicable profiles execute no contribution work.

#### NCK5-SB2 - Template executable-region lowering

**Independently testable outcome:** Vue and Svelte template bodies/branches/handlers are represented as authored framework regions.

**Architecture:**

- Lower lexical scopes, branches, loops, event handlers, slot/snippet bodies, and expression anchors into compact region descriptors.
- Reuse native flow/relation/call facts through declarative demands.
- Keep framework AST nodes and source maps in frontend ownership.

**Expected changes:**

- Implement region contribution builders for canary subsets.
- Add exact source/UTF encoding and profile provenance.

**Discriminating proof:**

- Template branch narrowing and handler call canaries match fresh/incremental behavior.
- No generated TSX text is read by native region execution.

#### NCK5-SB3 - Binding, contextual, and relation contributions

**Independently testable outcome:** Framework scopes contribute typed facts/demands without mutating semantic nodes.

**Architecture:**

- Contribute template bindings, contextual targets, narrowing facts, event payload expectations, directive effects, and relation demands.
- Validate canonical symbol/provenance and reject unresolved fake facts.
- Let the executor resolve declarative demands through the one semantic dispatch.

**Expected changes:**

- Implement contribution conversion and validation.
- Capture exact read sets and profile/source epochs.

**Discriminating proof:**

- Forged canonical symbol, stale source, or foreign-profile contribution is rejected.
- Equivalent TS and template semantic facts converge to the same relation/call outcomes.

#### NCK5-SB4 - Framework-neutral component contract

**Independently testable outcome:** Component surfaces from Vue and Svelte lower into one typed contract used by checker, TypeInfo, and language-service operations.

**Architecture:**

- Define inputs, outputs, slots/content, exposed instance, refs, directives/actions, models/bindings, and lifecycle values.
- Separate contract identity from framework presentation names.
- Reuse TIF1 component authority and avoid a duplicate component metadata store.

**Expected changes:**

- Implement adapter normalization into existing universal component contracts or amend them atomically.
- Migrate canary component checks to common relation rules.

**Discriminating proof:**

- Vue/Svelte equivalent contracts produce common query behavior while preserving framework provenance.
- A duplicate component authority guard rejects same-role stores.

#### NCK5-SB5 - Intrinsic framework diagnostic rules

**Independently testable outcome:** Only genuinely framework-specific semantics register framework-owned rules.

**Architecture:**

- Examples: invalid directive/action usage, slot/snippet contract shape, framework binding constraints, component registration rules.
- Common assignment/call/flow errors remain common NCF families.
- Framework rules declare exact contributed fact requirements and fix intents.

**Expected changes:**

- Implement a small Vue and Svelte canary set.
- Classify remaining legacy framework diagnostics in VIM/NCF manifests.

**Discriminating proof:**

- A rule requiring a framework-name branch in core fails architecture review.
- Canaries perform zero work on the other framework profile.

#### NCK5-SB6 - Atomic contribution snapshot and isolation proof

**Independently testable outcome:** Updates, cancellation, and profile changes cannot leak stale framework facts or diagnostics.

**Architecture:**

- Stage contribution batches and atomically swap only complete validated snapshots.
- Bind checker query reads to profile semantic epoch and exact source basis.
- Cancel superseded contribution/check work and refuse warm publication.

**Expected changes:**

- Implement project-scoped snapshot storage and teardown.
- Instrument contribution count, validation, reuse, and retained bytes.

**Discriminating proof:**

- Rapid edit/profile-switch tests publish no mixed-epoch diagnostics.
- Long churn plateaus memory and fresh equals incremental across both frameworks.

### Identity, invalidation, and publication

- A framework contribution is data plus provenance; it cannot own final TypeScript semantic truth.
- ComponentContract identity includes framework/profile and canonical component identity but remains queryable through common operations.
- Framework region coordinates are authored source coordinates with tagged encoding; generated coordinate identity is not accepted as primary.
- Only complete validated contribution snapshots admit and become visible to Check queries.
- Common and framework-owned diagnostics cannot share the same family/slice authority.

### Migration and cutover

- Start with Vue/Svelte canary regions and component contracts; broader vertical work belongs to generated NCF/VIM rows.
- Keep provider-generated TSX functioning during observation but prevent it from feeding native semantic facts.
- Delete old framework checker paths only after exact native/provider behavior and authored mapping are proven.

### Consumers and unlocks

- Unlocks NCK6 native publication for framework semantic families.
- Provides framework-authored targets/facts consumed by LSO2/LSO8 and future NCF slices.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **NCK5-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or reproduced defect that existing evidence does not discriminate.
- **NCK5-AC2 — positive contract:** the named API/data boundary preserves exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **NCK5-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm; otherwise bind a terse not-applicable rationale.
- **NCK5-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, provider, filesystem, network, allocation, copy, or retained-candidate work; otherwise bind a terse not-applicable rationale.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; do not add implementation mirrors, duplicate permutations, or universal test quotas.
- Test homes: `crates/verter_session/tests/cases`, `crates/verter_protocol/tests`, `packages/typescript-plugin/src`, and the exact generated vertical fixture selected by this node.


### Pack-specific proof obligations

- **NCK5-AC-NEUTRAL:** core checker modules contain no framework-name dispatch and contributors expose no resolver.
- **NCK5-AC-REGIONS:** Vue/Svelte canary template regions carry exact authored identity, scopes, and read sets.
- **NCK5-AC-CONTRACT:** common ComponentContract queries match vertical fixtures with provenance preserved.
- **NCK5-AC-ISOLATION:** profile changes, cancellation, and rapid edits publish no stale or mixed contributions.
- **NCK5-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **NCK5-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **NCK5-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **NCK5-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Deletions and forbidden designs

- Delete framework-specific native resolver/checker paths displaced by typed contributions.
- Delete duplicate framework component metadata authority after TIF1 contract migration.
- Delete generated-TSX-as-native-truth assumptions from live architecture.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

- Core `if framework == vue/svelte` branches.
- Adapters receiving raw ProjectSemanticDispatch or mutable semantic stores.
- Generated TypeScript/TSX text, regex, or source slicing as native semantic facts.
- Framework-specific copies of relation, call, flow, or diagnostic query stores.
- Cross-profile contribution or cache identity aliasing.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

- Delete or structurally reject every compatibility path that would preserve a second owner after cutover.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 800 production LOC, 8 production files, 2 related crates/packages.
- Mandatory rescope above 1500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, identity aliasing, or unauthorized executable work.
- Performance budget: when preflight identifies touched authority or a hot path, use the ratified replacement SLO and equivalent-work counters below; otherwise performance evidence is not applicable and no soak is invented solely to populate evidence.

- One parse and one shallow framework pass per content hash; no checker-triggered rescan.
- Contribution work is demand-selected and zero for non-participating capabilities.
- Target ceiling: 800 production LOC, 8 production files, and 2 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Abort conditions

- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. `node roadmap/0.1.0-tama/tools/validate-program-dag.mjs --strict`
2. Run every final command in the bound `targeted-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

### Pack-specific verification inventory

1. `cargo nextest run -p verter_language -p verter_semantic -p verter_session -p verter_vue_conformance -p verter_svelte_conformance`.
1. Static no-framework-switch/no-resolver adapter guards.
1. Vue/Svelte canary differential, profile-isolation, rapid-edit, and memory-plateau tests.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN evidence when applicable, the configured independent review profile, and the owning final gate on the squashed review candidate.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's predeclared row in `authority/state/implemented.toml` from `status = "pending"` to `status = "implemented"` with the planned squash commit message, approximate date with timezone, and optional pull-request number. The transitioned row is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.
