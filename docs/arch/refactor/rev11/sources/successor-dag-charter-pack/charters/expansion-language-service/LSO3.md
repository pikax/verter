<!-- unified-charter-v2
id=LSO3
name=Definition, type-definition, implementation, and symbol navigation
phase=expansion
train=expansion.language-service
product=language_service
kind=implementation
semantic_role=delivery
class=successor
predecessors=LSO2
conditional_predecessors=
owner=expansion.language-service:one authored-coordinate semantic-operation and edit-transaction authority
conflict_domains=semantic_authority,mapping_geometry,lsp_publication
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
charter=charters/expansion-language-service/LSO3.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# LSO3 - Definition, type-definition, implementation, and symbol navigation

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

The internal subblocks below are binding decomposition and review checkpoints. They do not receive independent dispatch, leases, receipts, or deletion ownership unless the pre-scope architect proves that one subblock is independently acceptable or the block crosses a mandatory rescope trigger. In that case, amend the DAG before production mutation rather than treating a train-sized subblock as an implementation checklist.

## Independently acceptable outcome

Implement one navigation executor for definition, type-definition, implementation, and document/workspace symbol targets over LSO2. Each operation declares edge traversal and terminalization policy while sharing candidate classification, target normalization, authored rendering, ambiguity, and cancellation.

The current owner is **separate native/provider handlers, early-return arbitration, virtual-file branches, barrel heuristics, and feature-specific result rendering**. The final and sole owner is **one NavigationEngine over TargetGraph with explicit per-operation traversal policy and exact authored target results**.

## Architectural role and end state

LSO3 turns the shared target graph into user-facing navigation without reintroducing multiple semantic engines. It keeps operation differences declarative: type definition follows type-declaration edges, implementation follows implementation/override edges, while all use one target identity and rendering path.

## Expected production surfaces

- `crates/verter_session` language-service navigation coordinator
- `crates/verter_semantic` navigation policy over target edges
- `crates/verter_lsp` thin Location/LocationLink adapter
- `crates/verter_protocol` operation results through PUB0
- `crates/verter_bench`/VIM fixtures for navigation conformance

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.

## Named APIs and data boundaries

- `NavigationRequest`, `NavigationKind`, and `NavigationPolicy`
- `DefinitionQuery`/semantic subject classification without LSP coordinates
- `NavigationResult { targets, basis, completeness }`
- `TargetTraversalBudget`, `TargetCycle`, and `NavigationAmbiguity`
- `AuthoredLocationLink` with origin/target anchors

## Exact predecessor contracts

- **LSO2:** consume exact canonical target graph, provenance, deduplication, and authored anchors.

External custody: none beyond the package activation boundary.

## Binding architecture

- All navigation kinds call one engine and one target renderer.
- Same-file, cross-file, framework component, barrel, external declaration, and provider observations differ only in target graph provenance/edges.
- Operation policy explicitly chooses alias terminalization, type/value namespace, implementation/override traversal, and whether intermediates are returned.
- A native result does not suppress provider candidates merely because it is non-empty; normalized semantic targets are composed under policy.
- Missing target compilation/mapping/input yields typed incomplete/NeedInputs, not fabricated locations.
- Target ordering and ambiguity are deterministic and independent of provider response order.

## Internal subblocks

### LSO3-SB1 - Authored position classification

**Independently testable outcome:** An authored query position becomes one typed semantic subject or explicit ambiguity/unmapped result.

**Architecture:**

- Classify declaration/name/tag/attribute/expression/import/export and embedded regions from authored analysis.
- Use exact source unit/profile/revision.
- Do not map synthetic positions to nearest authored token.

**Expected changes:**

- Create shared query classifier consumed by every navigation kind.
- Delete handler-local token/span heuristics.

**Discriminating proof:**

- All letter columns of mapped identifiers classify identically; synthetic prefixes classify unmapped.
- UTF/CRLF/emoji boundary tests are exact.

### LSO3-SB2 - Declarative traversal policies

**Independently testable outcome:** Definition/type-definition/implementation differences are explicit policy data over the same graph.

**Architecture:**

- Define allowed edge kinds, alias behavior, namespace, terminal kinds, cycle and budget behavior.
- Keep policy versioned and generated into conformance rows.
- Preserve ambiguous alternatives.

**Expected changes:**

- Implement policy interpreter and static policy table.
- Remove forked feature traversals.

**Discriminating proof:**

- Planting a wrong edge permission changes only that operation and is detected.
- Policy table covers every NavigationKind exactly once.

### LSO3-SB3 - Definition and component navigation

**Independently testable outcome:** Definitions terminalize to exact authored declarations/components/external declarations.

**Architecture:**

- Follow aliases/barrels to canonical declaration according to policy.
- Use explicit component anchors and named export spans.
- Keep .d.ts when it is the real terminal.

**Expected changes:**

- Migrate same-file/native, provider, component tags/imports, and barrels.
- Delete old merge arbitration after parity.

**Discriminating proof:**

- Vue/Svelte default/named/template-only/barrel cases land exactly.
- No .vue target is dropped by suffix preference.

