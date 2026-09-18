<!-- unified-charter-v2
id=WSP0
name=Issue 93 investigation and interactive SLO constitution
predecessors=ORC0,MEM0
phase=expansion
train=expansion.workspace-responsiveness
product=wsp_product
kind=constitution
semantic_role=delivery
class=successor
owner=expansion.workspace-responsiveness:Large-workspace responsiveness and Lapce hang elimination
conflict_domains=lsp_publication,performance_evidence,scheduler_admission
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
charter=charters/expansion-workspace-responsiveness/WSP0.md
size=M
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=600
rescope_files=6
rescope_unrelated_packages=3
-->

# WSP0 - Issue 93 investigation and interactive SLO constitution

## Independently acceptable outcome

Define reproducible large-workspace workloads and predeclared budgets while treating excessive LSP traffic as an unconfirmed hypothesis.

This is a contract/inventory outcome. No production implementation belongs in this node.

## Binding architecture and responsibility

Read `contracts/product-experience.md` and this train's plan in `plans/expansion-workspace-responsiveness.md`. All current source identity, semantic authority, TypeScript projection, mapping, lifetime and authored-edit contracts remain binding. This charter adds consumer/execution requirements; it is not permission to weaken existing Vue/Svelte or web-language support. Breaking changes are allowed with explicit migration and one final owner.

- Production surfaces: `crates/verter_lsp/src`, `crates/verter_session/src`, `packages/lsp-test-client`, `packages/dx-harness`
- Placement notes: crates/verter_lsp/src/; crates/verter_session/src/; packages/lsp-test-client/; packages/dx-harness/. These identify ownership and intended new modules, not a claim that every listed subdirectory already exists. Verify exact paths and symbols on the implementation candidate before mutation.
- Final owner: `expansion.workspace-responsiveness` for this product outcome; semantic algorithms, project resolution, format/lint logic, TypeScript projection and edit transactions stay with their existing producing owners.
- Conflict domains: `lsp_publication`, `performance_evidence`, `scheduler_admission`. Register narrow real roots before parallel mutation; no new orchestration system is introduced.

## Exact predecessors and received outcomes

- **ORC0**: Trusted implementation-ledger cutover. Consume its accepted outcome; implementation state is resolved by the existing PM/ledger, not inferred from this document.
- **MEM0**: Aggregate semantic memory budget and workload contract. Consume its accepted outcome; implementation state is resolved by the existing PM/ledger, not inferred from this document.

These are contract dependencies, not resource-capacity locks. Do not wait for a whole UI product where an accepted shared producer is sufficient. A discovered missing producer/API is a receiving-owner amendment before code, not a local substitute or an unrecorded dependency.

## Interfaces and delivered products

- `WorkspaceResponsivenessContract and reference-machine manifests`
- `Issue93EvidencePlan with client/server/provider timelines`
- `InteractiveSloCatalog with correctness and equivalent-work denominators`

The names above identify the contracts to produce or adapt, not assumed existing APIs. Reuse the canonical outcome, identity and capability vocabulary instead of introducing synonymous types. Each exposed product binds source/project/configuration/profile, execution host and engine version, its completeness state and its invalidation/lifetime rules.

## Scoped implementation sequence

### WSP0.1

Capture original issue facts: opened 25 July 2026, large codebase, suspected excess LSP information, no root cause proved. Record actual Lapce/server/plugin/provider versions and enabled features before reproducing.

### WSP0.2

Cover 1k/10k/50k authored-source projects, monorepo references, a 1 MiB source file, high diagnostic density, Unicode/CRLF, ignored dependency trees, edit storms and a slow-reading client. Use real pinned projects as well as adversarial synthetic cases.

### WSP0.3

Measure UI responsiveness independently of server response time, total process-tree RSS separately from language-service incremental RSS, bytes/messages, decode/apply time, worker queues, provider work and cancellation waste. Pin candidate targets in the shared contract before optimization.

At preflight, map each step to the smallest owning source/test files and identify the existing characterisation it preserves. Record only meaningful scope or placement amendments. Setup, documentation and configuration belong with the outcome that needs them; independent semantic algorithms or platform subsystems need separate nodes.

## Snapshot, cancellation and boundary rules

Use the canonical immutable observation basis. Reject old-source, old-project, old-engine, old-worker and wrong-result-handle outcomes; never publish stale results as current. Cancellation prevents additional irrelevant work and releases retained handles. Partial, pending, unsupported, ambiguous and failed are distinct from complete-empty. Cross-file or browser/native edits are authored, version-checked and atomic through the existing transaction authority.

For UI/adapter work, preserve semantic meaning under reduced client capabilities. Rich information is fetched on demand; closed/disabled views create no background semantic work. A protocol smoke or screenshot is not real-editor responsiveness or semantic correctness proof. For contract-only nodes, specify these obligations without constructing a speculative implementation.

## Acceptance and discriminating proof

**WSP0-AC1.** A fast server paired with a frozen client is a failure.

**WSP0-AC2.** A smaller feature set, hidden error truncation or reduced project membership cannot count as a performance improvement.

**WSP0-AC3.** Unreproduced original issue is labelled unverified, not fixed.

**WSP0-AC-OWNER.** Prove one final owner and the stated retirement obligation. Reuse/extend existing coverage; add negative tests only for a plausible wrong-complete, stale, unsafe or duplicated-authority failure.

**WSP0-AC-BASIS.** Where this outcome changes caching, mapping, snapshots, result handles or incremental publication, compare incremental and fresh execution on the same basis. For a pure contract/inventory outcome, bind the corresponding downstream test owner instead of adding a meaningless runtime test.

**WSP0-AC-RESOURCE.** Where hot paths or UI are touched, use WSP's equal-work corpus and the limits in the shared contract, with all required features enabled. Include actual client application/paint for a real-client claim, provider process costs, outbound bytes and retained memory. Explicitly label unavailable metrics. No general performance claim follows from a compiler microbenchmark.

**WSP0-AC-EXPOSURE.** Every supported operation introduced here supplies its DX1 registration, executable playground adapter and replay case. Before the explorer is available, register the schema/case and keep product promotion blocked on executable exposure. Core implementation is not forced to wait for PG17; a generic typed explorer view is sufficient. Native-only functionality requires real explicitly selected native execution, not an inactive card.

## Migration, deletions and forbidden routes

Replace anecdotal responsiveness claims; retain all raw failed measurements.

Characterise the existing user-visible feature first, route it through its surviving producer, verify consumers, then remove the named obsolete route. Do not remove useful existing behavior merely because the new presentation is not ready. Inert/dormant adapters may land before the owning activation boundary, but a cutover cannot retain two active semantic authorities.

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

Constitution with real measurement-manifest authoring; docs gate.

Sizing: size M, 0 production LOC across 0 files, rescope at 600/6; efforts implementation high, review high, verification low, confirmation medium. Effective predecessors: ORC0, MEM0. The delta proposed one uniform budget (M, 800 LOC, 8 files, every effort high) for all 115 nodes; this review sized each node from its actual owned outcome.
