<!-- unified-charter-v2
id=LSO2
name=Canonical authored target and provenance graph
phase=expansion
train=expansion.language-service
product=language_service
kind=implementation
semantic_role=delivery
class=successor
predecessors=LSO0,IDX0,ENCL0,TIF1
conditional_predecessors=
owner=expansion.language-service:one authored-coordinate semantic-operation and edit-transaction authority
conflict_domains=semantic_authority,mapping_geometry,source_lineage
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
size=M
dispatchable=true
optional=false
release_gating=none
source_refs=live:docs/arch/refactor/rev11/sources/legacy-arch-reconciliation.md
external_requirements=
activation_gate=ORC0
charter=charters/expansion-language-service/LSO2.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# LSO2 - Canonical authored target and provenance graph

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

The internal subblocks below are binding decomposition and review checkpoints. They do not receive independent dispatch, leases, receipts, or deletion ownership unless the pre-scope architect proves that one subblock is independently acceptable or the block crosses a mandatory rescope trigger. In that case, amend the DAG before production mutation rather than treating a train-sized subblock as an implementation checklist.

## Independently acceptable outcome

Implement one canonical authored target and provenance graph used by every navigation, occurrence, rename, completion-resolve, and linked presentation operation. It normalizes native, framework, provider, alias/barrel, generated, and external-declaration discoveries into exact semantic targets.

The current owner is **same-file binding paths, provider result merges, barrel/default-export heuristics, current-file mapper fallbacks, and feature-specific target DTOs**. The final and sole owner is **one TargetGraph with stable semantic target identity, explicit derivation edges, authored anchors, and exact source/generated provenance validation**.

## Architectural role and end state

LSO2 is the shared navigation substrate. It does not decide feature-specific traversal policy; it makes all candidate targets comparable and renderable without rediscovering symbol identity or guessing source ranges.

## Expected production surfaces

- `crates/verter_semantic` for target/edge identities and semantic anchors
- `crates/verter_session` for target graph construction over project snapshots
- `crates/verter_identity` and `crates/verter_span` for stable identities/ranges
- `crates/verter_type_runtime` adapters for provider target observations
- `crates/verter_compiler`/framework analysis for explicit component and generated anchors

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.

## Named APIs and data boundaries

- `TargetGraph`, `AuthoredTargetId`, `AuthoredTarget`, and compact target/edge tables
- `TargetProvenance::{LiveSemantic, HostSource, GeneratedMapping, ExternalDeclaration, FrameworkContribution}`
- `TargetEdgeKind::{Declares, Aliases, Reexports, Implements, Overrides, Augments, ProjectsTo}`
- `ComponentAnchor`, `GeneratedSnapshotBasis`, and `SourceRevisionBasis`
- `TargetNormalizationResult::{Exact, Ambiguous, Unmappable, Stale, NeedInputs}`

## Exact predecessor contracts

- **LSO0:** consume canonical target/provenance and authored-coordinate constitution.
- **IDX0:** consume bounded candidate discovery without granting indexes semantic authority.
- **ENCL0:** consume LSP/editor coordinate boundary cutover and strict mapping.
- **TIF1:** consume TypeInfo-first component and public semantic surface identity.

External custody: none beyond the package activation boundary.

## Binding architecture

- Target identity is semantic symbol/declaration identity plus exact profile/source ownership, not URI plus range.
- GeneratedMapping provenance is valid only when the exact generated snapshot used by the provider matches the mapper snapshot.
- Real source/external declarations validate with their own source revision/hash, never a generated compile snapshot.
- Barrels, aliases, default exports, augmentations, and framework components are explicit edges, not suffix or first-binding heuristics.
- Every target file obtains its own mapper, line index, snapshot, and analysis from the host; current-file fallback is forbidden.
- Ambiguity is preserved and sorted deterministically; arbitrary first target selection is forbidden.
- IDX0 supplies candidates; authoritative semantic resolution and edge construction remain downstream.

## Internal subblocks

### LSO2-SB1 - Target identity and compact graph storage

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

### LSO2-SB2 - Explicit authored anchors

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

### LSO2-SB3 - Provider/generated target normalization

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

### LSO2-SB4 - Alias, barrel, augmentation, and framework edges

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

### LSO2-SB5 - Canonical target deduplication and ordering

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

### LSO2-SB6 - Incremental graph invalidation and bounded discovery

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

## Data, identity, invalidation, and publication laws

- Core operation identity is rooted in project/profile/source basis, semantic subject, operation kind, demand, and capability epoch; presentation encoding is not semantic identity.
- Generated coordinates, provider handles, and editor DTOs may exist only inside typed adapters and never become public semantic truth.
- Cancellation, stale, superseded, partial, NeedInputs, unsupported, and ambiguous outcomes remain distinct and are never collapsed to empty success.
- All returned targets and edits carry exact authored provenance and validate the snapshot/mapping chain used to derive them.
- Target graph storage contains no LSP positions, provider JSON, presentation text, or raw workspace edits.
- A target range is renderable only with matching source/generated basis.
- Deduplication never hides ambiguity between different semantic targets.

## Migration and cutover

- Introduce graph behind existing feature characterization and normalize same-file native targets first.
- Migrate provider/generated/component/barrel/external targets incrementally.
- Delete old target merge/heuristic paths only after all opened consumers use LSO2.

## Deletions

- Delete current-file mapper fallback for cross-file targets.
- Delete Range::default/0:0 target construction, default-export first-binding heuristics, suffix-preference dedup, and virtual-file special branches.
- Delete feature-specific target identity enums displaced by the graph.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

## Forbidden designs

- URI/range-only target identity.
- Nearest-token or column-delta mapping through synthetic content.
- Eager workspace target graph construction for a leaf query.
- Index storage answering semantic target resolution.
- Feature-specific target graph forks.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

## Acceptance IDs and discriminating proof

- **LSO2-AC-TARGET-ID:** semantic target IDs/dedup survive input order and representation changes.
- **LSO2-AC-SNAPSHOT:** generated targets require exact provider/mapper snapshot equality.
- **LSO2-AC-ANCHOR:** every component/real/external target has an explicit authored anchor or typed refusal.
- **LSO2-AC-CYCLE:** alias/barrel/augmentation cycles terminate deterministically.
- **LSO2-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **LSO2-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **LSO2-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **LSO2-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Performance and bounded work

- Target graph materialization is demand-sliced; warm leaf queries perform zero unrelated candidate enumeration.
- Target ceiling: 800 production LOC, 8 production files, and 2 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Mandatory rescope and abort conditions

- Abort if a target kind cannot name an authoritative semantic identity and exact authored anchor.
- Abort if preserving legacy output requires suffix preference or approximate mapping.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. Target identity/dedup/order properties; cross-file mapper/snapshot mutation tests.
1. Vue/Svelte/default/named/barrel/external declaration navigation fixtures.
1. Incremental/fresh, cycle, cancellation, budget, allocation, and memory plateau tests.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Consumers and unlocks

- Unlocks LSO3-LSO7.
- Provides shared target/provenance to rename, completion resolve, diagnostics related locations, and future operations.
- Owns deletion of legacy target heuristics.

## Source reconciliation

- `docs/arch/goto-definition-architecture-decision.md`.
- Path-precise resolution and source-map/mapping designs classified by reconciliation.

Durable clauses are transferred as digest-bound requirement atoms. Historical path archaeology, obsolete branches, and implementation journals are not copied into the charter. Git history remains the archive.
