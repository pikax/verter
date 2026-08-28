<!-- unified-charter-v2
id=NCK1
name=Executable-region and typed semantic-contribution contract
phase=expansion
train=expansion.native-checker
product=native_checker
kind=contract
semantic_role=delivery
class=successor
predecessors=NCK0,UAI0,PAR0,IDX0
conditional_predecessors=
owner=expansion.native-checker:sole native semantic diagnostic authority and certified-family cutover
conflict_domains=semantic_authority,carrier_parser,source_lineage
resource_class=docs-light
review_profile=architecture-3
gate_profile=docs-domain
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
charter=charters/expansion-native-checker/NCK1.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCK1 - Executable-region and typed semantic-contribution contract

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

The internal subblocks below are binding decomposition and review checkpoints. They do not receive independent dispatch, leases, receipts, or deletion ownership unless the pre-scope architect proves that one subblock is independently acceptable or the block crosses a mandatory rescope trigger. In that case, amend the DAG before production mutation rather than treating a train-sized subblock as an implementation checklist.

## Independently acceptable outcome

Specify the framework-neutral executable-region graph and typed semantic-contribution ingress that the checker will consume, while preserving current semantic ownership and preventing adapters or indexes from becoming resolvers.

The current owner is **function-only flow structures, parser-specific body identities, framework-specific template analysis, and informal ProgramAnalysisContributor seams**. The final and sole owner is **one validated ExecutableRegionGraph identity model and one typed SemanticContributionBatch ingress consumed by the existing semantic graph and checker**.

## Architectural role and end state

NCK1 generalizes the function flow substrate into executable regions without rebuilding flow or inventing a framework checker. It defines stable region identity, sparse region topology, contribution provenance, validation, and the boundary between discovery/indexing and authoritative resolution.

## Expected production surfaces

- `crates/verter_identity/src` and `crates/verter_language/src` for region/profile identities
- `crates/verter_parser` and framework parser outputs for region discovery descriptors
- `crates/verter_semantic` for region graph and typed contribution contracts
- `crates/verter_session` for validated contribution ingestion and project-scoped snapshots
- `crates/verter_protocol` only where PUB0 exposes region/provenance diagnostics, not internal graph internals

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.

## Named APIs and data boundaries

- `ExecutableRegionId`, `ExecutableRegionKind`, `ExecutableRegionGraph`, and `ExecutableRegionSnapshot`
- `RegionStableHash`, `RegionRevision`, and explicit parent/owner source identities
- `SemanticContributionBatch` and closed typed `SemanticContribution` arms
- `ContributionProvenance`, `ContributionReadSet`, `ContributionValidation`
- `FrameworkRegionDescriptor` and `SemanticContributor` capability contract
- `ComponentContract` as a framework-neutral semantic contribution, not a checker-specific side table

## Exact predecessor contracts

- **NCK0:** consume the diagnostic ownership and no-second-resolver constitution.
- **UAI0:** consume exact identity, carrier, parser, and coordinate contracts.
- **PAR0:** consume parser ownership and lineage; region discovery may not create a second parser.
- **IDX0:** consume atomic semantic contributions and bounded workspace indexes while preserving the rule that indexes are not semantic authority.

External custody: none beyond the package activation boundary.

## Binding architecture

- A function is one ExecutableRegionKind, not the definition of an executable region.
- Region discovery is syntax/lowering work; type and diagnostic meaning are resolved later by the one semantic engine.
- Region nodes are compact and structural. Types, diagnostics, effects, and target-specific presentation live in side tables or query results.
- Stable identity is content-derived from semantic body structure and source lineage, cosmetic-insensitive where safe, and never a raw source offset alone.
- Contributors emit typed facts and demands. They cannot receive ProjectSemanticDispatch, raw resolver internals, or a callback that resolves types privately.
- Every contribution carries profile, source, environment, provenance, dependency read set, and validation status.
- IDX0 may index contribution identities and candidates but may not answer checker semantics.

## Internal subblocks

### NCK1-SB1 - Region identity and taxonomy

**Independently testable outcome:** Every executable body kind has a stable, non-colliding identity and explicit owner.

**Architecture:**

- Define ModuleTopLevel, Function, StaticBlock, FieldInitializer, ParameterInitializer, DecoratorExpression, TopLevelAwait, and FrameworkRegion kinds.
- Separate region identity from transient arena index and source offset.
- Define parent region, declaration owner, source unit, profile, and body stable hash components.

**Expected changes:**

- Add contract types and schema/catalog rows; no live builders in this contract block.
- Map existing FunctionFlowGraph identities to the Function region compatibility proof, then retire function-only naming in later implementation.

**Discriminating proof:**

- Collision/property tests over reordered declarations, cosmetic edits, and same-offset different source units.
- Exact identity changes on semantically relevant body edits and remains stable on approved cosmetic edits.

### NCK1-SB2 - Sparse executable-region graph shape

**Independently testable outcome:** The graph represents control and dependency structure without embedding types or per-target state.

**Architecture:**

- Define compact node/edge tables, entry/exit anchors, child regions, captures, declaration dependencies, and source anchors.
- Reuse D8 flow slices for function control flow rather than creating a second CFG.
- Permit demand-sliced materialization; whole-region construction is not required for leaf queries.

**Expected changes:**

- Specify logical graph and future compact storage layout.
- Define which edge classes are parser/lowering facts versus semantic facts.

**Discriminating proof:**

- Taxonomy guard rejects type nodes, diagnostics, provider handles, or per-feature Vec/String payloads in structural nodes.
- A function-region projection must reproduce the existing accepted flow identity and topology facts.

### NCK1-SB3 - Typed contribution vocabulary

