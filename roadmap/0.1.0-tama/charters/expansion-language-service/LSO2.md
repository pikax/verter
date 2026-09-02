<!-- unified-charter-v2
id=LSO2
name=Canonical authored target and provenance graph
predecessors=LSO0,IDX0,ENCL0,TIF1
phase=expansion
train=expansion.language-service
product=language_service
kind=implementation
semantic_role=delivery
class=successor
owner=expansion.language-service:one authored-coordinate semantic-operation and edit-transaction authority
conflict_domains=semantic_authority,mapping_geometry,source_lineage
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
charter=charters/expansion-language-service/LSO2.md
size=M
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# LSO2 — Canonical authored target and provenance graph

Readiness is derived only from implemented-ledger rows for the node ancestors. Commit message, approximate timezone-bearing date, and optional PR are loose locator hints; the CLI performs no Git, GitHub, SHA, tree, ancestry, receipt, lease, or digest validation.

## Independently acceptable outcome

Implement one canonical authored target and provenance graph used by every navigation, occurrence, rename, completion-resolve, and linked presentation operation. It normalizes native, framework, provider, alias/barrel, generated, and external-declaration discoveries into exact semantic targets.

The current owner is **same-file binding paths, provider result merges, barrel/default-export heuristics, current-file mapper fallbacks, and feature-specific target DTOs**. The final and sole owner is **one TargetGraph with stable semantic target identity, explicit derivation edges, authored anchors, and exact source/generated provenance validation**.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_semantic`, `crates/verter_session`, `crates/verter_identity`, `crates/verter_span`, `crates/verter_type_runtime`, `crates/verter_compiler`.
- Pack production inventory:
- `crates/verter_semantic` for target/edge identities and semantic anchors
- `crates/verter_session` for target graph construction over project snapshots
- `crates/verter_identity` and `crates/verter_span` for stable identities/ranges
- `crates/verter_type_runtime` adapters for provider target observations
- `crates/verter_compiler`/framework analysis for explicit component and generated anchors

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.
- Named API/data boundaries:
- `TargetGraph`, `AuthoredTargetId`, `AuthoredTarget`, and compact target/edge tables
- `TargetProvenance::{LiveSemantic, HostSource, GeneratedMapping, ExternalDeclaration, FrameworkContribution}`
- `TargetEdgeKind::{Declares, Aliases, Reexports, Implements, Overrides, Augments, ProjectsTo}`
- `ComponentAnchor`, `GeneratedSnapshotBasis`, and `SourceRevisionBasis`
- `TargetNormalizationResult::{Exact, Ambiguous, Unmappable, Stale, NeedInputs}`

## Exact predecessor contracts

- **LSO0:** implemented ledger row for “Authored-coordinate semantic operation constitution”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **IDX0:** implemented ledger row for “Atomic semantic contributions and workspace index”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **ENCL0:** implemented ledger row for “LSP and editor coordinate-boundary cutover”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **TIF1:** implemented ledger row for “TypeInfo-first ComponentInfo and component-meta cutover”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

### Binding architecture

- Target identity is semantic symbol/declaration identity plus exact profile/source ownership, not URI plus range.
- GeneratedMapping provenance is valid only when the exact generated snapshot used by the provider matches the mapper snapshot.
- Real source/external declarations validate with their own source revision/hash, never a generated compile snapshot.
- Barrels, aliases, default exports, augmentations, and framework components are explicit edges, not suffix or first-binding heuristics.
- Every target file obtains its own mapper, line index, snapshot, and analysis from the host; current-file fallback is forbidden.
- Ambiguity is preserved and sorted deterministically; arbitrary first target selection is forbidden.
- IDX0 supplies candidates; authoritative semantic resolution and edge construction remain downstream.

### Internal subblocks

#### LSO2-SB1 - Target identity and compact graph storage

**Independently testable outcome:** Targets and derivation edges have stable IDs, deterministic storage, and no feature-specific flags.

**Architecture:**

- Define target node/edge taxonomy and structural identity.
- Use compact tables and side data for spans/provenance.
- Separate semantic target identity from rendered source location.

**Expected changes:**

- Add shared target graph crate/module and serializers for debug/conformance only.
- Replace feature-local target enums as each consumer migrates.

**Discriminating proof:**

- Insertion/reorder does not perturb stable IDs.
- Distinct overload/member/component targets sharing a span remain distinct.

#### LSO2-SB2 - Explicit authored anchors

**Independently testable outcome:** Every target kind has a non-fallback authored anchor owned by its source/framework analysis.

**Architecture:**

- Define declaration/name/export/default/component/template/external anchors.
- Require Vue/Svelte component analyses to publish canonical component anchors.
- Use FileStart only when explicitly recorded for truly empty carriers.

**Expected changes:**

- Add anchor producers to framework/native analysis.
- Delete find-first-binding/default-range heuristics after parity.

**Discriminating proof:**

- Script-setup, explicit default export, defineOptions/name, template-only, and named export fixtures land exactly.
- Missing anchors yield typed NeedInputs/unmappable, never 0:0.

#### LSO2-SB3 - Provider/generated target normalization

**Independently testable outcome:** Provider results normalize only through exact snapshot-matched mapping and target-file context.

**Architecture:**

- Canonicalize provider paths and identify generated versus real/external source.
- Validate provider epoch and generated snapshot basis.
- Load target-file mapper/analysis through host and drop stale/unmappable ranges.

**Expected changes:**

- Implement one provider observation adapter into TargetGraph.
- Remove current-file mapper and virtual-file special cases.

**Discriminating proof:**

- Stale mapper mutation is rejected.
- Cross-file generated targets map through the target file, not current file.

#### LSO2-SB4 - Alias, barrel, augmentation, and framework edges

**Independently testable outcome:** Target chains are explicit, cycle-safe, and terminalize according to operation policy later.

**Architecture:**

- Represent import aliases, export aliases, star/default barrels, module/global augmentation, override/implementation, and component tag-to-declaration links.
- Preserve every hop provenance and detect cycles.
- Do not eagerly collapse chains in storage.

**Expected changes:**

- Build edges from authoritative semantic/index facts.
- Characterize legacy barrel and component navigation before deletion.

**Discriminating proof:**

- Default/named/star reexport chains resolve deterministically.
- Cycles return typed cycle/ambiguous results without loops.

#### LSO2-SB5 - Canonical target deduplication and ordering

**Independently testable outcome:** Multiple observations of one semantic target collapse while legitimate alternatives remain visible.

**Architecture:**

- Dedup by target identity and normalized provenance, not filename suffix or range alone.
- Prefer authored canonical representation over generated representation of the same target.
- Retain external declaration when it is the true terminal or no source exists.

**Expected changes:**

- Centralize normalization/dedup ordering.
- Delete “prefer non-.vue” and similar suffix rules.

**Discriminating proof:**

- Permutation/property tests yield byte-identical target sets.
- Canonical component target is never dropped merely because a .d.ts/generated result exists.

#### LSO2-SB6 - Incremental graph invalidation and bounded discovery

**Independently testable outcome:** Target graph updates exactly for changed declarations/edges and does not crawl the workspace for leaf queries.

**Architecture:**

- Version nodes/edges by source/profile/resolve/lib/index read sets.
- Use IDX0 bounded candidates and demand-select edge construction.
- Do not negative-cache incomplete enumeration or exhausted budgets.

**Expected changes:**

- Add per-family caches/singleflight and PER0 counters.
- Release target subgraphs on project/profile teardown.

**Discriminating proof:**

- Edit-one-file incremental graph equals fresh graph.
- Warm leaf navigation performs zero unrelated file scans and retained memory plateaus.

### Identity, invalidation, and publication

- Core operation identity is rooted in project/profile/source basis, semantic subject, operation kind, demand, and capability epoch; presentation encoding is not semantic identity.
- Generated coordinates, provider handles, and editor DTOs may exist only inside typed adapters and never become public semantic truth.
- Cancellation, stale, superseded, partial, NeedInputs, unsupported, and ambiguous outcomes remain distinct and are never collapsed to empty success.
- All returned targets and edits carry exact authored provenance and validate the snapshot/mapping chain used to derive them.
- Target graph storage contains no LSP positions, provider JSON, presentation text, or raw workspace edits.
- A target range is renderable only with matching source/generated basis.
- Deduplication never hides ambiguity between different semantic targets.

### Migration and cutover

- Introduce graph behind existing feature characterization and normalize same-file native targets first.
- Migrate provider/generated/component/barrel/external targets incrementally.
- Delete old target merge/heuristic paths only after all opened consumers use LSO2.

### Consumers and unlocks

- Unlocks LSO3-LSO7.
- Provides shared target/provenance to rename, completion resolve, diagnostics related locations, and future operations.
- Owns deletion of legacy target heuristics.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **LSO2-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or reproduced defect that existing evidence does not discriminate.
- **LSO2-AC2 — positive contract:** the named API/data boundary preserves exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **LSO2-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm; otherwise bind a terse not-applicable rationale.
- **LSO2-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, provider, filesystem, network, allocation, copy, or retained-candidate work; otherwise bind a terse not-applicable rationale.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; do not add implementation mirrors, duplicate permutations, or universal test quotas.
- Test homes: `crates/verter_session/tests/cases`, `crates/verter_protocol/tests`, `packages/typescript-plugin/src`, and the exact generated vertical fixture selected by this node.


### Pack-specific proof obligations

- **LSO2-AC-TARGET-ID:** semantic target IDs/dedup survive input order and representation changes.
- **LSO2-AC-SNAPSHOT:** generated targets require exact provider/mapper snapshot equality.
- **LSO2-AC-ANCHOR:** every component/real/external target has an explicit authored anchor or typed refusal.
- **LSO2-AC-CYCLE:** alias/barrel/augmentation cycles terminate deterministically.
- **LSO2-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **LSO2-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **LSO2-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **LSO2-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Deletions and forbidden designs

- Delete current-file mapper fallback for cross-file targets.
- Delete Range::default/0:0 target construction, default-export first-binding heuristics, suffix-preference dedup, and virtual-file special branches.
- Delete feature-specific target identity enums displaced by the graph.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

- URI/range-only target identity.
- Nearest-token or column-delta mapping through synthetic content.
- Eager workspace target graph construction for a leaf query.
- Index storage answering semantic target resolution.
- Feature-specific target graph forks.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

- Delete or structurally reject every compatibility path that would preserve a second owner after cutover.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 800 production LOC, 8 production files, 2 related crates/packages.
- Mandatory rescope above 1500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, identity aliasing, or unauthorized executable work.
- Performance budget: when preflight identifies touched authority or a hot path, use the ratified replacement SLO and equivalent-work counters below; otherwise performance evidence is not applicable and no soak is invented solely to populate evidence.

- Target graph materialization is demand-sliced; warm leaf queries perform zero unrelated candidate enumeration.
- Target ceiling: 800 production LOC, 8 production files, and 2 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Abort conditions

- Abort if a target kind cannot name an authoritative semantic identity and exact authored anchor.
- Abort if preserving legacy output requires suffix preference or approximate mapping.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. `node roadmap/0.1.0-tama/tools/validate-program-dag.mjs --strict`
2. Run every final command in the bound `targeted-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

### Pack-specific verification inventory

1. Target identity/dedup/order properties; cross-file mapper/snapshot mutation tests.
1. Vue/Svelte/default/named/barrel/external declaration navigation fixtures.
1. Incremental/fresh, cycle, cancellation, budget, allocation, and memory plateau tests.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN evidence when applicable, the configured independent review profile, and the owning final gate on the squashed review candidate.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's predeclared row in `authority/state/implemented.toml` from `status = "pending"` to `status = "implemented"` with the planned squash commit message, approximate date with timezone, and optional pull-request number. The transitioned row is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.