### LSO3-SB4 - Type-definition and implementation navigation

**Independently testable outcome:** Type and implementation targets use correct namespace/edge semantics and preserve overload/override alternatives.

**Architecture:**

- Traverse type declaration, alias, implements, override, and framework contract edges.
- Return multiple legitimate implementations deterministically.
- Use bounded project candidates from IDX0 through LSO2.

**Expected changes:**

- Replace provider-only and native-only forked paths.
- Add class/interface/component implementation fixtures.

**Discriminating proof:**

- Type/value namespace mutation tests discriminate the operations.
- Hierarchy cycles/budgets return typed partial/refusal and never hang.

### LSO3-SB5 - Authored rendering and protocol adaptation

**Independently testable outcome:** All targets render through their own source snapshot and exact negotiated boundary encoding.

**Architecture:**

- Resolve source snapshot/line index per target.
- Emit authored links with origin/selection/target ranges.
- Drop stale targets and preserve completeness truth.

**Expected changes:**

- Centralize renderer before LSP adapter.
- Delete target-file/current-file mapper confusion and Range::default fallbacks.

**Discriminating proof:**

- Cross-file target line/column is computed from target source.
- Stale target edits between resolution/render are rejected.

### LSO3-SB6 - Navigation conformance and bounded work

**Independently testable outcome:** Every opened framework/provider/profile topology has equivalent targets and bounded work.

**Architecture:**

- Generate vertical matrix for provider on/off, recovery, barrels, components, external declarations, and coexistence.
- Count candidate enumeration, target nodes/edges, provider calls, mappings, allocations.
- Require zero unrelated workspace scanning.

**Expected changes:**

- Add VIM rows and PER0 counters.
- Use deterministic hermetic fixtures plus gated real-provider canaries.

**Discriminating proof:**

- Incremental equals fresh and provider order permutations yield same targets.
- Warm same query performs zero parse/index rebuild and bounded allocations.

## Data, identity, invalidation, and publication laws

- Core operation identity is rooted in project/profile/source basis, semantic subject, operation kind, demand, and capability epoch; presentation encoding is not semantic identity.
- Generated coordinates, provider handles, and editor DTOs may exist only inside typed adapters and never become public semantic truth.
- Cancellation, stale, superseded, partial, NeedInputs, unsupported, and ambiguous outcomes remain distinct and are never collapsed to empty success.
- All returned targets and edits carry exact authored provenance and validate the snapshot/mapping chain used to derive them.
- Navigation results contain semantic target IDs plus authored anchors; LSP Locations are serialization only.
- A stale target is removed with completeness updated, never silently mapped against a newer file.
- Cycle/budget outcomes cannot enter complete-result caches.

## Migration and cutover

- Migrate definition first through LSO2, then type-definition and implementation.
- Route document/workspace symbol targets through the same authored renderer where semantically applicable.
- Delete feature-specific target merges after all opened navigation kinds pass conformance.

## Deletions

- Delete cross-file early returns, suffix-based dedup, current-file mapper fallback, Range::default navigation construction, and virtual-file special cases.
- Delete separate definition/type-definition/implementation target renderers.
- Delete provider/native first-nonempty arbitration.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

## Forbidden designs

- A second target graph or feature-specific source-map interpolation.
- Returning generated TSX/virtual paths as canonical targets.
- Dropping ambiguity by arbitrary first result.
- Unbounded barrel/hierarchy traversal.
- Surface-specific semantic target DTOs.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

## Acceptance IDs and discriminating proof

- **LSO3-AC-ONE-ENGINE:** all navigation kinds call one classifier, traversal engine, target graph, and renderer.
- **LSO3-AC-POLICY:** exact edge/namespace policies are generated and mutation-tested.
- **LSO3-AC-AUTHORED:** every location is rendered from the target source snapshot with no fallback.
- **LSO3-AC-PARITY:** framework/provider/recovery/coexistence matrix yields exact expected semantic target sets.
- **LSO3-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **LSO3-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **LSO3-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **LSO3-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Performance and bounded work

- Warm leaf navigation has bounded target/candidate work and zero unrelated parse/index/compile work.
- Target ceiling: 800 production LOC, 8 production files, and 2 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Mandatory rescope and abort conditions

- Abort if one navigation kind requires a separate target identity or mapper.
- Abort if an expected target cannot be represented without approximate source anchoring.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. Navigation policy mutation matrix and no-bypass architecture guard.
1. Hermetic Vue/Svelte/native/barrel/external/override/type-value fixtures plus gated providers.
1. Incremental/fresh, stale snapshot, cycle/budget, cancellation, allocation, and latency tests.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Consumers and unlocks

- Feeds LSO9 conformance and thin LSP navigation adapters.
- Provides target links for hover/signature/diagnostics where applicable.
- Owns deletion of navigation legacy routes.

## Source reconciliation

- `docs/arch/goto-definition-architecture-decision.md`.
- Global component and path-precise navigation clauses.

Durable clauses are transferred as digest-bound requirement atoms. Historical path archaeology, obsolete branches, and implementation journals are not copied into the charter. Git history remains the archive.