**Independently testable outcome:** Framework and language contributors can add declarations, bindings, contexts, relations, regions, and component contracts without source synthesis.

**Architecture:**

- Define a closed initial enum with versioned extension law.
- Keep semantic values typed: InjectedDeclaration, ExecutableRegion, Binding, NarrowingFact, ContextualType, RelationDemand, ComponentContract, and DiagnosticRuleDescriptor.
- Distinguish contributed facts from declarative demands that the executor must resolve.

**Expected changes:**

- Add schema/source atoms and exact ownership for every contribution arm.
- Forbid fake AST, generated TSX, source text, or mutable type-node injection as semantic truth.

**Discriminating proof:**

- Round-trip and exhaustive-match guards for the contribution taxonomy.
- Negative compile/static tests prove contributors cannot access private resolver APIs.

### NCK1-SB4 - Provenance, read sets, and validation

**Independently testable outcome:** No contributed fact can be admitted without exact source and environment basis.

**Architecture:**

- Define contribution batch basis, profile epoch, source revision, coordinate encoding, and dependency facts.
- Validate self-roots and reject stale, partial, cancelled, cyclic, or foreign-profile contributions.
- Specify ReturnOnly behavior for budget-exhausted contribution production.

**Expected changes:**

- Align validation with FactDomain::ProgramAnalysis and ReadSetSignature laws.
- Define deterministic batch ordering and digesting.

**Discriminating proof:**

- Mutation tests for stale source, wrong profile, missing read set, and forged complete status.
- Incremental contribution snapshot equals a fresh rebuild on the same inputs.

### NCK1-SB5 - Contributor capability boundary

**Independently testable outcome:** A contributor is a declarative producer, never a resolver or checker.

**Architecture:**

- Define capability-scoped input views: indexed syntax facts, carrier metadata, resolved catalog identity, and validated existing facts.
- Do not expose raw project store mutation, provider handles, or semantic dispatch.
- Define zero-work behavior for profiles without the capability.

**Expected changes:**

- Amend universal catalog/profile contracts to register contributor capabilities.
- Specify separate discovery, indexing, and contribution stages.

**Discriminating proof:**

- Static API surface test rejects forbidden resolver/session methods in contributor context.
- Disabled profile performs zero contribution construction and zero index writes.

### NCK1-SB6 - Migration and compatibility proof

**Independently testable outcome:** Existing function flow and framework facts map into the new contract without dual authorities.

**Architecture:**

- Characterize existing FunctionFlowGraph and current Vue/Svelte template facts.
- Define one-way migration into region/contribution snapshots.
- Do not retain legacy and new same-role stores after cutover.

**Expected changes:**

- Create migration manifest by owner and store.
- Assign implementation to NCK2/NCK5 rather than mutating production here.

**Discriminating proof:**

- Byte/semantic characterization fixtures prove accepted function behavior survives.
- A planted write to a displaced same-role store fails the ownership guard.

## Data, identity, invalidation, and publication laws

- Region IDs are project/profile/source qualified and never alias across files, framework profiles, or parser epochs.
- Region graph admission is independent of diagnostic demand; a graph may exist without checking and a check may demand only a slice.
- Contribution batches are immutable, sorted, validated, and atomically replaced by basis.
- A candidate index may point to contribution identity but cannot store final relation/call/checker verdicts.
- FrameworkRegion kinds remain open through profile registration; core code does not branch on Vue or Svelte names.

## Migration and cutover

- Reserve types and ownership first; NCK2 and NCK5 implement builders and ingestion.
- Map existing function-region identity with an explicit compatibility test, then remove legacy naming only when the final consumer moves.
- Admit no framework contribution until its profile epoch and validation contract are available.

## Deletions

- Delete the function-is-the-only-region assumption from live architecture.
- Delete any adapter proposal that receives raw semantic dispatch or synthesizes TSX as semantic truth.
- Delete duplicate ProgramAnalysis contribution stores after atomic migration.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

## Forbidden designs

- Source-offset-only region IDs, mutable region nodes, or per-node owned collections in hot structural storage.
- A framework-specific core enum branch or checker engine.
- Index-backed final semantic verdicts.
- Unvalidated injected narrowing/contextual/relation facts.
- Whole-workspace region graph construction as a prerequisite for an interactive leaf query.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

## Acceptance IDs and discriminating proof

- **NCK1-AC-REGION:** region taxonomy and identity are exact, collision-tested, and preserve FunctionFlowGraph compatibility.
- **NCK1-AC-CONTRIBUTION:** every contribution arm has sole ownership, provenance, validation, and no text/fake-AST path.
- **NCK1-AC-BOUNDARY:** contributor contexts expose no resolver or provider capability.
- **NCK1-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **NCK1-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **NCK1-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **NCK1-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Performance and bounded work

- Target ceiling: 0 production LOC, 0 production files, and 0 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Mandatory rescope and abort conditions

- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. Authority/schema tests for region and contribution taxonomies.
1. Property tests for stable region identity and deterministic contribution digests.
1. Static API-surface negative tests for contributor access to resolver/session internals.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Consumers and unlocks

- Unlocks NCK2 diagnostic queries and NCK5 framework ingress.
- Provides region/target identity input to LSO2 without coupling language-service operations to checker internals.
- Provides a future common substrate for compiler and lint consumers that explicitly demand executable regions.

## Source reconciliation

- `docs/arch/native-checker.md` executable-region and ProgramAnalysisContributor sections.
- `docs/arch/native-flow-return.md` function flow substrate requirements transferred to D8.
- `docs/arch/multi-framework-adapters-plan.md` durable typed-contribution and one-resolver rules.

Durable clauses are transferred as digest-bound requirement atoms. Historical path archaeology, obsolete branches, and implementation journals are not copied into the charter. Git history remains the archive.
