# Rev11 successor static charters

> Combined review artifact. Canonical individual charter paths remain authoritative within this proposal pack.


# Module `expansion-native-checker`


---

<!-- unified-charter-v2
id=NCK0
name=Native diagnostic authority and parity-certification constitution
phase=expansion
train=expansion.native-checker
product=native_checker
kind=constitution
semantic_role=delivery
class=successor
predecessors=UAK1,D8,E4,G2,TCM3,TIF1,LRA0,PUB0
conditional_predecessors=
owner=expansion.native-checker:sole native semantic diagnostic authority and certified-family cutover
conflict_domains=semantic_authority,diagnostic_action_service,public_protocol
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
charter=charters/expansion-native-checker/NCK0.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCK0 - Native diagnostic authority and parity-certification constitution

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

The internal subblocks below are binding decomposition and review checkpoints. They do not receive independent dispatch, leases, receipts, or deletion ownership unless the pre-scope architect proves that one subblock is independently acceptable or the block crosses a mandatory rescope trigger. In that case, amend the DAG before production mutation rather than treating a train-sized subblock as an implementation checklist.

## Independently acceptable outcome

Ratify the native semantic checker constitution: one diagnostic authority over the existing resolver, a typed diagnostic result model, a family and feature-slice certification law, and an atomic provider-to-native cutover protocol. This block changes authority and contracts only; it does not implement checker execution.

The current owner is **fragmented parser diagnostics, framework-specific checks, lint registration, provider diagnostics, LSP merge logic, and legacy Native Checker prose**. The final and sole owner is **the native checker product constitution, with semantic facts owned by their existing resolver and diagnostic evaluation owned by expansion.native-checker**.

## Architectural role and end state

NCK0 prevents the checker from becoming a second type system. It defines the ownership boundary between semantic fact production, diagnostic evaluation, framework contributions, external oracle certification, lint, publication, and fixes. Every later NCK and generated NCF node must be mechanically derivable from this constitution.

## Expected production surfaces

- `docs/arch/refactor/rev11/sources/legacy-arch-reconciliation.md` and generated authority catalogs
- `crates/verter_identity/src` for stable diagnostic, family, rule, and certification identities
- `crates/verter_protocol` for the future public diagnostic batch contract owned with PUB0
- `crates/verter_diagnostics`, `crates/verter_actions`, `crates/verter_semantic`, and `crates/verter_session` as future implementation owners
- `crates/verter_type_runtime` and `crates/verter_lsp` only for certified observation and cutover boundaries, never native semantic computation

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.

## Named APIs and data boundaries

- `DiagnosticOrigin`, `DiagnosticFamilyId`, `DiagnosticFeatureSliceId`, and `DiagnosticRuleId`
- `DiagnosticAuthorityState::{External, ObserveNative, CertifiedNative, Disabled}`
- `DiagnosticCertification` and immutable family certification receipts
- `DiagnosticBasis`, `DiagnosticCompleteness`, and typed operational outcomes
- `CorrectionOverlayEntry` as test and certification data, not a runtime compatibility mode
- `DiagnosticDedupKey` and the law that one family/profile/slice has one publishing authority

## Exact predecessor contracts

- **UAK1:** consume the universal-tooling constitution and product split so the checker is a successor product rather than an amendment hidden inside Rev11 finalization.
- **D8:** consume complete shared flow, call, contextual, and relation result admission; incomplete flow facts may not be relabeled as checker results.
- **E4:** consume reclaimable semantic storage and scoped interning so checker results cannot retain the whole project graph.
- **G2:** consume same-key singleflight ownership and ReturnOnly admission laws for checker query families.
- **TCM3:** consume the certified TypeScript semantic capability and observation identity contract; external TypeScript is oracle/fallback authority, never native query-time computation.
- **TIF1:** consume the TypeInfo-first public semantic contract and component metadata cutover.
- **LRA0:** consume diagnostic, rule, suppression, action, and authored-fix ownership boundaries.
- **PUB0:** consume the versioned public result/outcome vocabulary and truthful capability law.

External custody: none beyond the package activation boundary.

## Binding architecture

- Exactly one resolver remains authoritative for symbols, types, relation, calls, overloads, contextual typing, and flow. The checker evaluates diagnostic rules over those facts and may not recompute them.
- Diagnostic classes remain distinct: parser/recovery, native semantic, framework semantic, lint, external provider, and project/configuration diagnostics share a public shape but not authority or suppression rules.
- Certification and cutover occur at `profile + diagnostic family + semantic feature slice`; never at a vague project-wide percentage or one global boolean.
- External TypeScript is the oracle and fallback owner for uncertified families. Native checker execution never invokes tsserver or tsgo to decide a native result.
- The resolver has one correctness behavior. A reviewed correction overlay records conceded TypeScript bugs for certification; no user-facing compat mode or cache-key spec dimension exists.
- Every diagnostic carries authored provenance, exact input basis, completeness, and optional proof/fix references. Identity-less side effects are forbidden.
- Shadow observation is non-publishing. Promotion to CertifiedNative atomically suppresses the external family before native publication becomes visible.
- No monolithic CheckProgram cache entry is allowed. Program checks are coordinators over scoped region, file, and project-rule queries.

## Internal subblocks

### NCK0-SB1 - Diagnostic ownership matrix

**Independently testable outcome:** Every diagnostic class, family, and surface has a named owner and no overlapping publication authority.

**Architecture:**

- Define the authority matrix across parser, semantic checker, framework adapters, lint, external provider, and configuration/project services.
- Define which owner may create proof references, suppressions, related locations, and fixes.
- Require a stable family and feature-slice identity for every diagnostic capable of cutover.

**Expected changes:**

- Add the matrix and machine-readable catalog schema to Rev11 authority.
- Map existing diagnostics and legacy Native Checker clauses to the new classes.
- Reject an uncategorized diagnostic at registration and publication boundaries.

**Discriminating proof:**

- A planted duplicate owner or uncategorized diagnostic must fail the catalog validator.
- The generated ownership table must be byte-deterministic and complete against registered diagnostics.

### NCK0-SB2 - Typed result and operational outcome law

**Independently testable outcome:** Diagnostic results cannot collapse cancellation, stale state, missing inputs, or unsupported capability into empty success.

**Architecture:**

- Specify complete, NeedInputs, unsupported, cancelled, stale, and superseded outcomes.
- Specify that partial diagnostic batches are ReturnOnly and never warm-admitted as complete.
- Separate result completeness from an empty diagnostic vector.

**Expected changes:**

- Amend PUB0 result vocabulary and LRA0 diagnostic provenance requirements.
- Reserve the native checker query result domain without adding live query keys yet.

**Discriminating proof:**

- Mutation tests must prove empty-complete differs from NeedInputs, cancelled, stale, and unsupported.
- Serialization round trips must preserve basis and completeness exactly.

### NCK0-SB3 - Family and feature-slice taxonomy

**Independently testable outcome:** The checker can be implemented and certified in bounded slices rather than one train-sized parity claim.

**Architecture:**

- Define required diagnostic families and a stable feature-slice namespace.
- Permit a family to contain many independently generated NCF nodes.
- Define terminal criteria as manifest completeness, not a hand-maintained percentage.

**Expected changes:**

- Bind the family manifest schema and generated-node policy.
- Define split and merge rules for slices without renumbering published identities.

**Discriminating proof:**

- A missing required slice or duplicate slice identity must fail generation.
- A manifest reorder must not change generated node identity or evidence keys.

### NCK0-SB4 - Certification and correction-overlay constitution

**Independently testable outcome:** Native parity can be certified against TypeScript without placing TypeScript on the runtime query path or implementing bug-for-bug modes.

**Architecture:**

- Separate recomputable oracle snapshots from review-gated correction overlays.
- Require issue/evidence, semantic rationale, affected slices, and expiry review for each correction.
- Disallow production access to oracle values except static explanatory issue metadata explicitly approved by PUB0.

**Expected changes:**

- Amend TCM3 certification inputs and source atoms.
- Define deterministic canonicalization of provider diagnostics before comparison.

**Discriminating proof:**

- Planting a runtime provider callback, compat-mode query field, or unreviewed overlay must fail a critical guard.
- Recomputing an unchanged oracle corpus must produce byte-identical snapshots.

### NCK0-SB5 - Atomic authority transition law

**Independently testable outcome:** A family can move from external ownership to native ownership without duplicates, gaps, or stale mixed publication.

**Architecture:**

- Define External, ObserveNative, CertifiedNative, and Disabled transitions.
- Bind transitions to exact profile, provider epoch, native implementation receipt, and certification receipt.
- Require latest-basis publication and cancellation of superseded observation work.

**Expected changes:**

- Amend COX0/LRA0/PUB0 transition and publication contracts.
- Define rollback only to the previous certified authority receipt, never to an implicit fallback.

**Discriminating proof:**

- State-machine tests must reject illegal transitions and mixed-epoch batches.
- A planted double-publication path must fail before user-visible output.

### NCK0-SB6 - Critical guard and source-transfer index

**Independently testable outcome:** The constitution is mechanically tied to durable source atoms and named guards before legacy docs are deleted.

**Architecture:**

- Name guards for one resolver, no runtime oracle callback, no compat mode, exact authority, typed outcomes, and no monolithic program cache.
- Bind legacy Native Checker requirements to exact NCK targets and digests.

**Expected changes:**

- Register requirement atoms in `legacy-arch-reconciliation.md`.
- Add the future guard names to the authority catalog; implementation nodes activate them with code.

**Discriminating proof:**

- The legacy disposition validator must refuse deletion if any atom lacks a target charter.
- A renamed or removed guard without an amendment must fail authority validation.

## Data, identity, invalidation, and publication laws

- Diagnostic identity is independent of message wording and source position; it is rooted in family, rule, semantic subject, authored anchor identity, profile, and exact input basis.
- Severity and presentation are policy fields; they do not change semantic diagnostic identity or cache identity unless the rule itself branches on policy.
- A certified family result must name the facts and environment dimensions it read. Provider epoch enters only observation/cutover identity, never native semantic computation.
- Diagnostic ordering is deterministic: primary authored location, family ID, rule ID, semantic subject identity, then stable tie-breaker.
- Fixes are references to authored edit intents owned by LRA0/LSO7, not opaque text edits embedded in semantic facts.

## Migration and cutover

- Land this constitution and source atoms before deleting `docs/arch/native-checker.md`.
- Do not activate native query keys or publish native semantic diagnostics in NCK0.
- Classify every existing diagnostic producer and record unknown cases as blocking migration debt, not inferred ownership.
- Update successor DAG and existing contract charters in one amendment so no interim authority contradiction exists.

## Deletions

- Delete the legacy Native Checker prose only after all durable clauses are digest-bound to NCK0-NCK8 and generated-family authority.
- Delete any proposed checker-specific resolver or TypeScript compatibility-mode design from live authority.
- Delete ambiguous claims that a green coverage ledger alone proves TypeScript semantic parity.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

## Forbidden designs

- A checker-private type walker, relation engine, overload resolver, flow engine, symbol table, or module resolver.
- Runtime tsserver/tsgo calls from a native Check query.
- One global native-checker enabled boolean used as a substitute for family/slice authority.
- Diagnostics stored as GraphTypeNode arms or identity-less side products.
- A monolithic whole-program cache entry or eager workspace check on an interactive leaf request.
- Permanent duplicate native and provider diagnostics hidden by message-text deduplication.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

## Acceptance IDs and discriminating proof

- **NCK0-AC-AUTHORITY:** generated ownership and transition tables cover every registered diagnostic origin and reject overlap.
- **NCK0-AC-ONE-ENGINE:** static guard text and architecture tests reject any checker semantic resolver surface.
- **NCK0-AC-CERTIFICATION:** correction-overlay and oracle rules are exact and contain no runtime compatibility path.
- **NCK0-AC-LEGACY:** every durable clause from `native-checker.md` has a digest-bound disposition.
- **NCK0-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **NCK0-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **NCK0-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **NCK0-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Performance and bounded work

- The constitution must declare zero hidden work for Disabled and External-only native paths.
- Certification cost is test/offline work and must not enter runtime latency budgets.
- Target ceiling: 0 production LOC, 0 production files, and 0 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Mandatory rescope and abort conditions

- Abort if any diagnostic family cannot name a sole semantic fact owner and a sole publishing owner.
- Abort if certification requires generated TypeScript text to become semantic truth rather than oracle input.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. `programctl validate-authority --module expansion-native-checker` and source-coverage validation.
1. Schema tests for diagnostic family, authority state, result outcome, and correction overlay catalogs.
1. Negative mutations for duplicate owner, runtime provider callback, compat-mode field, and unclassified legacy clause.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Consumers and unlocks

- Unlocks NCK1 and all later native checker implementation.
- Provides the diagnostic authority contract consumed by CLI2, LSO8, COX0, LRA0, and PUB0 amendments.
- Defines the promotion law used by generated NCF family nodes.

## Source reconciliation

- `docs/arch/native-checker.md` blob `3e96bf48ec481e97b9fd3067041e21099d194944`.
- `docs/arch/native-typeinfo-parity.md` and the D/E/G/TCM authority it was partially absorbed into.
- `docs/arch/ts-compat-two-mode-model.md` durable single-spec/correction-overlay decision.

Durable clauses are transferred as digest-bound requirement atoms. Historical path archaeology, obsolete branches, and implementation journals are not copied into the charter. Git history remains the archive.


---

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


---

<!-- unified-charter-v2
id=NCK2
name=Incremental diagnostic query and result domain
phase=expansion
train=expansion.native-checker
product=native_checker
kind=implementation
semantic_role=delivery
class=successor
predecessors=NCK1,G2,H3,PUB0
conditional_predecessors=
owner=expansion.native-checker:sole native semantic diagnostic authority and certified-family cutover
conflict_domains=semantic_authority,semantic_cache_store,public_protocol
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
charter=charters/expansion-native-checker/NCK2.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCK2 - Incremental diagnostic query and result domain

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

The internal subblocks below are binding decomposition and review checkpoints. They do not receive independent dispatch, leases, receipts, or deletion ownership unless the pre-scope architect proves that one subblock is independently acceptable or the block crosses a mandatory rescope trigger. In that case, amend the DAG before production mutation rather than treating a train-sized subblock as an implementation checklist.

## Independently acceptable outcome

Implement the native checker query/result substrate: scoped Check query keys, typed DiagnosticBatch results, exact contexts and read sets, same-key production, reclaimable stores, aggregation, and public result conversion. This block implements no broad TypeScript diagnostic catalogue.

The current owner is **reserved or absent Check query names, ad hoc diagnostic vectors, LSP-owned aggregation, and provider-oriented result assumptions**. The final and sole owner is **ProjectSemanticDispatch and SemanticGraphStore-backed scoped diagnostic query families with typed outcomes and bounded storage**.

## Architectural role and end state

NCK2 makes diagnostics first-class semantic query values while preserving the one resolver. It supplies the execution, caching, cancellation, and aggregation substrate that NCK3 rules and generated NCF slices use.

## Expected production surfaces

- `crates/verter_session/src/semantic_query` for keys, contexts, specs, dispatch, and query-family admission
- `crates/verter_semantic` for diagnostic query values, bases, stores, and proof references
- `crates/verter_diagnostics` for immutable Diagnostic and DiagnosticBatch core types
- `crates/verter_protocol` and public adapters jointly with PUB0
- `crates/verter_session/tests` for spec-table generation, cache, cancellation, and incremental proofs

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.

## Named APIs and data boundaries

- `SemanticQueryKey::{CheckRegion, CheckFile, CheckProjectRule, CheckExpression}`
- `SemanticQueryValue::DiagnosticBatch` and `SemanticQueryValueTag::DiagnosticBatch`
- `CheckRegionContext`, `CheckFileContext`, `CheckProjectRuleContext`, `CheckExpressionContext`
- `DiagnosticBatch`, `DiagnosticBatchBasis`, `DiagnosticBatchOutcome`, `DiagnosticAggregate`
- `DiagnosticQueryStore`, per-family retention policy, and same-key FlightCell ownership
- `DiagnosticQuerySpec` generated alongside the enum/spec table

## Exact predecessor contracts

- **NCK1:** consume exact region identity and typed contribution ingress.
- **G2:** consume FlightCell-owned same-key production, cancellation, and admission laws.
- **H3:** consume exact-basis foreground settlement and stale-safe background publication semantics.
- **PUB0:** consume versioned typed public outcomes and capability truth.

External custody: none beyond the package activation boundary.

## Binding architecture

- CheckRegion is the primary semantic unit; CheckFile aggregates exact region and file-rule results; CheckProjectRule owns only genuinely project-scoped rules.
- CheckExpression is demand-sliced and interactive; it may not trigger whole-file checking unless the declared rule requirements demand it.
- There is no CheckProgram memo family. Workspace checking is an application coordinator over files/project rules with cancellation and progress.
- Query keys contain content-free identity and exact environment dimensions; result values are version-rooted by recorded facts.
- Only complete, current results admit. Cancelled, superseded, budget-exceeded, partial, or NeedInputs results are ReturnOnly.
- Storage is per project/profile/family, reclaimable, and bounded. Diagnostic payloads do not retain semantic arenas unnecessarily.

## Internal subblocks

### NCK2-SB1 - Query taxonomy and spec-table integration

**Independently testable outcome:** Every live Check key has one generated spec row and dispatch arm.

**Architecture:**

- Add scoped keys and tags together with value-domain, admission, allowed demand, environment dimensions, and cross-context guard.
- Keep project rules separate from file/region execution.
- Reserve future finer keys only through an amendment; do not add speculative dead variants.

**Expected changes:**

- Update enum, spec generator, artifact, tag set, and dispatch exhaustiveness in one change.
- Register critical guards for enum/spec/dispatch equality.

**Discriminating proof:**

- Enum-spec-dispatch triangulation fails on a planted missing row or dispatch arm.
- No Check key resolves to TypeNode or GraphTypeNode.

### NCK2-SB2 - Exact query contexts and identities

**Independently testable outcome:** Cross-project, profile, environment, and source revisions cannot warm-hit incorrectly.

**Architecture:**

- Define per-key contexts with only semantically relevant parse, resolve, type, lib, and project dimensions.
- Include region/file/project-rule identity and diagnostic family/slice identity.
- Keep content hashes on value/read-set rooting, not as substitutes for semantic identity.

**Expected changes:**

- Implement family key construction and minimal-dimension characterization tests.
- Add cross-context no-warm-hit guards.

**Discriminating proof:**

- Mutation matrix changes each identity axis independently and proves correct hit/miss behavior.
- Benched minimality rejects unnecessary dimensions that cause false misses.

### NCK2-SB3 - DiagnosticBatch core value domain

**Independently testable outcome:** A query result carries immutable diagnostics, basis, completeness, read set, and operational outcome.

**Architecture:**

- Define compact diagnostic records with stable IDs, primary/related authored anchors, proof refs, and fix-intent refs.
- Separate semantic diagnostic identity from localized message rendering.
- Use compact interned strings/IDs and avoid retaining full semantic nodes.

**Expected changes:**

- Implement core types and deterministic canonical ordering.
- Add public DTO conversion behind PUB0 schema/version gates.

**Discriminating proof:**

- Layout/size tests and deterministic serialization tests.
- Empty-complete, NeedInputs, cancelled, stale, and unsupported remain distinguishable.

### NCK2-SB4 - Same-key production and admission

**Independently testable outcome:** Concurrent identical checks compute once and only complete current results enter warm storage.

**Architecture:**

- Use the existing FlightCell/singleflight family runtime.
- Bind cancellation, deadline, budget, supersession, and provider-independent semantic basis.
- Record exact facts read by the rule executor.

**Expected changes:**

- Implement producer ownership, waiter behavior, admission probe, and ReturnOnly paths.
- Instrument compute, wait, cancellation, and admission counters.

**Discriminating proof:**

- Concurrency tests prove one producer and deterministic waiter results.
- Poison tests prove a cancelled/partial producer cannot populate the cache.

### NCK2-SB5 - Bounded diagnostic storage and reclamation

**Independently testable outcome:** Repeated checks do not create unbounded per-file, per-family, or per-revision retention.

**Architecture:**

- Define per-family retention and generation replacement.
- Store compact values detached from temporary semantic arenas.
- Evict superseded profile/source generations and release proof references safely.

**Expected changes:**

- Add DiagnosticQueryStore under the existing project semantic store ownership.
- Add memory counters and explicit teardown/epoch transition behavior.

**Discriminating proof:**

- Long-churn tests plateau RSS/retained bytes.
- A deleted/closed project releases all checker storage and contributor snapshots.

### NCK2-SB6 - File/project aggregation and public conversion

**Independently testable outcome:** Aggregators compose scoped results without becoming another semantic engine or cache authority.

**Architecture:**

- CheckFile joins exact region and file-rule batches; workspace coordination stays above query storage.
- Deduplicate only by stable diagnostic identity and authority, never message text.
- Propagate the least complete outcome and exact contributing bases.

**Expected changes:**

- Implement deterministic aggregation helpers and public DTO conversion.
- Leave publication arbitration to NCK6.

**Discriminating proof:**

- Fresh versus incremental aggregate equality across reordered region completion.
- A mixed stale/current input is rejected rather than published as current.

## Data, identity, invalidation, and publication laws

- A Check key may read semantic facts but may not mutate or create a second semantic node authority.
- DiagnosticBatch basis includes project/profile/source/region or rule identity plus canonical fact read-set signature.
- Message localization and editor formatting occur after semantic identity/cache computation.
- Aggregation is deterministic and non-cache-authoritative unless represented by its own exact scoped query key.
- CheckExpression has strict budgets and cannot admit a result that omitted a required rule input.

## Migration and cutover

- Introduce query values dormant until NCK3 registers rule executors; no user-visible native diagnostic publication in this block.
- Migrate ad hoc semantic diagnostic vectors only where exact identity and basis can be preserved; leave parser/lint/provider classes with their owners.
- Delete duplicate checker cache prototypes in the same candidate that routes their final consumer.

## Deletions

- Delete identity-less semantic diagnostic side channels displaced by DiagnosticBatch.
- Delete any ad hoc whole-file or whole-project checker cache introduced during prototyping.
- Delete duplicate per-feature same-key coordination for checker queries.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

## Forbidden designs

- CheckProgram as a monolithic memoized key.
- Diagnostic data embedded in TypeNode/GraphTypeNode or public TypeInfo graph nodes.
- Message text or source range as the sole diagnostic identity.
- Caching cancelled, partial, stale, budget-exceeded, or NeedInputs outcomes as complete.
- A query context that bundles an opaque project_config_hash instead of exact dimensions.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

## Acceptance IDs and discriminating proof

- **NCK2-AC-SPEC:** Check enum, tags, generated spec table, dispatch, and value tags are exactly equal.
- **NCK2-AC-ADMISSION:** only complete current batches warm-admit under same-key production.
- **NCK2-AC-MEMORY:** repeated revisions and project teardown prove bounded retention.
- **NCK2-AC-NO-PROGRAM-CACHE:** static guard rejects monolithic CheckProgram memo authority.
- **NCK2-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **NCK2-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **NCK2-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **NCK2-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Performance and bounded work

- Warm identical CheckRegion requests perform zero parse, index walk, semantic recomputation, provider work, and diagnostic allocation beyond result sharing.
- CheckFile aggregation is linear in returned scoped results, not project size.
- Target ceiling: 800 production LOC, 8 production files, and 2 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Mandatory rescope and abort conditions

- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. `cargo nextest run -p verter_semantic -p verter_session -p verter_diagnostics -p verter_protocol`.
1. Query-key spec generation/diff and cross-context mutation matrix.
1. Concurrent same-key, cancellation poison, incremental/fresh, and long-churn memory tests.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Consumers and unlocks

- Unlocks NCK3 rule execution and NCK4 certification harness integration.
- Provides typed result contracts consumed by NCK6, CLI2, LSO8, and public surfaces.

## Source reconciliation

- `docs/arch/native-checker.md` checker query layer and typed result requirements.
- `docs/arch/fact-based-cache.md` query identity/admission laws transferred through G1/G2/E4.
- Live `SemanticQueryKeySpec` table and D8 complete-result contract.

Durable clauses are transferred as digest-bound requirement atoms. Historical path archaeology, obsolete branches, and implementation journals are not copied into the charter. Git history remains the archive.


---

<!-- unified-charter-v2
id=NCK3
name=Shared-proof semantic diagnostic rule kernel
phase=expansion
train=expansion.native-checker
product=native_checker
kind=implementation
semantic_role=delivery
class=successor
predecessors=NCK2,D8,LRA0
conditional_predecessors=
owner=expansion.native-checker:sole native semantic diagnostic authority and certified-family cutover
conflict_domains=semantic_authority,flowslice,diagnostic_action_service
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
charter=charters/expansion-native-checker/NCK3.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCK3 - Shared-proof semantic diagnostic rule kernel

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

The internal subblocks below are binding decomposition and review checkpoints. They do not receive independent dispatch, leases, receipts, or deletion ownership unless the pre-scope architect proves that one subblock is independently acceptable or the block crosses a mandatory rescope trigger. In that case, amend the DAG before production mutation rather than treating a train-sized subblock as an implementation checklist.

## Independently acceptable outcome

Implement the shared-proof diagnostic rule kernel that plans fact demands, reads authoritative relation/call/flow/contextual/declaration facts, emits stable diagnostics and fix intents, and proves that no rule re-resolves semantic meaning. Only representative canary rules land here; catalogue parity belongs to generated NCF nodes.

The current owner is **scattered hard-coded checks, provider diagnostic messages, framework-specific validation, and prospective checker walkers**. The final and sole owner is **a static, typed, demand-declared diagnostic rule kernel over shared semantic proofs**.

## Architectural role and end state

NCK3 is the semantic checker engine in the narrow sense: not a resolver, but a rule planner/evaluator over existing facts. It establishes one reusable execution contract so every generated family slice implements semantic rules without forking infrastructure.

## Expected production surfaces

- `crates/verter_diagnostics` for rule descriptors, emission, suppression identity, and compact diagnostic construction
- `crates/verter_semantic` for read-only fact/proof views and typed rule demands
- `crates/verter_session` for query dispatch integration, rule planning, and exact read-set capture
- `crates/verter_actions` for typed fix intents, not direct edits
- `crates/verter_session/tests` and semantic fixtures for no-second-resolver guards and canary rules

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.

## Named APIs and data boundaries

- `DiagnosticRuleDescriptor`, `DiagnosticRulePlan`, and static `DiagnosticRuleRegistry`
- `FactRequirement`, `RuleApplicability`, `RuleBudget`, and `RuleExecutionContext`
- `DiagnosticFactView` exposing typed relation/call/flow/contextual/declaration results only
- `ProofRef`, `DiagnosticEmitter`, `SuppressionKey`, and `FixIntentRef`
- `RuleExecutionReceipt` with facts read, work counters, and completeness

## Exact predecessor contracts

- **NCK2:** consume scoped diagnostic queries, typed batches, same-key admission, and bounded stores.
- **D8:** consume complete authoritative flow/call/contextual results and completion algebra.
- **LRA0:** consume profile-scoped rule/action registration, provenance, suppression, and authored fix safety contracts.

External custody: none beyond the package activation boundary.

## Binding architecture

- Rules declare fact requirements before execution. Applicability and demand planning must permit zero work for irrelevant rules.
- The fact view exposes final typed outcomes and proof references, not mutable semantic stores or resolver callbacks.
- A negative relation or failed call applicability becomes a diagnostic through a rule; the rule does not rerun relation or overload matching.
- Control-flow diagnostics consume existing reachability, completion, return, capture, and narrowing facts.
- Rule registration is static/catalog-driven. Executing arbitrary third-party code inside the semantic engine is out of scope.
- Suppressions are keyed by stable rule/subject/provenance identity and cannot hide diagnostics from unrelated authorities.
- Fixes are semantic intents that LSO7 later materializes against authored current source.

## Internal subblocks

### NCK3-SB1 - Static rule descriptor and registry

**Independently testable outcome:** Every rule has exact family/slice identity, applicability, fact requirements, severity class, fix capability, and owner.

**Architecture:**

- Define a generated/static registry keyed by DiagnosticRuleId.
- Separate semantic rules from lint and framework-owned rule descriptors while allowing one public shape.
- Declare profile/language/region applicability without framework switches in core.

**Expected changes:**

- Implement descriptor types and registry generation hooks.
- Bind every rule to NCK4 manifest rows and LRA0 action policy.

**Discriminating proof:**

- Registry completeness and duplicate-ID mutation tests.
- An inapplicable rule records zero fact reads and zero allocations.

### NCK3-SB2 - Demand planning and applicability

**Independently testable outcome:** The kernel requests only facts required by applicable rules and never whole-checks by default.

**Architecture:**

- Compile applicable rule requirements into a deterministic RulePlan.
- Coalesce identical fact demands while preserving rule attribution.
- Propagate budget, cancellation, and NeedInputs before evaluation.

**Expected changes:**

- Implement planner and demand counters.
- Add plan dumps for tests/evidence, not production semantic authority.

**Discriminating proof:**

- Permutation tests produce byte-identical plans.
- A leaf CheckExpression proves unrelated file/project rules perform zero work.

### NCK3-SB3 - Read-only shared fact and proof view

**Independently testable outcome:** Rules can inspect authoritative facts without access to private resolver algorithms or mutable stores.

**Architecture:**

- Expose typed read methods for relation, resolve-call, overload, flow, contextual, declaration, and project-index facts.
- Record every read into the Check query read set.
- Return typed incomplete/NeedInputs rather than synthesizing fallback facts.

**Expected changes:**

- Implement capability-limited view wrappers.
- Add compile/static guards banning resolver entry points from rule modules.

**Discriminating proof:**

- A planted direct resolver call or store mutation fails static architecture tests.
- Read-set mutation invalidates the right cache entry and no broader family.

### NCK3-SB4 - Diagnostic emission, proof, and dedup

**Independently testable outcome:** Rule output is stable, authored, evidence-linked, and deterministic.

**Architecture:**

- Construct semantic diagnostic identity before localized message rendering.
- Attach primary and related authored anchors plus optional ProofRef.
- Deduplicate by stable identity/authority, not message text.

**Expected changes:**

- Implement DiagnosticEmitter and canonical sorting.
- Create proof retention/refcount policy compatible with NCK2 reclamation.

**Discriminating proof:**

- Equivalent reordered fact delivery yields byte-identical batches.
- Two distinct semantic subjects with identical messages never collapse.

### NCK3-SB5 - Suppression and fix-intent boundary

**Independently testable outcome:** Suppressions and fixes preserve owner, profile, source basis, and safety class.

**Architecture:**

- Model suppression directives separately from diagnostics and lint configuration.
- Emit fix intents containing semantic target and transformation class, never generated-coordinate TextEdits.
- Classify safe, suggested, and unsafe intents under LRA0.

**Expected changes:**

- Implement typed refs and validation hooks; LSO7 remains the edit materializer.
- Add duplicate/suppression provenance guards.

**Discriminating proof:**

- Stale or foreign-profile suppression fails closed.
- No fix intent can be converted without an exact authored basis.

### NCK3-SB6 - Representative canary rules and one-engine guards

**Independently testable outcome:** The kernel proves its architecture on a small cross-family set without absorbing the parity train.

**Architecture:**

- Canaries: assignment relation failure, failed call applicability, missing return/unreachable region, and duplicate declaration project rule.
- Each canary must consume an existing authoritative fact and carry an oracle fixture.
- No additional family breadth is accepted in NCK3.

**Expected changes:**

- Implement canaries and named guards.
- Record remaining catalogue work only in the NCK4 manifest.

**Discriminating proof:**

- Mutation of the underlying shared fact changes the diagnostic; mutation of a duplicate checker algorithm is impossible because none exists.
- Canary differential and incremental/fresh tests pass across native and provider observation.

## Data, identity, invalidation, and publication laws

- A rule result is complete only when all declared required facts are complete on the same basis.
- Rule execution order does not affect diagnostic identity, ordering, or read-set signature.
- Rules cannot mutate semantic facts or write index state.
- Framework-owned rules enter through NCK5 descriptors/contributions but run on the same kernel.
- Proof references are opaque stable handles with lifecycle tied to the batch/store generation.

## Migration and cutover

- Move only representative checks whose fact authority and exact replacement can be proven.
- Leave lint rules in LRA0/LNT ownership and provider semantic families external until their generated NCF slice is certified.
- Delete a displaced hard-coded rule only in the same candidate that routes its complete demand and output through the kernel.

## Deletions

- Delete canary-equivalent ad hoc semantic checks and duplicate rule registries.
- Delete direct TextEdit construction from semantic checker rules.
- Delete any checker-private relation/call/flow helper introduced during implementation.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

## Forbidden designs

- Rules parsing source text, regexing type text, synthesizing/reparsing TypeScript, or walking types to reproduce resolver decisions.
- Dynamic third-party rule code in the trusted semantic process.
- Message-text dedup or range-only suppression.
- Rules emitting LSP coordinates or provider handles.
- Expanding NCK3 into the full diagnostic catalogue.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

## Acceptance IDs and discriminating proof

- **NCK3-AC-FACTS:** every canary diagnostic is traceable to declared authoritative facts and exact read-set entries.
- **NCK3-AC-NO-RESOLVER:** static architecture guard rejects resolver calls and duplicate semantic algorithms in rule modules.
- **NCK3-AC-ZERO-WORK:** inapplicable rules execute no fact demand, provider call, or allocation.
- **NCK3-AC-CANARIES:** four cross-family canaries pass oracle, incremental, cancellation, and proof tests.
- **NCK3-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **NCK3-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **NCK3-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **NCK3-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Performance and bounded work

- Rule planning cost is proportional to applicable registered rules for the selected profile/slice, with catalog indexing preventing global scans.
- Repeated warm canary checks allocate only the returned batch representation or share it according to NCK2 policy.
- Target ceiling: 800 production LOC, 8 production files, and 2 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Mandatory rescope and abort conditions

- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. `cargo nextest run -p verter_diagnostics -p verter_actions -p verter_semantic -p verter_session`.
1. Static one-engine guards and rule-registry generation tests.
1. Canary differential, zero-work, incremental/fresh, cancellation, and proof-lifecycle tests.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Consumers and unlocks

- Unlocks NCK4 manifest/oracle generation and NCK5 framework rule ingress.
- Supplies the sole diagnostic rule execution contract for every generated NCF slice.

## Source reconciliation

- `docs/arch/native-checker.md` diagnostics-from-facts and named guard sections.
- D8 flow/call authority and LRA0 rule/action contract.

Durable clauses are transferred as digest-bound requirement atoms. Historical path archaeology, obsolete branches, and implementation journals are not copied into the charter. Git history remains the archive.


---

<!-- unified-charter-v2
id=NCK4
name=Diagnostic-family manifest, hermetic oracle, certification, and node generator
phase=expansion
train=expansion.native-checker
product=native_checker
kind=implementation
semantic_role=delivery
class=successor
predecessors=NCK3,TCM4,VIM1,PER0
conditional_predecessors=
owner=expansion.native-checker:sole native semantic diagnostic authority and certified-family cutover
conflict_domains=semantic_authority,vertical_manifest,performance_evidence
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
charter=charters/expansion-native-checker/NCK4.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCK4 - Diagnostic-family manifest, hermetic oracle, certification, and node generator

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

The internal subblocks below are binding decomposition and review checkpoints. They do not receive independent dispatch, leases, receipts, or deletion ownership unless the pre-scope architect proves that one subblock is independently acceptable or the block crosses a mandatory rescope trigger. In that case, amend the DAG before production mutation rather than treating a train-sized subblock as an implementation checklist.

## Independently acceptable outcome

Implement the machine-readable diagnostic-family manifest, hermetic TypeScript oracle corpus, deterministic diagnostic canonicalizer, review-gated correction overlays, generated NCF DAG/charter production, and evidence receipts. This block creates the parity production system; it does not implement all family slices itself.

The current owner is **free-form parity prose, scattered ignored tests, manually curated provider expectations, and no checker-family DAG generator**. The final and sole owner is **one source-digest-bound manifest and generator that creates bounded, independently acceptable native checker family slices**.

## Architectural role and end state

NCK4 converts the multi-person-year checker catalogue into explicit program work. It prevents parity claims from being hidden in a monolithic block and makes certification reproducible, reviewable, and tied to exact TypeScript engine identity.

## Expected production surfaces

- `docs/arch/refactor/rev11/catalogs` for diagnostic family and correction-overlay schemas
- `docs/arch/refactor/rev11/generated` and authority DAG/charters for generated NCF nodes
- `crates/verter_session/tests`, `crates/verter_diagnostics/tests`, and hermetic conformance corpora
- `crates/verter_type_runtime` or dedicated test harness code for oracle observation only
- `tools` or a dedicated Rust generator binary; tests never write generated authority artifacts

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.

## Named APIs and data boundaries

- `DiagnosticFamilyManifest`, `DiagnosticFamilyRow`, `DiagnosticFeatureSliceRow`
- `DiagnosticOracleCase`, `OracleEngineIdentity`, `OracleSnapshot`, `DiagnosticCanonicalizer`
- `CorrectionOverlay`, `CorrectionOverlayEntry`, and review/expiry metadata
- `GeneratedCheckerNodeSpec`, `DiagnosticFamilyReceipt`, and `FamilyPromotionEvidence`
- `gen-native-checker-dag` as the sole writer of generated NCF DAG/charter/index artifacts

## Exact predecessor contracts

- **NCK3:** consume the exact rule kernel and canary execution/evidence format.
- **TCM4:** consume certified TypeScript engine binding, input basis, mapping, and observation identity.
- **VIM1:** consume deterministic manifest compilation and conformance generation patterns.
- **PER0:** consume equivalent-work, allocation, latency, and retained-memory evidence methodology for certification and generated slices.

External custody: none beyond the package activation boundary.

## Binding architecture

- Manifest rows, not prose section headings, define required checker scope and terminal completeness.
- One generated NCF node owns one bounded semantic feature slice, exact rule population, exact deletion population, oracle corpus, and certification receipt.
- Oracle execution is hermetic and test-only. Production native queries have no access to provider observation.
- Diagnostic comparison canonicalizes codes, semantic family, subject, authored locations, related locations, severity, and stable message parameters; raw localized strings are not primary equality.
- Correction overlays are sparse, review-gated exceptions for clear TypeScript bugs and cannot become a second runtime behavior.
- The generator is the sole writer; tests render in memory and diff committed outputs.
- Generated node identity remains stable under manifest reordering and changes only when its semantic slice identity changes.

## Internal subblocks

### NCK4-SB1 - Manifest schema and family partition

**Independently testable outcome:** The full required diagnostic catalogue is partitioned into stable, bounded slices with no unowned rows.

**Architecture:**

- Define family, slice, rule population, applicability, prerequisites, oracle cases, deletion owner, and performance counters.
- Require explicit required/optional status and terminal coverage.
- Allow later versioned additions without renumbering existing slice identity.

**Expected changes:**

- Implement schema parser/validator and canonical renderer.
- Populate initial required families and representative rows.

**Discriminating proof:**

- Coverage bijection and duplicate/missing mutation tests.
- Reordering input produces identical canonical manifest and generated IDs.

### NCK4-SB2 - Hermetic oracle corpus and engine identity

**Independently testable outcome:** Every certified row is reproducible against an exact TypeScript/tsgo engine and exact project inputs.

**Architecture:**

- Pin engine artifact/version/platform, libs, compiler options, module graph, source encoding, and expected observation surface.
- Keep third-party corpora optional and external; required certification fixtures are vendored/hermetic.
- Separate syntax/provider failures from semantic diagnostic observations.

**Expected changes:**

- Implement oracle runner and fixture format.
- Capture deterministic snapshots only through an explicit recompute command.

**Discriminating proof:**

- Fresh recompute on the same engine/input is byte-identical.
- Engine/options/lib mutation changes the oracle identity and invalidates affected receipts.

### NCK4-SB3 - Diagnostic canonicalization and comparison

**Independently testable outcome:** Native/provider outputs compare semantically rather than by unstable localized text or generated coordinates.

**Architecture:**

- Normalize provider codes, categories, message arguments, authored locations, related info, and family mapping.
- Map generated/provider coordinates through exact TCM basis and drop unverifiable observations from certification rather than guessing.
- Represent missing, extra, mismatched, and non-comparable outcomes explicitly.

**Expected changes:**

- Implement canonicalizer and structured diff output.
- Add cross-platform/locale stability fixtures.

**Discriminating proof:**

- Locale and ordering mutations preserve semantic canonical result.
- Synthetic/unmappable provider locations cannot be silently accepted as parity.

### NCK4-SB4 - Correction overlay and divergence registry

**Independently testable outcome:** Approved TypeScript bugs are represented as sparse data with explicit evidence, never runtime modes.

**Architecture:**

- Require issue reference or equivalent evidence, affected rows, TS oracle value, Verter correct value, rationale, reviewer receipts, and review date.
- Default every non-overlay row to exact TypeScript parity.
- Provide expiry/revalidation when TypeScript fixes the bug.

**Expected changes:**

- Implement overlay schema, validator, and co-presence metadata rules.
- Compile only static issue metadata into production when explicitly authorized; oracle values remain test data.

**Discriminating proof:**

- Unreviewed, orphaned, broad wildcard, or stale overlay entries fail validation.
- Removing an overlay after an upstream fix restores ordinary parity comparison.

### NCK4-SB5 - Generated NCF DAG and charter writer

**Independently testable outcome:** Each semantic feature slice becomes a real bounded DAG node with a detailed charter and exact predecessors.

**Architecture:**

- Derive node ID, name, owner, conflict domains, budgets, source atoms, rule population, oracle fixtures, deletions, and acceptance IDs.
- Generate a detailed family charter from row-specific architecture templates; do not emit generic one-line charters.
- Require amendment review before generated authority enters the live DAG.

**Expected changes:**

- Implement `gen-native-checker-dag` and generated output directories.
- Add in-memory render/diff tests and cycle/reachability validation.

**Discriminating proof:**

- Tests never write generated files.
- A row exceeding limits or containing multiple independently acceptable outcomes fails generation and requests manual rescope.

### NCK4-SB6 - Certification receipts and promotion evidence

**Independently testable outcome:** A family slice can be promoted only from immutable implementation, oracle, performance, and review evidence.

**Architecture:**

- Bind candidate tree, implementation receipt, manifest row digest, oracle engine/input, diff result, correction overlays, incremental/fresh proof, and work counters.
- Separate observation success from authority promotion.
- Make NCK6 consume receipts rather than rerun hidden certification logic.

**Expected changes:**

- Implement receipt schema and validator.
- Generate human-readable evidence summaries from structured data.

**Discriminating proof:**

- Changing any input invalidates the receipt.
- A clean observation without exact candidate or manifest digest cannot promote authority.

## Data, identity, invalidation, and publication laws

- The family manifest is the exact scope authority; generated reports are derivative and never hand-edited.
- Oracle snapshots and correction overlays are test/evidence artifacts, not production semantic dependencies.
- Every generated NCF node owns an exact rule set and legacy deletion set; overlapping ownership is invalid.
- Certification receipts are immutable and content-addressed.
- A non-comparable provider observation is not a pass and cannot be hidden as an ignored test.

## Migration and cutover

- Import durable parity rows from legacy TypeInfo/checker docs and existing ignored tests into the manifest with explicit status.
- Do not mechanically convert every old test into required checker scope without classifying its semantic family and authority.
- Generate NCF nodes through an amendment and keep them locked until predecessors and implementation receipts exist.

## Deletions

- Delete free-form checker parity ledgers and generator-by-test patterns displaced by the manifest/generator.
- Delete wildcard ignored-test acceptance and manually stamped parity percentages.
- Delete runtime compatibility-mode scaffolding if any exists.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

## Forbidden designs

- One NCK4 implementation claiming the full TypeScript diagnostic catalogue.
- Tests mutating checked-in manifests, DAGs, charters, or snapshots.
- Localized message text as the sole parity comparator.
- Oracle execution in production or network-dependent required certification tests.
- Correction overlays without row-exact scope and independent review.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

## Acceptance IDs and discriminating proof

- **NCK4-AC-BIJECTION:** required manifest rows, generated NCF nodes, charters, and terminal coverage are exact bijections.
- **NCK4-AC-ORACLE:** hermetic recomputation is deterministic and engine/input identity is exact.
- **NCK4-AC-GENERATOR:** dedicated generator is sole writer; tests only assert in-memory equality.
- **NCK4-AC-OVERLAY:** sparse correction overlays satisfy evidence, scope, review, and expiry laws.
- **NCK4-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **NCK4-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **NCK4-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **NCK4-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Performance and bounded work

- Certification harness performance is measured separately from runtime; generated slice charters still require runtime equivalent-work counters.
- Manifest parsing/generation is deterministic and bounded by row count with no repository-wide semantic scan.
- Target ceiling: 800 production LOC, 8 production files, and 2 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Mandatory rescope and abort conditions

- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. `cargo nextest run` for manifest, canonicalizer, oracle harness, overlay, receipt, and generator crates/tests.
1. Run explicit oracle recompute in hermetic mode and compare committed snapshots.
1. Run generator in check mode plus planted missing/duplicate/oversized/cycle mutations.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Consumers and unlocks

- Generates the NCF implementation backlog and evidence contract.
- Supplies certification receipts consumed by NCK6 authority promotion and NCK7 terminal completeness.
- Provides checker rows consumed by LSO8 and CLI conformance when native diagnostics are enabled.

## Source reconciliation

- `docs/arch/native-typeinfo-parity.md` parity/oracle discipline, corrected so coverage is not semantic parity.
- `docs/arch/native-checker.md` separate checker manifest requirement.
- `docs/arch/ts-compat-two-mode-model.md` correction-overlay and one-spec rules.

Durable clauses are transferred as digest-bound requirement atoms. Historical path archaeology, obsolete branches, and implementation journals are not copied into the charter. Git history remains the archive.


---

<!-- unified-charter-v2
id=NCK5
name=Framework diagnostic contribution ingress and profile isolation
phase=expansion
train=expansion.native-checker
product=native_checker
kind=implementation
semantic_role=delivery
class=successor
predecessors=NCK1,NCK3,TIF1,IDX0,VIM1
conditional_predecessors=
owner=expansion.native-checker:sole native semantic diagnostic authority and certified-family cutover
conflict_domains=semantic_authority,capability_catalog,vertical_manifest
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
charter=charters/expansion-native-checker/NCK5.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCK5 - Framework diagnostic contribution ingress and profile isolation

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

The internal subblocks below are binding decomposition and review checkpoints. They do not receive independent dispatch, leases, receipts, or deletion ownership unless the pre-scope architect proves that one subblock is independently acceptable or the block crosses a mandatory rescope trigger. In that case, amend the DAG before production mutation rather than treating a train-sized subblock as an implementation checklist.

## Independently acceptable outcome

Implement validated framework semantic and diagnostic contribution ingress, executable template regions, component-contract facts, profile isolation, and Vue/Svelte canaries over the same checker kernel. Core remains framework-neutral and generated TypeScript remains an interoperability surface, not semantic truth.

The current owner is **framework-specific generated TSX checks, component-meta adapters, template diagnostics, and incomplete typed contribution seams**. The final and sole owner is **profile-registered typed framework contributions admitted into the same ProgramAnalysisGraph, relation/call/flow facts, and NCK rule kernel**.

## Architectural role and end state

NCK5 proves that framework templates are equal contributors to one checker rather than separate generated-code checkers. It defines the adapter boundary for regions, bindings, contexts, component contracts, and framework-owned rules while preserving exact profile isolation.

## Expected production surfaces

- `crates/verter_language/src` and universal catalog/profile registration
- `crates/verter_compiler/src/framework` only for typed lowering outputs already owned by framework frontends
- `crates/verter_semantic` and `crates/verter_session` for validated contribution snapshots and checker ingress
- `crates/verter_vue_conformance` and `crates/verter_svelte_conformance` for canary fixtures
- `crates/verter_protocol`/TypeInfo only where component contracts are public through existing universal contracts

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.

## Named APIs and data boundaries

- `FrameworkDiagnosticContributor` or catalog capability equivalent
- `FrameworkRegionContribution`, `TemplateScopeContribution`, and `ComponentContract`
- `InjectedBinding`, `InjectedNarrowingFact`, `InjectedContextualType`, and `InjectedRelationDemand`
- `ProfileSemanticEpoch`, `FrameworkContributionSnapshot`, and `ContributionValidationReceipt`
- `FrameworkDiagnosticRuleDescriptor` for intrinsic framework semantics only

## Exact predecessor contracts

- **NCK1:** consume executable-region and typed-contribution contracts.
- **NCK3:** consume the shared diagnostic rule kernel and no-second-resolver boundary.
- **TIF1:** consume TypeInfo-first component metadata and component-surface authority.
- **IDX0:** consume atomic semantic contribution/index updates and bounded candidate discovery.
- **VIM1:** consume deterministic vertical conformance generation.

External custody: none beyond the package activation boundary.

## Binding architecture

- Framework adapters lower syntax into typed contributions; they do not resolve types, run private relation/call algorithms, or call a framework checker.
- Template control flow and handlers become ExecutableRegionKind::FrameworkRegion with exact authored coordinates and profile identity.
- Generated TSX may remain an external-provider interoperability carrier but cannot be the native semantic fact source.
- Component contracts are framework-neutral: inputs/props, outputs/events, slots/children/content, exposed instance, refs, directives/actions, and lifecycle bindings.
- Framework-owned diagnostics are limited to intrinsic framework semantics. TypeScript-semantic diagnostics use common NCF rules over shared facts.
- Contributions are admitted atomically per profile/source basis and invalidated by exact read sets.
- No core branch on framework name is permitted; capability/catalog dispatch selects contributors.

## Internal subblocks

### NCK5-SB1 - Profile contributor registration

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

### NCK5-SB2 - Template executable-region lowering

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

### NCK5-SB3 - Binding, contextual, and relation contributions

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

### NCK5-SB4 - Framework-neutral component contract

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

### NCK5-SB5 - Intrinsic framework diagnostic rules

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

### NCK5-SB6 - Atomic contribution snapshot and isolation proof

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

## Data, identity, invalidation, and publication laws

- A framework contribution is data plus provenance; it cannot own final TypeScript semantic truth.
- ComponentContract identity includes framework/profile and canonical component identity but remains queryable through common operations.
- Framework region coordinates are authored source coordinates with tagged encoding; generated coordinate identity is not accepted as primary.
- Only complete validated contribution snapshots admit and become visible to Check queries.
- Common and framework-owned diagnostics cannot share the same family/slice authority.

## Migration and cutover

- Start with Vue/Svelte canary regions and component contracts; broader vertical work belongs to generated NCF/VIM rows.
- Keep provider-generated TSX functioning during observation but prevent it from feeding native semantic facts.
- Delete old framework checker paths only after exact native/provider behavior and authored mapping are proven.

## Deletions

- Delete framework-specific native resolver/checker paths displaced by typed contributions.
- Delete duplicate framework component metadata authority after TIF1 contract migration.
- Delete generated-TSX-as-native-truth assumptions from live architecture.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

## Forbidden designs

- Core `if framework == vue/svelte` branches.
- Adapters receiving raw ProjectSemanticDispatch or mutable semantic stores.
- Generated TypeScript/TSX text, regex, or source slicing as native semantic facts.
- Framework-specific copies of relation, call, flow, or diagnostic query stores.
- Cross-profile contribution or cache identity aliasing.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

## Acceptance IDs and discriminating proof

- **NCK5-AC-NEUTRAL:** core checker modules contain no framework-name dispatch and contributors expose no resolver.
- **NCK5-AC-REGIONS:** Vue/Svelte canary template regions carry exact authored identity, scopes, and read sets.
- **NCK5-AC-CONTRACT:** common ComponentContract queries match vertical fixtures with provenance preserved.
- **NCK5-AC-ISOLATION:** profile changes, cancellation, and rapid edits publish no stale or mixed contributions.
- **NCK5-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **NCK5-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **NCK5-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **NCK5-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Performance and bounded work

- One parse and one shallow framework pass per content hash; no checker-triggered rescan.
- Contribution work is demand-selected and zero for non-participating capabilities.
- Target ceiling: 800 production LOC, 8 production files, and 2 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Mandatory rescope and abort conditions

- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. `cargo nextest run -p verter_language -p verter_semantic -p verter_session -p verter_vue_conformance -p verter_svelte_conformance`.
1. Static no-framework-switch/no-resolver adapter guards.
1. Vue/Svelte canary differential, profile-isolation, rapid-edit, and memory-plateau tests.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Consumers and unlocks

- Unlocks NCK6 native publication for framework semantic families.
- Provides framework-authored targets/facts consumed by LSO2/LSO8 and future NCF slices.

## Source reconciliation

- `docs/arch/native-checker.md` framework-agnostic end-state sections.
- `docs/arch/multi-framework-adapters-plan.md` typed contribution and one-resolver invariants.
- TIF1, IDX0, VIM1, and architecture-proof vertical contracts.

Durable clauses are transferred as digest-bound requirement atoms. Historical path archaeology, obsolete branches, and implementation journals are not copied into the charter. Git history remains the archive.


---

<!-- unified-charter-v2
id=NCK6
name=Family-scoped diagnostic authority arbitration and atomic publication
phase=expansion
train=expansion.native-checker
product=native_checker
kind=cutover
semantic_role=delivery
class=successor
predecessors=NCK4,NCK5,H2,H3,COX0,PUB0
conditional_predecessors=
owner=expansion.native-checker:sole native semantic diagnostic authority and certified-family cutover
conflict_domains=diagnostic_action_service,provider_lifecycle,lsp_publication,public_protocol
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
charter=charters/expansion-native-checker/NCK6.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCK6 - Family-scoped diagnostic authority arbitration and atomic publication

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

The internal subblocks below are binding decomposition and review checkpoints. They do not receive independent dispatch, leases, receipts, or deletion ownership unless the pre-scope architect proves that one subblock is independently acceptable or the block crosses a mandatory rescope trigger. In that case, amend the DAG before production mutation rather than treating a train-sized subblock as an implementation checklist.

## Independently acceptable outcome

Implement the sole family-scoped diagnostic authority registry and atomic publication decision layer: exact External/ObserveNative/CertifiedNative/Disabled state, non-publishing shadow comparison, deterministic deduplication, provider/native epoch coordination, and rollback to an explicit prior certified receipt. This block does not integrate individual consumer surfaces.

The current owner is **provider-specific LSP merge branches, ad hoc suppression rules, global provider-enabled flags, and diagnostic message-text deduplication**. The final and sole owner is **one immutable DiagnosticAuthoritySnapshot and one atomic diagnostic publication decision for every project profile, family, and semantic feature slice**.

## Architectural role and end state

NCK6 is the authority cutover block. It prevents a green native implementation from becoming user-visible before certification and prevents external and native producers from publishing the same semantic family. It deliberately stops before LSP/CLI/MCP/NAPI/WASM adapters, which are owned by NCK7.

## Expected production surfaces

- `crates/verter_diagnostics` for authority registry, comparison, deduplication, and publication plans
- `crates/verter_session` for project-scoped immutable authority snapshots and exact basis selection
- `crates/verter_type_runtime` for external observation inputs and provider epoch identity only
- `crates/verter_lsp` publication coordinator only at the shared publication-plan seam, not feature adapters
- `crates/verter_protocol` for authority/certification status exposed under PUB0

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.

## Named APIs and data boundaries

- `DiagnosticAuthorityKey { project_profile, family, feature_slice }`
- `DiagnosticAuthorityState::{External, ObserveNative, CertifiedNative, Disabled}`
- `DiagnosticAuthoritySnapshot`, `DiagnosticAuthorityEpoch`, and immutable transition receipts
- `DiagnosticObservationBatch`, `DiagnosticComparisonResult`, and typed mismatch classes
- `DiagnosticPublicationPlan` and `DiagnosticDedupKey`
- `DiagnosticPromotionRequest`, `DiagnosticPromotionReceipt`, and `DiagnosticRollbackReceipt`

## Exact predecessor contracts

- **NCK4:** consume generated family manifests, exact certification receipts, and canonical oracle comparison.
- **NCK5:** consume validated framework contribution/profile isolation so authority keys never alias across profiles.
- **H2:** consume project-scoped provider bindings and exact provider epochs.
- **H3:** consume latest-basis stale-safe publication and supersession behavior.
- **COX0:** consume per-profile capability participation and dynamic withdrawal.
- **PUB0:** consume typed public outcomes, capability truth, and schema epochs.

External custody: none beyond the package activation boundary.

## Binding architecture

- Authority is keyed by exact project profile, diagnostic family, and semantic feature slice; one global checker/provider boolean is forbidden.
- ObserveNative computes and compares native output but never contributes user-visible diagnostics, fixes, actions, counts, or success status.
- CertifiedNative becomes visible only in the same atomic state transition that suppresses external publication for the exact key.
- Deduplication is by semantic identity and authority, never normalized message text or approximate source range.
- Provider epoch, native implementation receipt, certification receipt, configuration epoch, and authored basis are all explicit transition inputs.
- Rollback names a prior accepted authority snapshot; implicit fallback to whichever provider is available is forbidden.
- A mixed-epoch, stale, cancelled, partial, or NeedInputs producer cannot publish as complete.

## Internal subblocks

### NCK6-SB1 - Immutable authority registry and transition validator

**Independently testable outcome:** Every diagnostic authority key has one exact state and only legal receipt-backed transitions are admitted.

**Architecture:**

- Implement immutable project-scoped authority snapshots with structural keys.
- Define legal transitions and required receipts for External to ObserveNative to CertifiedNative, disablement, and rollback.
- Make configuration/profile changes produce a new authority epoch rather than mutate state in place.

**Expected changes:**

- Replace global provider/native booleans and scattered suppression flags at the authority seam.
- Generate transition tables and static guards from NCK0 authority catalogs.

**Discriminating proof:**

- Illegal transitions, missing receipts, cross-profile reuse, and stale snapshot publication fail closed.
- Incremental reconstruction byte-equals a fresh snapshot for the same inputs.

### NCK6-SB2 - Non-publishing shadow observation

**Independently testable outcome:** ObserveNative produces structured comparison evidence without changing user-visible behavior.

**Architecture:**

- Run native and external owners on the same exact input basis and canonicalize their diagnostic identities.
- Classify missing, extra, wrong-code, wrong-anchor, wrong-related-location, wrong-fix-intent, and completeness mismatches.
- Keep observation results bounded and non-admitted to ordinary diagnostic publication caches.

**Expected changes:**

- Add an observation scheduler lane with cancellation and budgets.
- Persist only bounded certification evidence or aggregate counters explicitly required by NCK4.

**Discriminating proof:**

- Observation on/off produces byte-identical user-visible diagnostics and actions.
- A planted native mismatch is detected while the external result remains the sole published result.

### NCK6-SB3 - Semantic deduplication and composed publication plan

**Independently testable outcome:** The publication plan contains exactly one authoritative diagnostic per semantic identity and preserves distinct legitimate diagnostics.

**Architecture:**

- Construct semantic dedup keys from origin/family/rule/subject/authored anchor/profile/basis.
- Compose parser, semantic, framework, lint, project/configuration, and external classes under their own authority rules.
- Preserve separately owned diagnostics even when wording and ranges coincide.

**Expected changes:**

- Move deduplication out of consumer-specific merge code into the shared diagnostic authority layer.
- Emit a deterministic publication plan with provenance and completeness.

**Discriminating proof:**

- Message wording mutations do not change dedup identity.
- Two different rules at the same anchor survive; duplicate authorities for one key fail.

### NCK6-SB4 - Provider/native epoch coordination

**Independently testable outcome:** A publication plan never combines provider and native results from incompatible bases or epochs.

**Architecture:**

- Join exact source revision, project profile, provider epoch, native authority epoch, and configuration epoch.
- Cancel or discard superseded comparison/publication work on any epoch transition.
- Require exact latest-basis settlement from H3 before publication.

**Expected changes:**

- Thread authority snapshot IDs through shared diagnostic production and publication receipts.
- Remove best-effort merge behavior that accepts whichever batch arrives first.

**Discriminating proof:**

- Race tests with provider restart, edit, config change, and promotion publish only the newest coherent basis.
- No mixed-epoch batch can serialize as complete.

### NCK6-SB5 - Promotion and rollback execution

**Independently testable outcome:** Promotion and rollback are atomic, auditable, and leave neither duplicate nor missing authority.

**Architecture:**

- Validate certification, implementation, profile, provider, and source receipts immediately before transition.
- Publish the new authority snapshot and invalidate displaced result routes atomically.
- Rollback only to an explicitly named accepted snapshot with compatible inputs.

**Expected changes:**

- Implement transition receipts and negative guards against implicit fallback.
- Expose truthful capability/maturity status through PUB0/COX0.

**Discriminating proof:**

- Crash/failure injection at every transition point results in either old or new complete authority, never half-transition.
- Promotion immediately drives external diagnostic work for the certified key to zero.

### NCK6-SB6 - Authority observability and bounded counters

**Independently testable outcome:** Operators and tests can prove which authority ran and how much equivalent work it performed without leaking provider internals into semantic APIs.

**Architecture:**

- Count native/external requests by family/slice/state, comparisons, discarded stale batches, promotions, rollbacks, and dedup decisions.
- Keep counters keyed by stable IDs and bounded cardinality.
- Separate certification/test telemetry from production result identity.

**Expected changes:**

- Add audit events and PER0-compatible work counters.
- Remove consumer-local diagnostic count heuristics used as authority evidence.

**Discriminating proof:**

- Certified warm requests show zero provider diagnostic work for that key.
- Counter reset/restart does not affect semantic or publication identity.

## Data, identity, invalidation, and publication laws

- Authority snapshots are immutable and project/profile scoped; no process-global mutable map is semantic truth.
- Observation results never enter public caches or consumer responses.
- Promotion invalidates displaced producer routes by exact key, not by broad provider shutdown.
- Publication ordering is deterministic after authority selection and semantic deduplication.
- Uncertified families remain externally owned and are reported honestly.

## Migration and cutover

- Introduce the registry in External state for every existing family, proving behavior identity before observation.
- Enable ObserveNative only for accepted NCF slices and compare without publication.
- Promote one canary slice, validate zero duplicates/gaps, then expand only through accepted receipts.
- Leave consumer adapters on the shared publication plan seam for NCK7 migration.

## Deletions

- Delete global checker/provider diagnostic booleans displaced by the exact authority registry.
- Delete message-text and approximate-range deduplication used as an authority substitute.
- Delete provider/native first-arrival merge arbitration for migrated diagnostic classes.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

## Forbidden designs

- Publishing ObserveNative results or fixes.
- Promoting an entire provider/project when only bounded families are certified.
- Implicit rollback to any available provider or stale authority snapshot.
- Consumer-specific authority decisions after the shared publication plan exists.
- Counting diagnostic equality as certification without identity/provenance/completeness comparison.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

## Acceptance IDs and discriminating proof

- **NCK6-AC-STATE:** exhaustive state-machine mutations reject illegal, stale, cross-profile, and receipt-less transitions.
- **NCK6-AC-SHADOW:** observation is user-invisible and detects planted semantic mismatches.
- **NCK6-AC-ATOMIC:** failure injection proves old-or-new atomic authority with no duplicate or missing publication.
- **NCK6-AC-ZERO-PROVIDER:** certified warm slices perform zero external diagnostic work.
- **NCK6-AC-DEDUP:** semantic dedup preserves distinct owners and removes only exact duplicate authority.
- **NCK6-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **NCK6-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **NCK6-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **NCK6-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Performance and bounded work

- External-only state adds no native semantic work; Disabled adds no producer work; ObserveNative cost is explicit and budgeted.
- Authority lookup is allocation-free after snapshot construction and does not scan all families for a leaf request.
- Target ceiling: 800 production LOC, 8 production files, and 2 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Mandatory rescope and abort conditions

- Abort if a producer cannot name exact family/slice/profile/basis identity.
- Abort if promotion cannot atomically suppress the displaced authority.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. Authority-state, epoch-race, observation-invisibility, semantic-dedup, promotion/rollback, and zero-provider-work suites.
1. Provider restart and concurrent edit failure injection under H2/H3 publication semantics.
1. Architecture guard proving consumer adapters cannot independently choose diagnostic authority.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Consumers and unlocks

- Unlocks NCK7 shared consumer integration.
- Supplies the exact diagnostic authority snapshot consumed by language-service conformance when NCK is opened.
- Provides truthful family maturity to COX0 and PUB0.

## Source reconciliation

- `docs/arch/native-checker.md` authority/cutover clauses.
- `docs/arch/provider-*` and diagnostic merge designs containing provider/native arbitration behavior.

Durable clauses are transferred as digest-bound requirement atoms. Historical path archaeology, obsolete branches, and implementation journals are not copied into the charter. Git history remains the archive.


---

<!-- unified-charter-v2
id=NCK7
name=Shared diagnostic service and consumer-surface integration
phase=expansion
train=expansion.native-checker
product=native_checker
kind=implementation
semantic_role=delivery
class=successor
predecessors=NCK6,PUB0
conditional_predecessors=CLI2:when-opened,CLI4:when-opened
owner=expansion.native-checker:sole native semantic diagnostic authority and certified-family cutover
conflict_domains=diagnostic_action_service,public_protocol,lsp_publication,cli_application
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
charter=charters/expansion-native-checker/NCK7.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCK7 - Shared diagnostic service and consumer-surface integration

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

The internal subblocks below are binding decomposition and review checkpoints. They do not receive independent dispatch, leases, receipts, or deletion ownership unless the pre-scope architect proves that one subblock is independently acceptable or the block crosses a mandatory rescope trigger. In that case, amend the DAG before production mutation rather than treating a train-sized subblock as an implementation checklist.

## Independently acceptable outcome

Expose one shared DiagnosticService across LSP, CLI, MCP, NAPI, WASM, and library consumers. Consumers receive authored-coordinate, provenance-complete diagnostic batches from NCK6 and apply only presentation policy; they cannot call semantic/provider engines directly or re-arbitrate authority.

The current owner is **consumer-local diagnostic DTOs, LSP-specific provider merge code, command-local typecheck composition, and inconsistent mapping/drop behavior**. The final and sole owner is **one shared DiagnosticService request/result contract with thin surface adapters and one authored-coordinate projection law**.

## Architectural role and end state

NCK7 completes product integration without mixing it into authority arbitration. It makes diagnostic semantics and completeness identical across consumers while allowing each surface to format, stream, or serialize the same authoritative batch appropriately.

## Expected production surfaces

- `crates/verter_diagnostics` and `crates/verter_session` for the shared service and project snapshot access
- `crates/verter_protocol` for versioned public requests/results and stable IDs
- `crates/verter_lsp` for diagnostics publication and code-action references
- `crates/verter_mcp_server`, `crates/verter_napi`, `crates/verter_wasm`, and FFI/public packages for thin adapters
- `packages/binary-launcher`, `packages/verter-lsp`, and CLI application services when their conditional predecessors are opened

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.

## Named APIs and data boundaries

- `DiagnosticRequest { scope, profile, demand, basis, cancellation, budget }`
- `DiagnosticService::check_region`, `check_file`, and `check_project_rules`
- `DiagnosticBatch { basis, completeness, diagnostics, authority_snapshot }`
- `AuthoredDiagnostic`, `AuthoredRelatedLocation`, `DiagnosticProofRef`, and `DiagnosticFixIntentRef`
- `DiagnosticSurfaceAdapter` as serialization/presentation only, not semantic extension
- `DiagnosticStreamCursor` for bounded project/watch enumeration where supported

## Exact predecessor contracts

- **NCK6:** consume the exact authority snapshot, publication plan, semantic deduplication, and promotion law.
- **PUB0:** consume common public request/result outcomes, schema epochs, cancellation, budgets, and capability truth.
- **CLI2:when-opened:** when opened, integrate the Verter-native typecheck command as a thin DiagnosticService consumer.
- **CLI4:when-opened:** when opened, integrate CLI LSP/MCP adapters without command-local diagnostic semantics.

External custody: none beyond the package activation boundary.

## Binding architecture

- Core diagnostic batches are authored-coordinate results; generated/provider coordinates do not cross the service boundary.
- Every surface observes the same semantic diagnostics, authority state, basis, completeness, related locations, proof refs, and fix-intent refs.
- Presentation fields such as LSP severity tags, terminal colors, JSON layout, progress UI, and streaming framing are adapter policy.
- A surface cannot convert NeedInputs, unsupported, cancelled, stale, or partial into empty complete success.
- Fixes remain typed intents/references until LSO8/LRA0 validates an authored edit transaction.
- Project checks are bounded coordinators with streaming/pagination; a consumer cannot request hidden unbounded workspace work.
- Provider calls and semantic queries occur inside the shared service/authority layer only.

## Internal subblocks

### NCK7-SB1 - Shared service request and scope contract

**Independently testable outcome:** All consumers request the same region/file/project-rule diagnostic operations with exact basis, demand, cancellation, and budgets.

**Architecture:**

- Define scope selectors without LSP URI or CLI presentation fields.
- Require exact project/profile/source basis and capability availability.
- Model project-rule enumeration as bounded pages/streams with explicit completeness.

**Expected changes:**

- Add the shared service facade over NCK6 publication plans and NCK2 queries.
- Replace consumer-local project loading and diagnostic plan selection.

**Discriminating proof:**

- Equivalent requests from two surfaces produce the same core request identity.
- Unbounded or ambiguous project selection is rejected rather than silently choosing the first project.

### NCK7-SB2 - Authored-coordinate diagnostic projection

**Independently testable outcome:** Every returned primary and related location is mapped to exact authored source or refused with typed provenance loss.

**Architecture:**

- Use UAI0/TCM authored mapping and source lineage for native, framework, and external diagnostics.
- Preserve source unit, profile, revision, mapping chain, and anchor confidence.
- Drop or return a typed incomplete result for unmappable provider artifacts; never synthesize 0:0 or nearest ranges.

**Expected changes:**

- Centralize diagnostic range projection before consumer adapters.
- Delete LSP-only range fallbacks and duplicated carrier mapping branches.

**Discriminating proof:**

- UTF-8/UTF-16/CRLF/emoji/embedded carrier cases round-trip exact authored spans.
- Stale mapper/source revisions are rejected and cannot publish.

### NCK7-SB3 - LSP diagnostics and code-action reference adapter

**Independently testable outcome:** LSP publication consumes shared authored batches and exposes exact code-action references without rechecking or remapping semantics.

**Architecture:**

- Translate authored spans through negotiated position encoding only at the LSP edge.
- Publish latest-basis batches under H3 and clear only capabilities withdrawn by COX0.
- Resolve fix-intent references through LRA0/LSO8 rather than embedding unchecked workspace edits.

**Expected changes:**

- Route foreground/background diagnostic publication through one adapter.
- Delete provider/native merge and authority selection from LSP code.

**Discriminating proof:**

- Foreground and background paths publish identical core diagnostic identities.
- Dynamic capability withdrawal cancels work and clears only owned diagnostics.

### NCK7-SB4 - CLI, MCP, NAPI, WASM, and library adapters

**Independently testable outcome:** Non-LSP surfaces preserve core semantics and report unavailable inputs/capabilities truthfully.

**Architecture:**

- Define stable JSON/protobuf/FFI projections from PUB0 without surface-specific semantic DTOs.
- CLI typecheck writes nothing and uses explicit project/reference/watch selection.
- WASM/MCP report NeedInputs when filesystem/provider/project services are unavailable.

**Expected changes:**

- Replace command-local or binding-local diagnostic composition.
- Generate bindings and compatibility tests from the public schema.

**Discriminating proof:**

- Cross-surface differential fixtures match diagnostic identity, basis, completeness, provenance, related/fix refs.
- A missing input never becomes empty success.

### NCK7-SB5 - Watch, cancellation, streaming, and supersession

**Independently testable outcome:** Long-running and watch consumers receive deterministic latest-basis batches without stale cache admission or retained-work growth.

**Architecture:**

- Use cancellation/deadline/budget tokens through region/file/project coordinators.
- Supersede in-flight work on source/profile/authority/provider epoch changes.
- Bound stream cursors and release snapshots after completion/cancellation.

**Expected changes:**

- Unify watch and one-shot paths over the same service.
- Remove polling/sleep readiness and consumer-owned debounce semantics from diagnostic correctness.

**Discriminating proof:**

- Rapid edit/revert/provider restart tests publish only the latest basis.
- Cancelled project streams release retained regions/results and admit nothing partial.

### NCK7-SB6 - Consumer route inventory and migration proof

**Independently testable outcome:** Every public diagnostic consumer is known, migrated, and structurally prevented from bypassing the shared service.

**Architecture:**

- Generate a call-site inventory for direct provider diagnostics, native checker calls, and legacy DTO construction.
- Migrate one surface at a time behind behavior characterization, then delete bypasses.
- Keep optional conditional consumers zero-work and unclaimed when unopened.

**Expected changes:**

- Add static architecture guards and generated consumer matrix.
- Record exact deletions and residual unsupported surfaces.

**Discriminating proof:**

- Planting a direct provider/checker call in a consumer crate fails the guard.
- The inventory reaches zero unexplained bypasses before NCK8.

## Data, identity, invalidation, and publication laws

- Core result identity is independent of surface encoding and presentation.
- Authored span projection validates the exact source/mapping basis used to obtain the range.
- Consumers may filter only explicitly policy-filterable classes under a named capability/configuration rule; they cannot suppress semantic families silently.
- Project stream cursors are scoped to an immutable basis and become stale on any authority/source/profile change.
- No consumer adapter owns semantic caching; it may cache serialization only by full core result identity.

## Migration and cutover

- Characterize each consumer surface against existing behavior and identify intentional corrections.
- Introduce the shared service with LSP as first consumer, then CLI/MCP/NAPI/WASM/library surfaces.
- Delete direct provider/native merge paths immediately after the last consumer moves.
- Keep unopened conditional CLI predecessors outside acceptance and prove zero hidden integration work.

## Deletions

- Delete consumer-local diagnostic authority arbitration, semantic deduplication, and provider/native merge logic.
- Delete Range::default/0:0/nearest-position diagnostic fallbacks and surface-specific semantic DTOs.
- Delete command-local project/checker construction displaced by shared application/service integration.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

## Forbidden designs

- A surface adapter calling tsgo/tsserver or native Check queries directly.
- LSP URI/Position, terminal formatting, or provider handles in core diagnostic results.
- Embedding raw text edits in diagnostics instead of typed fix-intent references.
- Converting unavailable/partial/stale results to empty success.
- Hidden full-workspace checks on file-open, hover, completion, or unrelated leaf operations.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

## Acceptance IDs and discriminating proof

- **NCK7-AC-SURFACES:** all opened consumers match core diagnostic identity, basis, completeness, provenance, related locations, and fix refs.
- **NCK7-AC-AUTHORED:** no public diagnostic leaves the service with generated coordinates or unvalidated mapping basis.
- **NCK7-AC-NO-BYPASS:** static inventory proves consumer crates cannot call diagnostic providers/resolvers directly.
- **NCK7-AC-WATCH:** watch/stream cancellation and supersession publish only latest complete batches and release retained state.
- **NCK7-AC-NEEDINPUTS:** unavailable surfaces return typed NeedInputs/unsupported, never empty complete success.
- **NCK7-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **NCK7-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **NCK7-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **NCK7-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Performance and bounded work

- Thin adapters add no parse/resolve/provider/checker work and perform bounded serialization/allocation proportional to returned diagnostics.
- Repeated serialization may cache only by full result/basis/schema identity and must plateau in retained bytes.
- Target ceiling: 800 production LOC, 8 production files, and 2 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Mandatory rescope and abort conditions

- Abort if any consumer requires a surface-specific semantic result not representable under PUB0; amend PUB0 rather than forking.
- Abort if exact authored projection is unavailable and a fallback location is proposed.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. Cross-surface differential matrix, authored mapping/encoding tests, watch/cancel/supersession tests, and no-bypass architecture guard.
1. LSP foreground/background equivalence and dynamic capability withdrawal tests.
1. CLI/MCP/NAPI/WASM NeedInputs and schema-compatibility tests for every opened consumer.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Consumers and unlocks

- Unlocks NCK8 terminal closure.
- Provides the checker diagnostic service consumed conditionally by LSO9 and future verticals.
- Supports CLI typecheck without claiming full TypeScript engine retirement.

## Source reconciliation

- `docs/arch/native-checker.md` public query/result clauses.
- `docs/arch/ide-error-recovery-design.md` diagnostic publication and strict mapping clauses.
- Legacy LSP/provider diagnostic merge and CLI typecheck plans classified by the reconciliation catalog.

Durable clauses are transferred as digest-bound requirement atoms. Historical path archaeology, obsolete branches, and implementation journals are not copied into the charter. Git history remains the archive.


---

<!-- unified-charter-v2
id=NCK8
name=Native checker terminal and displaced-authority deletion
phase=expansion
train=expansion.native-checker
product=native_checker
kind=terminal
semantic_role=delivery
class=successor
predecessors=NCK7,NCKF0,PER0,UAO0,UAP0,BR0
conditional_predecessors=
owner=expansion.native-checker:sole native semantic diagnostic authority and certified-family cutover
conflict_domains=semantic_authority,diagnostic_action_service,performance_evidence,program_authority
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
release_gating=product
source_refs=live:docs/arch/refactor/rev11/sources/legacy-arch-reconciliation.md
external_requirements=
activation_gate=ORC0
charter=charters/expansion-native-checker/NCK8.md
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCK8 - Native checker terminal and displaced-authority deletion

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

The internal subblocks below are binding decomposition and review checkpoints. They do not receive independent dispatch, leases, receipts, or deletion ownership unless the pre-scope architect proves that one subblock is independently acceptable or the block crosses a mandatory rescope trigger. In that case, amend the DAG before production mutation rather than treating a train-sized subblock as an implementation checklist.

## Independently acceptable outcome

Close the native checker product only after the required generated diagnostic slices, framework ingress, authority promotions, shared consumer integrations, performance/cancellation/memory proofs, and legacy authority deletion are complete on one exact terminal basis. This block adds no new diagnostic semantics.

The current owner is **accepted NCK/NCF nodes plus residual displaced diagnostic routes, stores, tests, flags, and legacy architecture documents**. The final and sole owner is **the promoted native checker product receipt, exact certified-family authority snapshot, and structurally enforced absence of displaced diagnostic authority**.

## Architectural role and end state

NCK8 is a proof, deletion, and promotion terminal. Any missing diagnostic algorithm, unsupported required family, semantic mismatch, or public-contract gap reopens its owning NCF/NCK predecessor; terminal cleanup may not patch semantics locally.

## Expected production surfaces

- `docs/arch/refactor/rev11/authority`, catalogs, generated manifests, receipts, and legacy disposition
- `crates/verter_session`, `crates/verter_semantic`, `crates/verter_diagnostics`, `crates/verter_lsp`, and CLI only for bounded final cutover/deletion
- `crates/verter_bench` and performance evidence for checker latency, work, allocation, and RSS
- repository-wide architecture guards for deleted diagnostic authority paths

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.

## Named APIs and data boundaries

- `NativeCheckerProductReceipt` and exact required/residual family inventory
- `LegacyDiagnosticAuthorityDeletionManifest`
- `CheckerSurfaceEquivalenceReceipt` across CLI/LSP/public consumers
- `CheckerPerformanceReceipt` and long-churn memory evidence

## Exact predecessor contracts

- **NCK7:** consume the shared consumer service and zero-bypass surface integration.
- **NCKF0:** consume the machine-generated required-family convergence receipt, exact manifest/predecessor bijection, current certification/promotion chains, provider-zero-work, and per-slice performance/admission closure.
- **PER0:** consume equivalent-work, latency, allocation, cancellation, and RSS terminal methodology.
- **UAO0:** consume activation, TypeInfo, index, and performance contract lock.
- **UAP0:** consume capability, coexistence, diagnostic/action, and public contract lock.
- **BR0:** consume successor product promotion authority and exact release law.

External custody: none beyond the package activation boundary.

## Binding architecture

- Terminal completeness is manifest-derived. NCK8 cannot declare success by sampling or percentage.
- External residual families are allowed only when explicitly classified as product exclusions or future requirements with honest capability reporting.
- No semantic algorithm work is hidden in the terminal. Any missing rule/family opens or amends an NCF node.
- Every displaced route/store/guard/doc is deleted or explicitly retained with sole ownership and rationale.
- Cross-surface equivalence compares semantic diagnostic identity/basis, not editor formatting.
- Performance acceptance uses equivalent work, first/warm check latency, cancellation waste, allocations, and long-churn RSS.

## Internal subblocks

### NCK8-SB1 - Manifest completeness and residual classification

**Independently testable outcome:** Every required family slice has an accepted implementation/certification/promotion receipt or an explicit product exclusion.

**Architecture:**

- Compute completeness from the canonical manifest and authority table.
- Reject wildcard deferrals and unowned residual rows.
- Record future external-owned scope separately from completed native product claims.

**Expected changes:**

- Generate terminal completeness report and machine receipt.
- Open amendments for any missing independently acceptable work before proceeding.

**Discriminating proof:**

- Planted missing/duplicate/unpromoted required slice blocks terminal.
- Report is reproducible from authority inputs.

### NCK8-SB2 - Displaced authority and store deletion

**Independently testable outcome:** No migrated family has an old producer, cache, merge path, or fallback capable of publishing.

**Architecture:**

- Sweep semantic, session, LSP, provider, framework, and command paths by registered family owners.
- Delete old stores and compatibility branches after final consumers move.
- Retain external provider machinery only for explicitly external families and other language-service capabilities.

**Expected changes:**

- Apply exact deletion manifest and negative guards.
- Remove stale docs/tests/config flags tied to deleted authority.

**Discriminating proof:**

- Planting any deleted route fails architecture tests.
- No migrated family produces provider diagnostic work in runtime counters.

### NCK8-SB3 - Cross-surface semantic equivalence

**Independently testable outcome:** CLI, LSP, MCP, NAPI/WASM/public surfaces observe equivalent native semantic diagnostics and truthful outcomes.

**Architecture:**

- Compare diagnostic identity, basis, completeness, provenance, and related/fix refs.
- Allow presentation-specific formatting only after core equivalence.
- Verify unavailable inputs yield NeedInputs rather than empty success.

**Expected changes:**

- Generate surface matrix fixtures and receipts.
- Fix only bounded adapter discrepancies; semantic gaps reopen NCF work.

**Discriminating proof:**

- Differential matrix passes for all available surfaces/profiles.
- A surface-specific semantic DTO or dropped provenance blocks terminal.

### NCK8-SB4 - Performance, cancellation, and memory terminal

**Independently testable outcome:** The checker is production-bounded under cold, warm, incremental, churn, cancellation, and parallel load.

**Architecture:**

- Measure equivalent fact/rule/query work, allocations, retained bytes, latency distributions, and provider avoidance.
- Test repeated edits, project open/close, profile transitions, and cancelled workspace checks.
- Require no unbounded result/proof/contribution retention.

**Expected changes:**

- Capture checker performance receipt under PER0 methodology.
- Reopen the owning implementation node for unexplained regressions; do not micro-optimize blindly in NCK8.

**Discriminating proof:**

- Long-churn memory plateaus and project teardown releases storage.
- Warm certified families perform zero provider diagnostic work.

### NCK8-SB5 - Legacy architecture reconciliation and deletion

**Independently testable outcome:** All durable legacy checker/type-parity clauses are in Rev11 authority and obsolete files are removed.

**Architecture:**

- Validate exact blob-SHA disposition for every legacy path.
- Ensure no live authority references deleted files.
- Keep product/user docs outside `docs/arch` where appropriate.

**Expected changes:**

- Delete classified legacy files in the same accepted amendment.
- Enable permanent guard forbidding new docs/arch files outside Rev11.

**Discriminating proof:**

- Repository tree contains no unclassified live legacy architecture.
- Source-atom coverage remains complete after deletion.

### NCK8-SB6 - Native checker product receipt and promotion

**Independently testable outcome:** The product is promoted with exact scope, residuals, evidence, and no hidden claim of full TypeScript replacement beyond certified families.

**Architecture:**

- Bind manifest digest, authority snapshot, surface/performance/deletion receipts, and review verdicts.
- State remaining external families and runtime provider uses honestly.
- Separate checker completion from full language-service/provider retirement.

**Expected changes:**

- Emit immutable product receipt and update capability/maturity matrices.
- Do not delete TypeScript provider capabilities still owned by LSO/EPR or external residual families.

**Discriminating proof:**

- Receipt invalidates on any authority/source/evidence change.
- Public capability claims match the exact certified scope.

## Data, identity, invalidation, and publication laws

- NCK8 may not add a new diagnostic algorithm, rule family, or semantic fact authority.
- Residual external ownership is explicit and capability-visible; it is not a failure if product scope says so.
- A product receipt names exact manifest and authority epochs and is immutable.
- Deleting provider diagnostic paths does not imply deleting provider completion/navigation capabilities.

## Migration and cutover

- Run terminal only after required NCF nodes and NCK6 promotions are accepted.
- Perform bounded cleanup/deletion in one landing-frozen candidate with complete negative guards.
- If final sweeps discover semantic gaps, stop and open owning NCF/NCK amendments.

## Deletions

- Delete all displaced checker diagnostic producers, stores, merge paths, flags, and legacy docs named in the terminal manifest.
- Delete stale parity ledgers and ignored-test mechanisms replaced by NCK4 authority.
- Delete any claim that NCK8 retires the entire TypeScript engine unless separate LSO/EPR/provider retirement authority exists.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

## Forbidden designs

- Adding missing semantic features in the terminal block.
- Treating sampled parity, green coverage, or message counts as full certification.
- Deleting provider capabilities still owned outside diagnostic families.
- Accepting unexplained performance or memory regressions as cleanup noise.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

## Acceptance IDs and discriminating proof

- **NCK8-AC-MANIFEST:** all required slices have exact accepted implementation, certification, and promotion receipts.
- **NCK8-AC-DELETION:** every displaced diagnostic route/store/doc is absent and structurally rejected.
- **NCK8-AC-SURFACES:** semantic diagnostic results and outcomes are equivalent across supported public surfaces.
- **NCK8-AC-TERMINAL-PERF:** cold/warm/incremental/cancel/churn work, allocation, latency, and RSS satisfy PER0 evidence.
- **NCK8-AC-HONESTY:** residual external ownership and non-checker provider uses are explicitly documented.
- **NCK8-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **NCK8-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **NCK8-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **NCK8-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Performance and bounded work

- Terminal performance thresholds must be replacement/equivalent-work thresholds ratified by PER0, not arbitrary zero-regression assertions when capability work differs.
- Target ceiling: 300 production LOC, 3 production files, and 1 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Mandatory rescope and abort conditions

- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. Full native checker manifest/authority/source/deletion validation.
1. Canonical cross-surface, provider-avoidance, incremental/fresh, cancellation, and long-churn test matrix.
1. Configured architecture-3 review and product promotion receipt validation on the exact candidate.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Consumers and unlocks

- Promotes the native checker product for certified families.
- Provides a stable diagnostic service for CLI, language-service conformance, lint/fix composition, and future framework verticals.
- Does not by itself unlock full TypeScript engine retirement.

## Source reconciliation

- All NCK/NCF authority and `legacy-arch-disposition.toml` entries targeting native checker/type-parity docs.
- PER0, PUB0, UAO0, UAP0, and BR0 terminal contracts.

Durable clauses are transferred as digest-bound requirement atoms. Historical path archaeology, obsolete branches, and implementation journals are not copied into the charter. Git history remains the archive.


# Module `expansion-language-service`


---

<!-- unified-charter-v2
id=LSO0
name=Authored-coordinate semantic operation constitution
phase=expansion
train=expansion.language-service
product=language_service
kind=constitution
semantic_role=delivery
class=successor
predecessors=UAI0,UAP0,TCM4,H3
conditional_predecessors=
owner=expansion.language-service:one authored-coordinate semantic-operation and edit-transaction authority
conflict_domains=public_protocol,mapping_geometry,source_lineage
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
charter=charters/expansion-language-service/LSO0.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# LSO0 - Authored-coordinate semantic operation constitution

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

The internal subblocks below are binding decomposition and review checkpoints. They do not receive independent dispatch, leases, receipts, or deletion ownership unless the pre-scope architect proves that one subblock is independently acceptable or the block crosses a mandatory rescope trigger. In that case, amend the DAG before production mutation rather than treating a train-sized subblock as an implementation checklist.

## Independently acceptable outcome

Ratify one framework-neutral, provider-neutral, authored-coordinate semantic-operation constitution for navigation, occurrences, rename, completion, presentation, and edits. This block establishes ownership and public contracts only; it implements no feature execution.

The current owner is **feature-specific LSP handlers, generated-TSX mapping branches, provider-specific DTOs, framework-specific target heuristics, and separate rename/import edit paths**. The final and sole owner is **one language-service operation constitution, one authored target/provenance vocabulary, and one typed authored edit-intent boundary consumed by thin LSP/CLI/editor adapters**.

## Architectural role and end state

LSO0 defines the boundary that prevents language-service features from becoming a collection of unrelated LSP patches. Semantic meaning is requested and returned in authored coordinates; providers and generated projections are implementation details behind typed adapters.

## Expected production surfaces

- `crates/verter_identity/src` for stable operation, target, occurrence, and transaction identities
- `crates/verter_protocol` for versioned request/result/outcome and capability contracts
- `crates/verter_session` for operation coordination over immutable project snapshots
- `crates/verter_span` and mapping owners for authored-coordinate basis types
- `crates/verter_lsp` only as a future protocol adapter, not semantic authority

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.

## Named APIs and data boundaries

- `SemanticOperationKind`, `SemanticOperationRequest`, `SemanticOperationOutcome`, and `OperationBasis`
- `AuthoredTargetId`, `AuthoredTarget`, `TargetKind`, and `TargetProvenance`
- `OccurrenceId`, `OccurrenceRole`, and `OccurrenceSet`
- `EditIntentId`, `AuthoredEditIntent`, `EditSafetyClass`, and `EditPrecondition`
- `OperationCapability`, `OperationMaturity`, and exact profile participation masks
- `ProviderObservationRef` as opaque provenance only, never provider JSON or routing state

## Exact predecessor contracts

- **UAI0:** consume exact source, carrier, parser, coordinate, and identity contracts.
- **UAP0:** consume capability/coexistence/public contract lock and authored diagnostic/action ownership.
- **TCM4:** consume the atomic TypeScript mapper/provider activation contract and exact generated-to-authored basis.
- **H3:** consume stale-safe latest-basis publication and supersession semantics.

External custody: none beyond the package activation boundary.

## Binding architecture

- The core operates on authored source units, semantic subjects, and exact profile identity; LSP URI/Position, TSX paths, and editor client state are edge concerns.
- One canonical target/provenance graph serves definition, type-definition, implementation, references, hierarchy, rename, hover links, and completion resolve.
- A provider may contribute typed observations but does not define core request/result shapes.
- Every edit-producing feature emits typed edit intents first; only LSO8 validates and materializes an authored transaction.
- No feature accepts nearest-position, current-file-mapper fallback, Range::default, 0:0, or fabricated source anchors.
- Capabilities are profile/document-selector scoped and dynamically truthful under COX0; installing one conflicting editor extension must not disable unrelated operations.
- Interactive leaf operations are demand-bounded and cannot trigger an implicit whole-workspace crawl.

## Internal subblocks

### LSO0-SB1 - Operation taxonomy and authority matrix

**Independently testable outcome:** Every language-service operation has one owner, one result domain, and an explicit provider/native/framework composition law.

**Architecture:**

- Define navigation, occurrence, rename, completion, presentation, and edit transaction operation families.
- Name semantic authority versus observation/provider authority for each operation.
- Separate workspace discovery/index planning from authoritative semantic resolution.

**Expected changes:**

- Add a machine-readable operation catalog and generated capability table.
- Classify every existing LSP/custom-method feature and legacy design clause.

**Discriminating proof:**

- Unclassified, duplicate-owned, or cyclic operation authority fails validation.
- Generated catalog order is deterministic and complete against registered operations.

### LSO0-SB2 - Typed request, outcome, and basis vocabulary

**Independently testable outcome:** No operation can confuse no-result, ambiguity, missing input, cancellation, staleness, or unsupported capability.

**Architecture:**

- Define exact operation basis and typed outcomes including complete empty success, ambiguous, NeedInputs, unsupported, cancelled, stale, and superseded.
- Carry budgets, cancellation, demand, source/profile identity, and capability epoch.
- Keep presentation and provider fields out of the core request.

**Expected changes:**

- Amend PUB0 schema and compatibility rules.
- Reserve operation-specific result domains without activating feature routes.

**Discriminating proof:**

- Round-trip and mutation tests discriminate every operational outcome.
- A missing workspace/project/provider input cannot serialize as complete empty success.

### LSO0-SB3 - Authored target and provenance constitution

**Independently testable outcome:** Every semantic target has a stable identity and validates the exact derivation route to authored source.

**Architecture:**

- Define target kinds for real declaration, component anchor, external declaration, synthetic semantic anchor, and unresolved/ambiguous outcomes.
- Separate source hash/revision provenance from generated compile snapshot provenance.
- Define alias/barrel/augmentation and framework-contribution provenance edges.

**Expected changes:**

- Ratify canonical identity and deduplication rules consumed by LSO2.
- Ban suffix-preference and URI/range-only deduplication.

**Discriminating proof:**

- Two derivation paths to one symbol canonicalize to one target; two different symbols at one range remain distinct.
- Stale or wrong-file mapping provenance is rejected.

### LSO0-SB4 - Occurrence and edit-intent constitution

**Independently testable outcome:** References/rename/fixes/imports share typed occurrence roles and cannot emit unchecked raw edits.

**Architecture:**

- Define declaration/read/write/type/import/export/tag/attribute/string-literal and framework-specific extensible occurrence roles.
- Define edit intents independent of final text edit encoding and exact preconditions.
- Classify safe, suggested, and unsafe transaction intents under LRA0.

**Expected changes:**

- Add common occurrence and edit-intent schemas.
- Require every edit-producing operation to depend on LSO8 materialization.

**Discriminating proof:**

- Static guards reject raw workspace edits from semantic operation modules.
- Role mutation tests prove rename policy changes only intended occurrences.

### LSO0-SB5 - Capability and coexistence law

**Independently testable outcome:** Operation availability is exact, profile-scoped, dynamically withdrawable, and zero-work when disabled.

**Architecture:**

- Bind operation capabilities to catalog/profile/document selectors and maturity receipts.
- Define Disabled, WorkspaceOnly, and Full participation consequences per operation.
- Require dynamic cancellation and stale-result withdrawal on capability changes.

**Expected changes:**

- Amend COX0/PUB0 capability generation inputs.
- Remove global all-or-nothing language-service enablement from authority.

**Discriminating proof:**

- Coexistence fixtures withdraw only overlapping operations.
- Disabled operations perform zero parse/index/provider/semantic work.

### LSO0-SB6 - Legacy source transfer and critical guards

**Independently testable outcome:** The constitution captures all durable navigation/recovery/completion/edit clauses before legacy documents are deleted.

**Architecture:**

- Bind goto-definition, completion-resolve, import placement, recovery, and global-component clauses to exact LSO nodes.
- Name guards for authored-only public results, one target graph, no raw edits, no mapping fallback, and no consumer semantic forks.

**Expected changes:**

- Populate legacy reconciliation and disposition catalogs.
- Register future guard names and source atom applicability.

**Discriminating proof:**

- Deleting a legacy file without complete target atoms fails.
- A planted forbidden fallback or parallel feature engine fails authority validation.

## Data, identity, invalidation, and publication laws

- Core operation identity is rooted in project/profile/source basis, semantic subject, operation kind, demand, and capability epoch; presentation encoding is not semantic identity.
- Generated coordinates, provider handles, and editor DTOs may exist only inside typed adapters and never become public semantic truth.
- Cancellation, stale, superseded, partial, NeedInputs, unsupported, and ambiguous outcomes remain distinct and are never collapsed to empty success.
- All returned targets and edits carry exact authored provenance and validate the snapshot/mapping chain used to derive them.
- Operation and target IDs are stable across serialization and independent of message wording or LSP encoding.
- Indexes return bounded candidates and memberships; they never return authoritative semantic operation answers.
- A provider observation must name provider epoch and input basis, but provider epoch is not part of native semantic subject identity.

## Migration and cutover

- Land the constitution and amend PUB0/COX0/LRA0 contracts before implementing operations.
- Inventory existing feature routes and classify each as retained observation, migration source, or deletion target.
- Do not move any user-visible feature in LSO0.

## Deletions

- Delete legacy architecture prose only after requirement atoms target LSO0-LSO10.
- Delete proposed per-feature mapping tables, raw-edit authorities, and provider-specific core DTO designs from live authority.
- Delete claims that V3/source maps alone are semantic target identity.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

## Forbidden designs

- LSP or provider DTOs in the core operation API.
- Feature-specific target identity, mapper fallback, or edit application engines.
- Generated TSX/source text as semantic truth.
- Global enable/disable flags in place of profile-scoped capabilities.
- Raw edits returned before authored transaction validation.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

## Acceptance IDs and discriminating proof

- **LSO0-AC-CATALOG:** every registered operation and legacy clause has exact ownership and result-domain classification.
- **LSO0-AC-AUTHORED:** public contracts contain no generated coordinate, TSX path, provider handle, or LSP position.
- **LSO0-AC-OUTCOMES:** typed outcome mutations distinguish empty success from every refusal/partial state.
- **LSO0-AC-EDIT-LAW:** every edit producer is statically required to route through LSO8.
- **LSO0-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **LSO0-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **LSO0-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **LSO0-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Performance and bounded work

- The constitution declares explicit zero-work behavior and bounded-demand axes for every operation family.
- Target ceiling: 0 production LOC, 0 production files, and 0 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Mandatory rescope and abort conditions

- Abort if one operation cannot name a sole semantic owner and sole public result owner.
- Abort if a legacy feature requires approximate coordinates to preserve current behavior.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. Authority/catalog/source coverage validation.
1. Schema and negative architecture tests for authored-only contracts, typed outcomes, one target graph, and edit-intent routing.
1. Generated coexistence/capability table determinism test.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Consumers and unlocks

- Unlocks LSO1-LSO8 implementation.
- Defines the operation vocabulary consumed by VIM/COX/PUB and future editor clients.
- Provides the contract needed to delete the legacy feature-design corpus.

## Source reconciliation

- `docs/arch/goto-definition-architecture-decision.md`.
- `docs/arch/provider-completion-resolve-design.md`.
- `docs/arch/ide-error-recovery-design.md` and framework import-placement designs.

Durable clauses are transferred as digest-bound requirement atoms. Historical path archaeology, obsolete branches, and implementation journals are not copied into the charter. Git history remains the archive.


---

<!-- unified-charter-v2
id=LSO1
name=Tolerant carrier recovery and two-rail syntax/semantic diagnostics
phase=expansion
train=expansion.language-service
product=language_service
kind=implementation
semantic_role=delivery
class=successor
predecessors=LSO0,PAR0,EMB0,B2,LRA0
conditional_predecessors=
owner=expansion.language-service:one authored-coordinate semantic-operation and edit-transaction authority
conflict_domains=carrier_parser,mapping_geometry,diagnostic_action_service,lsp_publication
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
charter=charters/expansion-language-service/LSO1.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# LSO1 - Tolerant carrier recovery and two-rail syntax/semantic diagnostics

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

The internal subblocks below are binding decomposition and review checkpoints. They do not receive independent dispatch, leases, receipts, or deletion ownership unless the pre-scope architect proves that one subblock is independently acceptable or the block crosses a mandatory rescope trigger. In that case, amend the DAG before production mutation rather than treating a train-sized subblock as an implementation checklist.

## Independently acceptable outcome

Implement tolerant Vue/Svelte carrier recovery and a two-rail diagnostic model that preserves stable authored tokens, regions, mappings, and semantic work during recoverable edits without inventing semantic facts or weakening strict mapping.

The current owner is **parser/compiler bailouts, aggressive generated-token repair, LSP diagnostic drop behavior, and carrier-specific recovery flags**. The final and sole owner is **one RecoverySnapshot contract with native syntax diagnostics, minimal capability-tagged synthetic repair, stable authored mappings, and exact per-region semantic participation**.

## Architectural role and end state

LSO1 makes broken carriers behave like broken source files: syntax errors remain visible while unaffected semantic diagnostics and operations continue. Recovery is a parser/lowering concern, not a resolver special case.

## Expected production surfaces

- `crates/verter_parser` and framework parser outputs
- `crates/verter_compiler` IDE projection/recovery chunks
- `crates/verter_session` native syntax diagnostic rail and recovery snapshot storage
- `crates/verter_lsp` only for consuming authored diagnostics/publication
- `crates/verter_span` and mapping contracts

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.

## Named APIs and data boundaries

- `RecoverySnapshot`, `RecoveryRegionState`, and `RecoveryParticipation`
- `NativeSyntaxDiagnostic` with authored spans and stable parser identity
- `SyntheticRepairChunk` with verification/navigation/completion capability flags
- `RecoveryBoundary`, `MissingNodeAnchor`, and exact source mapping metadata
- `RecoveredCarrierResult::{Usable, Degraded, Catastrophic}`

## Exact predecessor contracts

- **LSO0:** consume authored operation and typed outcome laws.
- **PAR0:** consume parser ownership and source lineage.
- **EMB0:** consume embedded-codec and exact authored map-chain contracts.
- **B2:** consume accepted framework parsing/recovery diagnostics and stable identities.
- **LRA0:** consume diagnostic provenance and suppression ownership.

External custody: none beyond the package activation boundary.

## Binding architecture

- Recoverable syntax diagnostics and IDE semantic surface availability are separate channels.
- Authored user tokens remain mapped and semantically visible whenever parser recovery can preserve them; synthetic repair text is unmapped and capability-tagged.
- Native syntax diagnostics are produced from parser errors in authored coordinates independently of provider output.
- Strict mapping remains strict; synthetic provider diagnostics are suppressed by explicit chunk metadata or dropped, never heuristically re-anchored.
- Recovery incompleteness causes fail-open usage analysis and ReturnOnly semantic results where completeness cannot be proven.
- Catastrophic failure is explicit and does not erase previously valid diagnostics without a typed stale/NeedInputs outcome.

## Internal subblocks

### LSO1-SB1 - Native syntax diagnostic rail

**Independently testable outcome:** Recoverable script/template/parser errors become stable authored diagnostics for all consumers.

**Architecture:**

- Harvest recoverable parser errors and convert extracted-region spans to carrier-authored spans.
- Assign parser/source/recovery identities and related anchors.
- Keep the rail independent of external provider availability.

**Expected changes:**

- Add parser-to-session diagnostic conversion for Vue and Svelte.
- Route the result through the shared diagnostic/publication service when NCK7 is available or existing native rail otherwise.

**Discriminating proof:**

- Broken script/template fixtures always show syntax diagnostics.
- Provider-off and provider-crash cases retain native syntax diagnostics.

### LSO1-SB2 - Surface-production versus diagnostic decoupling

**Independently testable outcome:** A recoverable error may publish a diagnostic while still producing a degraded but usable semantic projection.

**Architecture:**

- Define Usable/Degraded/Catastrophic outcomes and per-region participation.
- Stop using has_errors as a proxy for cannot-build-IDE-surface.
- Make catastrophic absence explicit and basis-scoped.

**Expected changes:**

- Refactor parse/compile/session result carriers.
- Remove early returns that publish an empty diagnostic set on recoverable failures.

**Discriminating proof:**

- A pre-existing type error survives an unrelated dangling expression.
- Only catastrophic fixtures refuse the semantic surface.

### LSO1-SB3 - Reference-preserving recovery chunks

**Independently testable outcome:** Recovery preserves authored identifier reads/writes and introduces only minimal synthetic structure.

**Architecture:**

- Prefer missing-node/boundary insertion over rewriting user identifiers into different expressions.
- Tag synthetic chunks with operation capabilities and suppression metadata.
- Preserve exact source ranges before and after inserted chunks.

**Expected changes:**

- Replace aggressive member/expression token rewrites where they alter liveness or navigation.
- Add structured recovery emit operations.

**Discriminating proof:**

- Identifier references/rename/hover around broken sites remain stable.
- Synthetic punctuation/helper positions map to None.

### LSO1-SB4 - Fail-open usage and synthetic diagnostic policy

**Independently testable outcome:** Incomplete recovery never creates spurious unused/copy diagnostics or drops legitimate source diagnostics.

**Architecture:**

- Treat unknown usage as used while recovery participated.
- Either emit bounded keep-alives or explicit synthetic-code suppression by code/chunk class.
- Forbid message-based suppression and source re-anchoring.

**Expected changes:**

- Centralize recovery-participation flags and synthetic suppression metadata.
- Remove local ad hoc unused-diagnostic workarounds.

**Discriminating proof:**

- TS6133-like diagnostics on synthetic destructures do not leak or erase source diagnostics.
- A genuine authored unused binding remains diagnosable when completeness is known.

### LSO1-SB5 - Best-effort template and embedded-region recovery

**Independently testable outcome:** One malformed template node does not invalidate unrelated regions or the entire carrier.

**Architecture:**

- Recover per node/region with stable missing-node anchors.
- Preserve framework profile and embedded-map chain.
- Mark unsupported/degraded operations per region rather than globally.

**Expected changes:**

- Implement Vue/Svelte parity fixtures and bounded recovery builders.
- Coordinate with EMB0/PAR0 rather than introducing a template-only parser authority.

**Discriminating proof:**

- Malformed branch/attribute/expression retains unaffected navigation and diagnostics.
- Incremental recovery equals fresh parse/recovery for the same broken source.

### LSO1-SB6 - Recovery capability and performance proof

**Independently testable outcome:** Recovery work is bounded, deterministic, and truthful to every operation capability.

**Architecture:**

- Count recovered nodes, synthetic chunks, mapping drops, parser passes, and semantic regions.
- Require one parse/shallow pass per content hash and no retry loops.
- Generate operation participation matrix for broken-state fixtures.

**Expected changes:**

- Add PER0 counters and VIM rows.
- Remove sleep/debounce timing assumptions from correctness tests.

**Discriminating proof:**

- Linear adversarial broken-input tests stay bounded.
- Warm unchanged broken files perform zero additional parse/recovery work.

## Data, identity, invalidation, and publication laws

- Core operation identity is rooted in project/profile/source basis, semantic subject, operation kind, demand, and capability epoch; presentation encoding is not semantic identity.
- Generated coordinates, provider handles, and editor DTOs may exist only inside typed adapters and never become public semantic truth.
- Cancellation, stale, superseded, partial, NeedInputs, unsupported, and ambiguous outcomes remain distinct and are never collapsed to empty success.
- All returned targets and edits carry exact authored provenance and validate the snapshot/mapping chain used to derive them.
- Recovery snapshots are content/profile/parser-version keyed and never reused across different broken text.
- Synthetic suppression metadata is structural and code-specific, never message text.
- A degraded region cannot warm-admit a complete semantic result.

## Migration and cutover

- Characterize current Vue/Svelte broken-source behavior and mapping drops.
- Land native syntax rail first, then decouple surface production, then replace reference-altering recovery.
- Delete old bailouts only after per-region parity and catastrophic cases are explicit.

## Deletions

- Delete recoverable-error empty-publication paths.
- Delete reference-altering recovery helpers displaced by structured chunks.
- Delete heuristic synthetic diagnostic re-anchoring or message suppression.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

## Forbidden designs

- Resolver special cases for broken syntax.
- Rewriting authored identifiers into semantically different expressions.
- Weakening strict mapping to keep diagnostics visible.
- Treating parser errors as a reason to clear all diagnostics.
- Repeated parse/recovery retries or hidden whole-file rechecks.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

## Acceptance IDs and discriminating proof

- **LSO1-AC-TWO-RAIL:** syntax plus surviving semantic diagnostics are both visible on broken carriers.
- **LSO1-AC-REFERENCE:** authored identifier occurrence sets remain stable across recoverable edits.
- **LSO1-AC-STRICT-MAP:** synthetic positions never acquire approximate authored ranges.
- **LSO1-AC-REGION:** unaffected regions retain exact operation capability and incremental/fresh equivalence.
- **LSO1-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **LSO1-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **LSO1-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **LSO1-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Performance and bounded work

- Recovery remains one bounded parse/lowering pass with linear work and no unbounded synthetic chunks.
- Target ceiling: 800 production LOC, 8 production files, and 2 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Mandatory rescope and abort conditions

- Abort if preserving current behavior requires semantic token fabrication.
- Abort if a proposed suppression cannot identify an explicit synthetic chunk and diagnostic class.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. Vue/Svelte broken script/template differential fixtures with provider on/off/crash.
1. Mapping round-trip, reference/liveness, strict-drop, cancellation, incremental/fresh, and adversarial linearity suites.
1. Architecture guard against has_errors-based recoverable surface bailout.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Consumers and unlocks

- Supplies tolerant input to LSO3-LSO8 and NCK diagnostics.
- Enables truthful editor behavior during active typing.
- Provides VIM broken-carrier conformance rows.

## Source reconciliation

- `docs/arch/ide-error-recovery-design.md`.
- B2/PAR0 recovery and diagnostic clauses.

Durable clauses are transferred as digest-bound requirement atoms. Historical path archaeology, obsolete branches, and implementation journals are not copied into the charter. Git history remains the archive.


---

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


---

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


---

<!-- unified-charter-v2
id=LSO4
name=References, hierarchy, and bounded occurrence planning
phase=expansion
train=expansion.language-service
product=language_service
kind=implementation
semantic_role=delivery
class=successor
predecessors=LSO2,IDX0
conditional_predecessors=
owner=expansion.language-service:one authored-coordinate semantic-operation and edit-transaction authority
conflict_domains=semantic_authority,mapping_geometry,performance_evidence
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
charter=charters/expansion-language-service/LSO4.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# LSO4 - References, hierarchy, and bounded occurrence planning

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

The internal subblocks below are binding decomposition and review checkpoints. They do not receive independent dispatch, leases, receipts, or deletion ownership unless the pre-scope architect proves that one subblock is independently acceptable or the block crosses a mandatory rescope trigger. In that case, amend the DAG before production mutation rather than treating a train-sized subblock as an implementation checklist.

## Independently acceptable outcome

Implement bounded semantic occurrence planning for references, call/type hierarchy, incoming/outgoing relationships, and rename candidate discovery. LSO4 returns role-typed occurrences and hierarchy edges over exact targets; it does not decide replacement text or materialize edits.

The current owner is **provider reference arrays, native binding scans, generated-text occurrences, feature-local workspace traversal, and untyped ranges reused by rename**. The final and sole owner is **one OccurrencePlanner over LSO2/IDX0 with typed occurrence roles, exact target identity, bounded enumeration, hierarchy edges, and explicit completeness**.

## Architectural role and end state

LSO4 separates discovery from mutation. References and hierarchy need broad but bounded candidate enumeration; rename needs the same occurrences plus stricter role/policy analysis in LSO5. The index narrows candidates while the semantic engine validates every occurrence.

## Expected production surfaces

- `crates/verter_semantic` for occurrence roles and hierarchy edge semantics
- `crates/verter_session` for project-scoped planning and semantic validation
- `crates/verter_language`/`crates/verter_identity` for profile/target identities
- `crates/verter_type_runtime` adapters for provider observations
- `crates/verter_lsp` thin references/hierarchy adapters

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.

## Named APIs and data boundaries

- `OccurrencePlanner`, `OccurrenceRequest`, and `OccurrenceDemand`
- `SemanticOccurrence { id, target, role, authored_anchor, basis }`
- `OccurrenceRole` including declaration/read/write/call/type/import/export/tag/attribute/string-contract roles
- `OccurrencePage`, `OccurrenceCursor`, and `OccurrenceCompleteness`
- `HierarchyEdge`, `HierarchyKind`, and `HierarchyResult`
- `CandidateSource::{Index, LocalSemantic, ProviderObservation, FrameworkContribution}`

## Exact predecessor contracts

- **LSO2:** consume canonical target/provenance graph and exact authored anchors.
- **IDX0:** consume bounded cross-file candidates, memberships, and invalidation without semantic authority.

External custody: none beyond the package activation boundary.

## Binding architecture

- IDX0 narrows files/symbol candidates; every returned occurrence is semantically validated against the canonical target.
- Occurrence roles are explicit and preserved through mapping; rename may select by role without parsing text.
- Generated/provider occurrences normalize to authored anchors under exact snapshot provenance before entering the result.
- Incomplete enumeration, budget exhaustion, cancellation, stale inputs, and unsupported provider capabilities are typed and never negative-cached as complete.
- References and hierarchy may stream/pages; ordering and cursor identity are deterministic on an immutable basis.
- A leaf/local request does not scan all project files when the index can prove bounded candidates.
- Hierarchy edges are semantic target relationships, not textual name matches.

## Internal subblocks

### LSO4-SB1 - Occurrence role and identity model

**Independently testable outcome:** Every reference-like site has a stable role and target identity sufficient for navigation and rename policy.

**Architecture:**

- Define closed common roles plus profile-qualified extension mechanism.
- Root occurrence ID in target, source anchor, role, profile, and basis.
- Separate declaration occurrence from target node identity.

**Expected changes:**

- Add role taxonomy and generated guards.
- Map existing native/provider/framework occurrences to roles.

**Discriminating proof:**

- Role set equality guard catches missing/duplicate registrations.
- Message/text changes do not change occurrence identity.

### LSO4-SB2 - Bounded candidate planning

**Independently testable outcome:** Workspace occurrence work is proportional to indexed candidates and explicit demand.

**Architecture:**

- Query name/export/component/link/membership indexes by target identity and profile.
- Represent incomplete index enumeration and budgets explicitly.
- Plan local, project, dependency, and external scopes separately.

**Expected changes:**

- Implement planner/read-set capture and candidate audit counters.
- Remove eager workspace loops from feature handlers.

**Discriminating proof:**

- Inapplicable profiles/files perform zero semantic/provider work.
- Budget exhaustion never admits a complete negative result.

### LSO4-SB3 - Semantic occurrence validation

**Independently testable outcome:** Every candidate is validated by authoritative native/framework/provider semantics before publication.

**Architecture:**

- Validate binding/symbol/alias/augmentation/component-contract identity.
- Normalize provider observations through LSO2 snapshot matching.
- Preserve multiple roles for one authored span when semantically real.

**Expected changes:**

- Add per-source validators and same-key singleflight.
- Delete name/range-only reference matching.

**Discriminating proof:**

- Planting a same-name unrelated symbol is rejected.
- Incremental validation equals fresh after alias/export/profile edits.

### LSO4-SB4 - Reference result assembly and pagination

**Independently testable outcome:** Reference results are deterministic, authored, complete-truthful, and streamable without snapshot leaks.

**Architecture:**

- Sort by canonical source/anchor/role/target identity.
- Bind cursor to immutable basis and invalidate on changes.
- Dedup exact occurrence identity only.

**Expected changes:**

- Implement bounded pages/stream and LSP adapter.
- Release snapshots/cursors on completion/cancel/timeout.

**Discriminating proof:**

- Permutation and page-size changes yield the same complete occurrence set.
- Stale cursor is rejected and retained bytes plateau.

### LSO4-SB5 - Hierarchy relationship planning

**Independently testable outcome:** Call/type/implementation hierarchy uses target edges and validated call/override occurrences rather than text searches.

**Architecture:**

- Define preparation target and incoming/outgoing edge semantics.
- Use LSO2 edges plus call/implementation occurrence roles.
- Bound recursion and detect cycles.

**Expected changes:**

- Implement hierarchy planner and typed partial outcomes.
- Share target renderer with LSO3.

**Discriminating proof:**

- Overload/override/component hierarchy fixtures preserve legitimate alternatives.
- Cycles and recursive calls terminate deterministically.

### LSO4-SB6 - Provider/framework parity and work evidence

**Independently testable outcome:** Native, provider, and framework contributions compose to one occurrence set with measurable bounded work.

**Architecture:**

- Generate profile/provider/recovery matrix.
- Count index candidates, semantic validations, provider requests, mappings, pages, allocations.
- Prove provider absence and disabled profiles perform zero provider work.

**Expected changes:**

- Add VIM/PER0 rows and gated real-provider canaries.
- Classify residual unsupported roles honestly.

**Discriminating proof:**

- Differential fixtures match semantic occurrence IDs/roles, not message/range counts.
- Warm repeated query avoids parse/index/provider work when facts are unchanged.

## Data, identity, invalidation, and publication laws

- Core operation identity is rooted in project/profile/source basis, semantic subject, operation kind, demand, and capability epoch; presentation encoding is not semantic identity.
- Generated coordinates, provider handles, and editor DTOs may exist only inside typed adapters and never become public semantic truth.
- Cancellation, stale, superseded, partial, NeedInputs, unsupported, and ambiguous outcomes remain distinct and are never collapsed to empty success.
- All returned targets and edits carry exact authored provenance and validate the snapshot/mapping chain used to derive them.
- Occurrence cursors are snapshot-scoped capabilities and are not serializable long-term cache keys.
- A source span may host multiple occurrence roles; range deduplication alone is forbidden.
- Incomplete candidate enumeration cannot publish a complete empty occurrence set.

## Migration and cutover

- Migrate same-file references, then indexed project references, then provider/framework contributions.
- Move call/type hierarchy after occurrence and target identity are stable.
- Keep rename materialization in LSO5/LSO8.

## Deletions

- Delete eager workspace/name-only reference scans and generated-range result construction.
- Delete feature-local occurrence dedup/order/pagination.
- Delete hierarchy text matching and current-file mapper fallback.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

## Forbidden designs

- Index candidates treated as authoritative references.
- Name-only/string search as semantic validation.
- Range-only occurrence identity/dedup.
- Unbounded project enumeration on interactive requests.
- Caching a budget-exhausted negative as complete.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

## Acceptance IDs and discriminating proof

- **LSO4-AC-ROLES:** every occurrence has exact role/target/basis and planted same-name false positives fail.
- **LSO4-AC-BOUNDED:** candidate/validation work is bounded and zero for inapplicable profiles.
- **LSO4-AC-PAGES:** pagination permutations reconstruct one deterministic exact set.
- **LSO4-AC-HIERARCHY:** hierarchy edges are semantic, cycle-safe, and share target rendering.
- **LSO4-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **LSO4-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **LSO4-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **LSO4-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Performance and bounded work

- Warm occurrence queries reuse index/semantic facts; memory remains bounded across cancelled/abandoned cursors.
- Target ceiling: 800 production LOC, 8 production files, and 2 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Mandatory rescope and abort conditions

- Abort if a required occurrence role can only be inferred from generated text or regex.
- Abort if the index cannot name a complete/read-set basis for a claimed complete result.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. Role/identity/same-name negative fixtures; indexed bounded-work tests.
1. Pagination/cursor stale/cancel/memory tests.
1. Provider/framework differential and call/type hierarchy cycle suites.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Consumers and unlocks

- Unlocks LSO5 semantic rename planning.
- Feeds LSO9 references/hierarchy conformance.
- Provides occurrence sets to future refactor/search operations.

## Source reconciliation

- Goto-definition/references/rename legacy designs.
- IDX0 workspace discovery and framework adapter clauses.

Durable clauses are transferred as digest-bound requirement atoms. Historical path archaeology, obsolete branches, and implementation journals are not copied into the charter. Git history remains the archive.


---

<!-- unified-charter-v2
id=LSO5
name=Semantic rename planning and conflict analysis
phase=expansion
train=expansion.language-service
product=language_service
kind=implementation
semantic_role=delivery
class=successor
predecessors=LSO4,LRA0
conditional_predecessors=
owner=expansion.language-service:one authored-coordinate semantic-operation and edit-transaction authority
conflict_domains=semantic_authority,diagnostic_action_service,mapping_geometry
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
charter=charters/expansion-language-service/LSO5.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# LSO5 - Semantic rename planning and conflict analysis

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

The internal subblocks below are binding decomposition and review checkpoints. They do not receive independent dispatch, leases, receipts, or deletion ownership unless the pre-scope architect proves that one subblock is independently acceptable or the block crosses a mandatory rescope trigger. In that case, amend the DAG before production mutation rather than treating a train-sized subblock as an implementation checklist.

## Independently acceptable outcome

Implement semantic rename planning as a distinct bounded block: classify the rename subject, select role-eligible occurrences, derive framework/language-aware replacement intents, detect conflicts, and return a typed RenamePlan. LSO5 never creates final workspace edits or writes files.

The current owner is **provider rename edits, native references reused without role policy, component/tag special cases, and direct WorkspaceEdit construction**. The final and sole owner is **one RenamePlanner over canonical targets and typed occurrences, producing authored edit intents plus explicit conflicts/refusals for LSO8 materialization**.

## Architectural role and end state

LSO5 prevents rename from being buried inside references or the edit transaction engine. Rename is semantic policy: declaration namespace, aliases, property shorthand, imports/exports, component casing, template roles, strings, and conflict analysis all differ, while final atomic edit application belongs to LSO8.

## Expected production surfaces

- `crates/verter_semantic` for rename subjects, policies, and conflict semantics
- `crates/verter_session` for project-scoped planning
- `crates/verter_actions` for edit-intent/safety integration
- `crates/verter_language` for profile-specific casing/contract contributions
- `crates/verter_lsp` prepareRename/rename adapters only

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.

## Named APIs and data boundaries

- `RenameSubject`, `RenameSubjectKind`, and `RenamePolicy`
- `RenameRequest`, `RenamePlan`, `RenameRefusal`, and `RenameConflict`
- `RenameOccurrenceSelection` over `OccurrenceRole`
- `ReplacementIntent { occurrence, replacement, transform, preconditions }`
- `NameTransform` for exact alias/case/segment transformations
- `RenameSafety::{Safe, Suggested, Unsafe, Unsupported}`

## Exact predecessor contracts

- **LSO4:** consume complete-truthful role-typed occurrences and exact target identity.
- **LRA0:** consume action safety, applicability, provenance, and authored transaction ownership.

External custody: none beyond the package activation boundary.

## Binding architecture

- Rename subject classification is semantic and profile-aware, not token-text heuristics.
- Each subject kind declares eligible occurrence roles, namespace, transformations, and conflict checks.
- The planner emits replacement intents with exact old text/anchor/revision preconditions; it does not emit LSP TextEdits.
- Component Pascal/kebab/local/global relationships are contributed as typed role/transform data, not Vue branches in neutral core.
- String/comment occurrences are excluded unless a language/framework contract explicitly marks them semantic.
- Ambiguous target, incomplete occurrences, stale inputs, unsupported transformation, or conflict yields typed refusal/plan status.
- No partial multi-file plan is represented as safely applicable.

## Internal subblocks

### LSO5-SB1 - Rename subject classification and prepare contract

**Independently testable outcome:** The queried authored position resolves to an exact renameable subject or typed refusal with an exact selection range.

**Architecture:**

- Classify bindings, properties, methods, types, imports/exports, aliases, components/tags/props/events/slots, labels, and unsupported subjects.
- Bind subject to canonical target and namespace.
- Return exact authored prepare range and placeholder.

**Expected changes:**

- Implement shared prepareRename classifier.
- Delete handler-local token and current-file generated heuristics.

**Discriminating proof:**

- Every supported subject has a stable kind/target; ambiguous/unmapped/generated-only positions refuse.
- Prepare range round-trips across encodings and carriers.

### LSO5-SB2 - Role eligibility and occurrence selection

**Independently testable outcome:** Only semantically affected roles are selected for each rename subject.

**Architecture:**

- Define generated policy table from subject kind to roles and alias behavior.
- Handle shorthand/destructuring/import-export and read/write/declaration distinctions.
- Preserve exclusions with explicit reason codes.

**Expected changes:**

- Implement deterministic selection over LSO4 results.
- Remove broad replace-all-reference behavior.

**Discriminating proof:**

- Role mutation tests detect over- and under-selection.
- Same spelling in unrelated namespaces remains unchanged.

### LSO5-SB3 - Language/framework replacement transforms

**Independently testable outcome:** Replacement spelling is derived by typed transforms with exact segment mapping.

**Architecture:**

- Support identity, alias preservation, shorthand expansion, Pascal/kebab segment conversion, event/prop conventions, and profile-owned transforms.
- Require transforms to preserve authored letter mapping and reject lossy/ambiguous conversions.
- Keep framework contributions data-driven.

**Expected changes:**

- Add `NameTransform` registry keyed by profile/capability.
- Migrate Vue/Svelte component/tag cases and global-component rules into VIM fixtures.

**Discriminating proof:**

- Round-trip/collision tests cover acronym, Unicode, separators, and mixed case.
- No central `if framework == vue` branch appears.

### LSO5-SB4 - Conflict and legality analysis

**Independently testable outcome:** The plan detects scope collisions, duplicate exports, property conflicts, filesystem/path collisions, and profile contract violations before edit materialization.

**Architecture:**

- Query semantic scopes/module/project/index facts under exact basis.
- Separate blocking conflicts from warnings/suggested unsafe changes.
- Treat incomplete conflict analysis as non-safe.

**Expected changes:**

- Implement conflict analyzers and typed related targets.
- Do not rely on post-edit provider diagnostics as the only safety test.

**Discriminating proof:**

- Planting a local shadow/export collision blocks safe rename.
- Incremental conflict results equal fresh after concurrent project edits.

### LSO5-SB5 - Rename plan and edit intents

**Independently testable outcome:** A complete plan contains deterministic authored replacement intents with exact preconditions and no raw edits.

**Architecture:**

- Sort intents by source/anchor/role; preserve semantic identity.
- Require old-text/hash/revision/target/authority preconditions.
- Model file rename/path intents separately and only when explicitly supported.

**Expected changes:**

- Emit `RenamePlan` consumed by LSO8.
- Remove direct WorkspaceEdit creation from semantic planners.

**Discriminating proof:**

- Plan serialization is deterministic and rejects stale/missing preconditions.
- No overlapping intent is silently resolved in LSO5.

### LSO5-SB6 - Provider comparison and migration

**Independently testable outcome:** Provider rename observations are used for certification/residual coverage without becoming the public edit authority.

**Architecture:**

- Normalize provider rename locations to occurrence/target identity.
- Compare selected roles and replacement intents.
- Retain provider ownership only for unsupported subject families with truthful capability.

**Expected changes:**

- Add hermetic/gated provider comparison matrix.
- Migrate subject families incrementally and delete direct provider edit replay for promoted families.

**Discriminating proof:**

- Promoted family performs zero provider rename work.
- Provider order/output formatting cannot change the native RenamePlan.

### LSO5-SB7 - Bounded work and cancellation proof

**Independently testable outcome:** Rename planning is bounded, cancellable, and admits only complete plans.

**Architecture:**

- Budget candidate enumeration/conflict checks and propagate cancellation.
- Do not cache partial occurrence/conflict analysis as safe.
- Release plan/intermediate snapshots after cancellation.

**Expected changes:**

- Add PER0 counters and memory tests.
- Expose typed budget/refusal to consumers.

**Discriminating proof:**

- Cancelled/budgeted plans produce no edit intents usable as complete.
- Long-churn repeated rename planning plateaus in memory.

## Data, identity, invalidation, and publication laws

- Core operation identity is rooted in project/profile/source basis, semantic subject, operation kind, demand, and capability epoch; presentation encoding is not semantic identity.
- Generated coordinates, provider handles, and editor DTOs may exist only inside typed adapters and never become public semantic truth.
- Cancellation, stale, superseded, partial, NeedInputs, unsupported, and ambiguous outcomes remain distinct and are never collapsed to empty success.
- All returned targets and edits carry exact authored provenance and validate the snapshot/mapping chain used to derive them.
- A safe RenamePlan is complete across its declared scope and exact basis.
- Replacement text is semantic plan data; final position encoding and transaction grouping belong to LSO8.
- Profile transform epochs enter plan identity and invalidation.

## Migration and cutover

- Characterize prepare/rename for bounded subject families.
- Migrate local/native bindings first, then imports/exports/properties, then Vue/Svelte component contracts.
- Keep unsupported families provider-owned until certified.

## Deletions

- Delete direct provider WorkspaceEdit replay for migrated rename families.
- Delete name-only/string replace rename paths and feature-local casing logic.
- Delete semantic planners that materialize final TextEdits.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

## Forbidden designs

- Rename implemented as references plus string replacement.
- Partial/incomplete plan labeled safe.
- Central framework switch for spelling transforms.
- Raw edits, line/column encoding, or file writes in RenamePlanner.
- Silent overlap/conflict resolution.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

## Acceptance IDs and discriminating proof

- **LSO5-AC-SUBJECT:** prepare/classification is exact across supported subject kinds and refuses ambiguity.
- **LSO5-AC-ROLES:** role policy prevents same-name/namespace false edits.
- **LSO5-AC-TRANSFORM:** framework/language transforms are typed, round-trip tested, and data-driven.
- **LSO5-AC-CONFLICT:** planted scope/export/path conflicts block safe plans.
- **LSO5-AC-NO-RAW-EDIT:** output contains intents/preconditions only.
- **LSO5-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **LSO5-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **LSO5-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **LSO5-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Performance and bounded work

- Planning work is proportional to LSO4 occurrences plus declared conflict scopes; no extra workspace scan.
- Target ceiling: 800 production LOC, 8 production files, and 2 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Mandatory rescope and abort conditions

- Abort if a subject family lacks complete role/conflict semantics.
- Abort if final edit materialization leaks into this block.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. Prepare/subject/role/namespace/conflict/transform mutation suites.
1. Vue/Svelte component/tag/prop/event/slot and global component fixtures.
1. Provider comparison, incremental/fresh, cancellation/budget, memory, and no-raw-edit guards.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Consumers and unlocks

- Unlocks LSO8 authored transaction materialization.
- Feeds LSO9 rename conformance.
- Provides reusable semantic rename plan for CLI/refactor clients.

## Source reconciliation

- Goto-definition/references/rename legacy decisions.
- `docs/arch/global-components-ide-typing.md` rename-relevant clauses.

Durable clauses are transferred as digest-bound requirement atoms. Historical path archaeology, obsolete branches, and implementation journals are not copied into the charter. Git history remains the archive.


---

<!-- unified-charter-v2
id=LSO6
name=Completion candidates and provider-neutral resolve intents
phase=expansion
train=expansion.language-service
product=language_service
kind=implementation
semantic_role=delivery
class=successor
predecessors=LSO0,LSO2,H2,TCM4,PUB0
conditional_predecessors=
owner=expansion.language-service:one authored-coordinate semantic-operation and edit-transaction authority
conflict_domains=provider_lifecycle,mapping_geometry,public_protocol
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
charter=charters/expansion-language-service/LSO6.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# LSO6 - Completion candidates and provider-neutral resolve intents

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

The internal subblocks below are binding decomposition and review checkpoints. They do not receive independent dispatch, leases, receipts, or deletion ownership unless the pre-scope architect proves that one subblock is independently acceptable or the block crosses a mandatory rescope trigger. In that case, amend the DAG before production mutation rather than treating a train-sized subblock as an implementation checklist.

## Independently acceptable outcome

Implement one provider-neutral completion pipeline: authored completion context classification, bounded candidate composition, typed lazy resolve handles, exact provider-epoch validation, and authored import/fix intents. Completion resolve never emits unchecked generated-file edits.

The current owner is **provider-specific completion parsing, opaque JSON data envelopes, LSP-baked routing flags, generated TSX import edits, and separate workspace component candidates**. The final and sole owner is **one CompletionService with normalized candidates and typed resolve intents, plus thin provider adapters and LSO8-authored transaction materialization**.

## Architectural role and end state

LSO6 preserves provider strengths without allowing provider protocol details to define Verter semantics. Completion list and resolve share one authored request basis; lazy provider state is typed and epoch-checked, while final import placement is an edit-transaction concern.

## Expected production surfaces

- `crates/verter_session` completion coordination and candidate composition
- `crates/verter_type_runtime` provider-specific candidate/resolve adapters
- `crates/verter_semantic`/`crates/verter_language` native/framework candidates and context
- `crates/verter_protocol` normalized completion contracts
- `crates/verter_lsp` envelope serialization and completionItem adapter only

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.

## Named APIs and data boundaries

- `CompletionRequest`, `CompletionContext`, `CompletionCandidate`, and `CompletionSet`
- `CompletionOrigin`, `CompletionCandidateId`, `CompletionKind`, and `SortGroup`
- `CompletionResolveKey::{Provider, Native, Framework, Workspace}` with typed payloads
- `CompletionResolveRequest`, `CompletionResolveResult`, and exact epoch/basis validation
- `ImportIntent`, `AdditionalEditIntent`, and `CompletionDocumentation`
- `CompletionCapability` and honest resolve support

## Exact predecessor contracts

- **LSO0:** consume authored operation, typed outcome, and edit-intent constitution.
- **LSO2:** consume canonical target/provenance identity for imports/definitions/resolve.
- **H2:** consume exact project-scoped provider binding and provider epoch.
- **TCM4:** consume certified mapper/provider activation and exact basis.
- **PUB0:** consume public schema/capability truth and typed outcomes.

External custody: none beyond the package activation boundary.

## Binding architecture

- Completion candidates normalize into provider-neutral identity/kind/target/origin; provider opaque data stays inside a typed resolve key.
- Resolve keys are valid only for the exact provider/native/framework epoch and request basis that created them.
- The LSP envelope contains routing serialization only and is rejected on provider/profile/session mismatch.
- Additional provider edits normalize to authored edit intents; preamble imports are classified structurally before any strict map result is accepted.
- Workspace/native/framework/provider candidates compose deterministically and dedup by semantic target/candidate identity, not label text alone.
- Advertised resolve capability reflects the active providers/candidate origins actually supported.
- Completion list is demand-bounded and may be incomplete with explicit continuation/completeness; it is never silently truncated as complete.

## Internal subblocks

### LSO6-SB1 - Authored completion context classifier

**Independently testable outcome:** An authored cursor position produces an exact language/framework context or typed unmapped/unsupported result.

**Architecture:**

- Classify script/template/style/attribute/tag/expression/import/member/string contexts.
- Carry profile/source/recovery/capability basis.
- Reject synthetic/generated-only positions.

**Expected changes:**

- Implement one classifier consumed by native/framework/provider candidate sources.
- Delete provider-specific LSP-position classification forks.

**Discriminating proof:**

- Context mutation fixtures discriminate nearby syntactic sites.
- Broken-carrier contexts use LSO1 capability flags truthfully.

### LSO6-SB2 - Normalized candidate identity and composition

**Independently testable outcome:** Candidates from all origins compose deterministically without label-only collisions or hidden precedence.

**Architecture:**

- Define candidate ID from origin/target/kind/insert semantics/context.
- Separate display label, filter/sort text, detail, docs, and semantic identity.
- Use exact origin priority policy only where ratified.

**Expected changes:**

- Implement shared candidate set builder and dedup/order.
- Migrate workspace components and provider completions.

**Discriminating proof:**

- Input/provider ordering permutations yield byte-identical candidate sets.
- Same-label distinct targets survive; duplicate observations collapse.

### LSO6-SB3 - Typed resolve keys and provider adapters

**Independently testable outcome:** Lazy resolve is replayable only against the exact producer and cannot route opaque data to a foreign provider.

**Architecture:**

- Define typed per-provider-family keys in `verter_type_runtime`.
- Stamp provider ID/epoch, path/target basis, candidate ID, and request scope.
- Fail closed on mismatch, malformed data, swap, or stale snapshot.

**Expected changes:**

- Replace arbitrary JSON and `tsgo` marker envelopes.
- Share tsserver-family detail mapping at the lowest reusable owner.

**Discriminating proof:**

- Provider swap/malformed key returns unchanged/refusal and never calls the foreign provider.
- Round-trip serialization preserves typed key exactly.

### LSO6-SB4 - Authored import and additional edit intents

**Independently testable outcome:** Resolve produces authored semantic intents, never trusted generated offsets or final LSP edits.

**Architecture:**

- Classify generated preamble insertions structurally using exact mapper boundaries.
- Resolve carrier import anchors and target source context.
- Represent other mapped replacements as authored intents with preconditions.

**Expected changes:**

- Reuse one import intent model and route materialization to LSO8.
- Remove generated-head-to-carrier-0:0 acceptance.

**Discriminating proof:**

- Vue/Svelte/self/foreign carrier and no-script cases place or refuse correctly.
- Absent/stale boundary or anchor fails closed.

### LSO6-SB5 - Documentation/detail enrichment and capability truth

**Independently testable outcome:** Resolve enriches candidate detail/docs/commands without changing semantic identity or advertising unsupported behavior.

**Architecture:**

- Normalize provider display parts/docs and native/framework metadata.
- Keep commands/code actions typed and separately authorized.
- Compute resolve capability from active origin support.

**Expected changes:**

- Implement shared enrichment and protocol projection.
- Remove dishonest global `resolve_provider: true`.

**Discriminating proof:**

- Provider-off/no-resolve sessions advertise false.
- Enrichment does not change candidate/dedup identity.

### LSO6-SB6 - Completion performance, cancellation, and conformance

**Independently testable outcome:** List/resolve work is bounded, cancellable, cache-safe, and equivalent across providers/profiles.

**Architecture:**

- Count candidate sources, provider requests, index lookups, mappings, allocations, retained keys.
- Cache by exact context/origin/epoch and admit complete results only.
- Generate matrix for providers, recovery, coexistence, global components, and auto-import.

**Expected changes:**

- Add VIM/PER0 rows and gated provider canaries.
- Release stale resolve keys/candidate sets on epoch changes.

**Discriminating proof:**

- Warm list/resolve avoids repeated parse/index/provider work where supported.
- Cancelled/partial lists never masquerade as complete and memory plateaus.

## Data, identity, invalidation, and publication laws

- Core operation identity is rooted in project/profile/source basis, semantic subject, operation kind, demand, and capability epoch; presentation encoding is not semantic identity.
- Generated coordinates, provider handles, and editor DTOs may exist only inside typed adapters and never become public semantic truth.
- Cancellation, stale, superseded, partial, NeedInputs, unsupported, and ambiguous outcomes remain distinct and are never collapsed to empty success.
- All returned targets and edits carry exact authored provenance and validate the snapshot/mapping chain used to derive them.
- Resolve key identity is origin-specific but serialization is provider-neutral.
- Import/additional edit intents are not applicable until LSO8 validates the authored transaction.
- Candidate display enrichment does not mutate semantic candidate identity.

## Migration and cutover

- Introduce typed keys while preserving existing candidate output, then migrate each provider.
- Move workspace/framework candidates into shared composition.
- Move provider additional edits to authored intents and delete direct LSP edit replay.

## Deletions

- Delete `{ tsgo: true, original_data, tsx_path }` and arbitrary provider JSON routing.
- Delete provider-specific completion merge/dedup and direct generated edit translation.
- Delete dishonest resolve capability and current-file/foreign-file import fallbacks.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

## Forbidden designs

- Opaque JSON provider data in core candidate/results.
- Provider ID or generated path encoded in display fields.
- Accepting strict-mapped preamble insertion at carrier file top.
- Final TextEdit/WorkspaceEdit materialization in completion service.
- Label-only candidate dedup or unbounded candidate enumeration.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

## Acceptance IDs and discriminating proof

- **LSO6-AC-CANDIDATES:** normalized semantic candidate identity/dedup/order is stable across origin order.
- **LSO6-AC-RESOLVE-KEY:** foreign/stale/malformed keys fail closed before provider invocation.
- **LSO6-AC-IMPORT:** preamble/import edits become exact authored intents or typed refusal, never 0:0.
- **LSO6-AC-CAPABILITY:** advertised resolve support is exact for active origins.
- **LSO6-AC-PROVIDERS:** tsgo/tsserver/extension/native/framework matrix preserves equivalent actionable behavior.
- **LSO6-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **LSO6-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **LSO6-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **LSO6-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Performance and bounded work

- List/resolve work is context-demanded and bounded; inactive origins perform zero work.
- Target ceiling: 800 production LOC, 8 production files, and 2 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Mandatory rescope and abort conditions

- Abort if a provider cannot expose a typed stable resolve key.
- Abort if an edit cannot be mapped to an exact authored intent and a heuristic fallback is proposed.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. Provider key/epoch swap/malformed negative tests.
1. Candidate identity/order/dedup and global-component/context fixtures.
1. Import intent current/foreign/self/no-script/stale-map matrix; cancellation/cache/memory/performance tests.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Consumers and unlocks

- Feeds LSO8 edit transaction materialization and LSO9 conformance.
- Provides thin completion adapters to LSP/editors.
- Preserves external providers without provider-shaped core APIs.

## Source reconciliation

- `docs/arch/provider-completion-resolve-design.md`.
- Framework import-placement and global-components typing designs.

Durable clauses are transferred as digest-bound requirement atoms. Historical path archaeology, obsolete branches, and implementation journals are not copied into the charter. Git history remains the archive.


---

<!-- unified-charter-v2
id=LSO7
name=Hover, signature-help, and inlay presentation composition
phase=expansion
train=expansion.language-service
product=language_service
kind=implementation
semantic_role=delivery
class=successor
predecessors=LSO0,LSO2,H2,TCM4,PUB0
conditional_predecessors=
owner=expansion.language-service:one authored-coordinate semantic-operation and edit-transaction authority
conflict_domains=provider_lifecycle,public_protocol,lsp_publication
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
charter=charters/expansion-language-service/LSO7.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# LSO7 - Hover, signature-help, and inlay presentation composition

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

The internal subblocks below are binding decomposition and review checkpoints. They do not receive independent dispatch, leases, receipts, or deletion ownership unless the pre-scope architect proves that one subblock is independently acceptable or the block crosses a mandatory rescope trigger. In that case, amend the DAG before production mutation rather than treating a train-sized subblock as an implementation checklist.

## Independently acceptable outcome

Implement one presentation composition service for hover, signature help, and inlay hints. It combines authoritative native/framework facts and provider observations under explicit per-fragment authority, then returns authored-range semantic presentation fragments independent of editor markup/protocol.

The current owner is **feature-local native/provider merge rules, early returns, provider text dominance heuristics, generated helper stripping, and LSP-specific markup construction**. The final and sole owner is **one PresentationService with stable subjects/fragments, explicit authority/provenance, exact authored ranges, and thin LSP/editor renderers**.

## Architectural role and end state

LSO7 separates semantic presentation from navigation and diagnostics while reusing LSO2 targets. It avoids pretending provider-formatted strings are semantic types: fragments state whether they are source annotations, provider display, native resolved facts, documentation, parameter activity, or hints.

## Expected production surfaces

- `crates/verter_session` presentation coordination
- `crates/verter_semantic` native fact/subject extraction
- `crates/verter_type_runtime` provider observation adapters
- `crates/verter_protocol` fragment/result schemas
- `crates/verter_lsp` Markdown/MarkupContent/SignatureHelp/InlayHint projection only

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.

## Named APIs and data boundaries

- `PresentationRequest`, `PresentationKind`, and `PresentationSubject`
- `PresentationFragment`, `FragmentKind`, `FragmentAuthority`, and `FragmentProvenance`
- `HoverPresentation`, `SignaturePresentation`, and `InlayPresentation`
- `ActiveSignature`, `ActiveParameter`, and exact call-site basis
- `InlayHintIntent` with authored anchor and optional target/edit intent refs
- `PresentationPolicy` keyed by profile/capability/configuration epoch

## Exact predecessor contracts

- **LSO0:** consume authored operation and public outcome constitution.
- **LSO2:** consume canonical target/provenance links for definitions and subjects.
- **H2:** consume exact provider binding/epoch.
- **TCM4:** consume provider/mapping basis.
- **PUB0:** consume public result/capability vocabulary.

External custody: none beyond the package activation boundary.

## Binding architecture

- Fragments carry semantic kind/authority/provenance; formatting and Markdown are edge concerns.
- Provider display text, native resolved types, source-literal annotations, framework labels, and docs are distinct fragments and may coexist only under explicit policy.
- A native result cannot be silently discarded by a provider early return, nor can two authorities present contradictory “sole type” blocks without declared composition.
- Signature active parameter/index is derived from exact call/context basis and validated against provider/native signature identity.
- Inlay hints are semantic intents at authored anchors and cannot carry generated positions or direct edits.
- Provider absence/staleness degrades only provider-owned fragments and updates completeness/capability truth.
- Helper/synthetic names are excluded by structured provenance, not arbitrary string stripping in core semantics.

## Internal subblocks

### LSO7-SB1 - Presentation subject classification

**Independently testable outcome:** Hover/signature/inlay queries classify one exact semantic subject and authored anchor.

**Architecture:**

- Classify symbols, expressions, component tags/attributes/events, call sites, parameters, inferred types, and unsupported/synthetic sites.
- Bind source/profile/recovery/target basis.
- Reuse LSO2 targets for linked definitions.

**Expected changes:**

- Implement shared classifier and remove handler-local early-return targets.
- Keep operation-specific subject refinements typed.

**Discriminating proof:**

- Adjacent token/context fixtures classify exactly.
- Synthetic/helper-only positions return unmapped/unsupported.

### LSO7-SB2 - Fragment authority and composition policy

**Independently testable outcome:** Every fragment has a named authority and deterministic composition order with no hidden semantic override.

**Architecture:**

- Define kinds for signature/type display, source annotation, framework contract, docs, provenance, diagnostics note, parameter label, and hint.
- Define provider/native/source/framework authority and replacement/coexistence rules.
- Keep optional known-TS-bug annotations static and issue-backed, not a second resolver value.

**Expected changes:**

- Implement generated policy table and composition engine.
- Replace implicit provider-text dominance/early returns.

**Discriminating proof:**

- Policy mutation tests expose contradictory duplicate sole-type fragments.
- Provider on/off changes only declared provider-owned fragments.

### LSO7-SB3 - Hover semantic assembly

**Independently testable outcome:** Hover returns exact semantic fragments and links without LSP Markdown dependence.

**Architecture:**

- Compose native/provider/framework/source/docs fragments.
- Preserve child component/tag/import/event targets and related definitions.
- Represent partial/NeedInputs per fragment/result.

**Expected changes:**

- Migrate common hover and framework child hovers.
- Delete helper-prefix text cleanup as semantic logic; keep rendering sanitation at edge.

**Discriminating proof:**

- Provider/native/framework matrices yield exact fragment kinds/authority.
- No provider early return skips valid native/framework metadata.

### LSO7-SB4 - Signature help assembly

**Independently testable outcome:** Signature sets, active signature, and active parameter are stable, exact, and provider-neutral.

**Architecture:**

- Normalize native/provider overload/signature identities and documentation.
- Use exact call-site context and mapping basis.
- Preserve ambiguity and budget/cancellation outcomes.

**Expected changes:**

- Implement shared signature result and provider adapters.
- Remove provider-specific LSP SignatureHelp construction.

**Discriminating proof:**

- Nested/generic/optional/rest/callback and broken-call fixtures choose correct active parameter.
- Provider ordering cannot change canonical signature set.

### LSO7-SB5 - Inlay hint intents and resolution

**Independently testable outcome:** Inlay hints are authored semantic intents with stable identity and optional lazy resolution.

**Architecture:**

- Define parameter/type/chaining/framework hint kinds and applicability.
- Carry authored anchor, label parts, target refs, padding policy, and optional resolve key.
- Keep edits/commands as LSO8/LRA0 intents.

**Expected changes:**

- Migrate native/provider inlay sources into normalized hints.
- Implement exact capability/config filtering.

**Discriminating proof:**

- Hint identity remains stable across rendering/encoding.
- Disabled kinds and inapplicable profiles perform zero work.

### LSO7-SB6 - Rendering adapters, caching, and bounded work

**Independently testable outcome:** Editor renderers preserve fragment semantics while list/resolve caches remain exact and bounded.

**Architecture:**

- Render Markdown/plain/signature/inlay protocol at edge with escaping and encoding.
- Cache by full subject/policy/provider/native/profile basis and complete-only admission.
- Count provider/native queries, fragments, allocations, retained docs.

**Expected changes:**

- Add LSP adapter and VIM/PER0 matrix.
- Release stale provider fragments/resolve keys on epoch change.

**Discriminating proof:**

- Cross-renderer snapshots preserve fragment content/provenance.
- Warm requests avoid repeated semantic/provider work where supported and memory plateaus.

## Data, identity, invalidation, and publication laws

- Core operation identity is rooted in project/profile/source basis, semantic subject, operation kind, demand, and capability epoch; presentation encoding is not semantic identity.
- Generated coordinates, provider handles, and editor DTOs may exist only inside typed adapters and never become public semantic truth.
- Cancellation, stale, superseded, partial, NeedInputs, unsupported, and ambiguous outcomes remain distinct and are never collapsed to empty success.
- All returned targets and edits carry exact authored provenance and validate the snapshot/mapping chain used to derive them.
- Presentation fragment identity is semantic kind/subject/authority/basis, not rendered Markdown text.
- Source-literal annotations are explicitly distinguished from resolved types.
- Provider text is observation data and cannot enter native semantic cache identity.

## Migration and cutover

- Introduce fragment model behind current hover, then migrate signature and inlay.
- Characterize provider on/off and framework child-hover behavior.
- Delete feature-local merge/early-return/render logic after conformance.

## Deletions

- Delete provider-baked hover/signature/inlay core DTOs and implicit merge precedence.
- Delete early-return paths that bypass shared composition.
- Delete core string hacks used to infer semantic provenance.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

## Forbidden designs

- Rendered Markdown/text as semantic result identity.
- Provider response order deciding fragment authority.
- Generated helper names exposed as semantic subjects.
- Direct TextEdits/commands without typed intents.
- Whole-workspace/provider work for a leaf presentation request.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

## Acceptance IDs and discriminating proof

- **LSO7-AC-FRAGMENTS:** every fragment has exact kind/authority/provenance and deterministic policy.
- **LSO7-AC-HOVER:** provider/native/framework/source fragments compose without hidden early returns.
- **LSO7-AC-SIGNATURE:** canonical signatures/active parameter are provider-order independent.
- **LSO7-AC-INLAY:** hints use authored anchors, stable IDs, truthful capabilities, and zero work when disabled.
- **LSO7-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **LSO7-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **LSO7-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **LSO7-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Performance and bounded work

- Fragment assembly is proportional to demanded presentation and does not materialize unrelated public type graphs.
- Target ceiling: 800 production LOC, 8 production files, and 2 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Mandatory rescope and abort conditions

- Abort if a semantic distinction can only be represented by formatted provider text.
- Abort if provider/native contradictory authority cannot be settled by explicit policy.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. Fragment policy mutation tests and cross-renderer snapshots.
1. Hover/signature/inlay provider/profile/recovery/coexistence fixtures.
1. Stale provider/cancel/cache/allocation/memory and zero-work tests.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Consumers and unlocks

- Feeds LSO9 presentation conformance and thin editor adapters.
- Reuses LSO2 targets and PUB0 public contracts.
- Provides stable presentation substrate for future frameworks.

## Source reconciliation

- Legacy hover/provider merge behavior and TypeScript correction-overlay display clauses.
- Global component hover/navigation design details.

Durable clauses are transferred as digest-bound requirement atoms. Historical path archaeology, obsolete branches, and implementation journals are not copied into the charter. Git history remains the archive.


---

<!-- unified-charter-v2
id=LSO8
name=Authored edit transaction engine for rename, fixes, and imports
phase=expansion
train=expansion.language-service
product=language_service
kind=implementation
semantic_role=delivery
class=successor
predecessors=LSO1,LSO5,LSO6,LRA0,ENCL0
conditional_predecessors=
owner=expansion.language-service:one authored-coordinate semantic-operation and edit-transaction authority
conflict_domains=diagnostic_action_service,mapping_geometry,source_lineage,lsp_publication
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
charter=charters/expansion-language-service/LSO8.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# LSO8 - Authored edit transaction engine for rename, fixes, and imports

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

The internal subblocks below are binding decomposition and review checkpoints. They do not receive independent dispatch, leases, receipts, or deletion ownership unless the pre-scope architect proves that one subblock is independently acceptable or the block crosses a mandatory rescope trigger. In that case, amend the DAG before production mutation rather than treating a train-sized subblock as an implementation checklist.

## Independently acceptable outcome

Implement the sole authored edit transaction engine for semantic rename plans, completion/import intents, diagnostic fixes, code actions, and future refactors. It validates exact document/project preconditions, resolves insertion anchors and mapping provenance, detects overlap/conflict, classifies safety, and materializes one atomic multi-file transaction.

The current owner is **direct LSP WorkspaceEdit construction, provider-generated file edits, per-feature import re-anchoring, ad hoc overlap handling, and command-local filesystem writes**. The final and sole owner is **one AuthoredEditTransactionBuilder and transaction validator with exact basis/preconditions, deterministic edits, atomic application semantics, and thin protocol/filesystem adapters**.

## Architectural role and end state

LSO8 is intentionally separate from rename/completion/rules. Semantic producers state what authored change is intended; LSO8 proves where and whether it can be safely applied. This centralizes the highest-risk provenance and concurrency boundary.

## Expected production surfaces

- `crates/verter_actions` for transaction/edit intent/safety authority
- `crates/verter_session` for immutable source/project snapshot validation
- `crates/verter_span` and mapping owners for exact authored anchors
- `crates/verter_lsp` for WorkspaceEdit projection/application negotiation
- `packages`/CLI application service only for thin write adapters when opened

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.

## Named APIs and data boundaries

- `AuthoredEditTransaction`, `AuthoredEditTransactionId`, and `TransactionBasis`
- `AuthoredEdit`, `TextReplacementIntent`, `InsertionIntent`, `FileOperationIntent`
- `EditPrecondition::{Revision, Hash, OldText, TargetIdentity, AuthorityEpoch, MappingBasis}`
- `InsertionAnchor`, `ImportPlacementPolicy`, and `AnchorResolution`
- `EditConflict`, `OverlapClass`, `TransactionSafety`, and `TransactionRefusal`
- `TransactionApplyReceipt` and atomic write boundary

## Exact predecessor contracts

- **LSO1:** consume recovery/mapping capability so edits never target unstable synthetic regions.
- **LSO5:** consume complete semantic RenamePlan and replacement intents.
- **LSO6:** consume completion/additional/import intents with exact producer basis.
- **LRA0:** consume fix/action applicability and safe/suggested/unsafe classification.
- **ENCL0:** consume exact editor/LSP coordinate conversion at the boundary.

External custody: none beyond the package activation boundary.

## Binding architecture

- Every edit is authored-coordinate and validates source revision/hash plus semantic/mapping/authority basis appropriate to its origin.
- Semantic producers cannot materialize final TextEdits/WorkspaceEdits or write files.
- All intents are normalized, sorted, overlap-checked, and either accepted as one transaction or refused; partial safe application is forbidden.
- Insertion placement is syntax/structure-aware through explicit authored anchors, never line-zero, nearest position, or text regex.
- Provider-generated edits are evidence only until normalized to authored intents and validated against exact target-file context.
- Foreign-file mapping uses the foreign file snapshot/mapper/anchors; current-file context is never reused.
- Application is atomic at the adapter boundary or returned as a plan requiring an adapter capable of equivalent atomic precondition checks.

## Internal subblocks

### LSO8-SB1 - Transaction basis and precondition model

**Independently testable outcome:** Every transaction and edit names exact source/project/profile/authority/mapping basis and cannot apply after drift.

**Architecture:**

- Define transaction basis over immutable project/source snapshots.
- Require old-text/hash/revision and semantic target/authority preconditions as applicable.
- Separate planning identity from application receipt.

**Expected changes:**

- Add core transaction schemas and validation engine.
- Remove edit objects lacking basis/preconditions from semantic APIs.

**Discriminating proof:**

- Concurrent edit/config/profile/provider changes invalidate affected transaction.
- Unchanged basis revalidates deterministically.

### LSO8-SB2 - Intent normalization and deterministic ordering

**Independently testable outcome:** Heterogeneous rename/import/fix/action intents normalize to one authored edit vocabulary without losing origin or safety.

**Architecture:**

- Normalize replacements, insertions, deletions, file operations, annotations, and change groups.
- Preserve producer/rule/subject/target provenance.
- Sort by canonical file/range/kind/origin before conflict analysis.

**Expected changes:**

- Implement adapters from LSO5, LSO6, and LRA0 intents.
- Delete feature-specific final edit builders.

**Discriminating proof:**

- Input order permutations produce byte-identical transaction plans.
- Semantic provenance and safety survive normalization.

### LSO8-SB3 - Structural insertion anchors and import placement

**Independently testable outcome:** Imports/scripts/framework blocks are inserted only at exact syntax-owned authored anchors.

**Architecture:**

- Define existing-script/import-list/create-block/after-directive/before-declaration anchors.
- Resolve policy per profile/source structure and intent origin.
- Support create-script/setup only for explicitly authorized operation/profile contexts.

**Expected changes:**

- Consolidate completion and code-action import placement into one implementation.
- Remove text sniffing, file-top insertion, and caller-specific anchor construction.

**Discriminating proof:**

- Vue/Svelte/no-script/options/script-setup/module/foreign-file matrices place or refuse exactly.
- A missing/stale anchor never falls back to 0:0.

### LSO8-SB4 - Mapping and foreign-file edit validation

**Independently testable outcome:** Mapped provider edits use exact source/mapper/snapshot context for the target file and reject synthetic/unmappable ranges.

**Architecture:**

- Classify preamble/synthetic insertions before accepting strict mapping.
- Load foreign target mapper/line index/source snapshot independently.
- Require full range endpoint compatibility and mapping basis equality.

**Expected changes:**

- Centralize current/foreign/self-file mapping paths.
- Delete current-file mapper fallback and approximate range conversion.

**Discriminating proof:**

- Foreign preamble insertion cannot land at foreign/current file top.
- Stale/absent map boundary yields typed refusal.

### LSO8-SB5 - Overlap, conflict, safety, and atomicity

**Independently testable outcome:** A transaction either proves a deterministic conflict-free change set or returns explicit conflicts/refusal.

**Architecture:**

- Classify identical/coalescible/nested/conflicting overlaps.
- Coalesce only under exact intent-specific law such as ordered imports.
- Validate file operation/path collisions and cross-file dependencies.
- Require safe transactions complete; suggested/unsafe remain explicit.

**Expected changes:**

- Implement interval/conflict engine and transaction safety evaluator.
- Expose conflicts/related anchors without applying partial edits.

**Discriminating proof:**

- Overlap mutation matrix detects dropped/duplicated/reordered changes.
- Failure injection proves no half-applied multi-file transaction.

### LSO8-SB6 - Protocol/application adapters and receipts

**Independently testable outcome:** LSP/CLI/filesystem adapters preserve transaction semantics and preconditions while core remains protocol-independent.

**Architecture:**

- Project to WorkspaceEdit/documentChanges/change annotations only after validation.
- Negotiate client resource/file-operation capabilities truthfully.
- For CLI writes, stage/validate then atomically replace or refuse.

**Expected changes:**

- Implement thin adapters and immutable apply receipts.
- Delete command-local/direct writes displaced by shared transaction service.

**Discriminating proof:**

- LSP and CLI plan projections describe equivalent authored changes.
- Unsupported client capabilities return typed refusal, not degraded partial write.

### LSO8-SB7 - Transaction conformance, cancellation, and performance

**Independently testable outcome:** Edit planning/materialization is bounded, cancellable before apply, deterministic, and memory-safe.

**Architecture:**

- Count intents, files, mappings, anchors, conflicts, allocations, copies, staged bytes.
- Propagate cancellation through mapping/anchor/conflict stages; application commit is explicit.
- Generate operation/profile/provider/recovery matrix.

**Expected changes:**

- Add VIM/PER0 receipts and adversarial overlap/file-count tests.
- Release staged content/snapshots on refusal/cancel.

**Discriminating proof:**

- Warm unchanged plan validation avoids parse/provider work and retained bytes plateau.
- Cancellation before commit applies nothing and admits no complete receipt.

## Data, identity, invalidation, and publication laws

- Core operation identity is rooted in project/profile/source basis, semantic subject, operation kind, demand, and capability epoch; presentation encoding is not semantic identity.
- Generated coordinates, provider handles, and editor DTOs may exist only inside typed adapters and never become public semantic truth.
- Cancellation, stale, superseded, partial, NeedInputs, unsupported, and ambiguous outcomes remain distinct and are never collapsed to empty success.
- All returned targets and edits carry exact authored provenance and validate the snapshot/mapping chain used to derive them.
- Transaction identity includes normalized intent set and exact basis, not final LSP encoding.
- Only LSO8 or an explicitly delegated equivalent validator may produce applicable workspace edits.
- An apply receipt binds the exact transaction digest and observed preconditions.

## Migration and cutover

- Introduce transaction builder for one completion import path, then code actions, rename, and fixes.
- Characterize exact existing outputs but reject unsafe fallback behavior as intentional correction.
- Delete direct edit builders immediately after the last producer migrates.

## Deletions

- Delete direct WorkspaceEdit/TextEdit construction in semantic rename/completion/fix modules.
- Delete duplicate completion/code-action import re-anchor implementations, file-top fallbacks, current-file mapper reuse, and partial overlap application.
- Delete command-local non-atomic multi-file write paths.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

## Forbidden designs

- Unchecked raw edits or filesystem writes from semantic producers.
- Approximate/nearest/0:0 insertion or mapping fallback.
- Partial application of a plan claimed safe.
- Current-file mapper/anchor used for a foreign edit.
- Regex/text-sniff import placement or silent overlap resolution.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

## Acceptance IDs and discriminating proof

- **LSO8-AC-PRECONDITIONS:** every edit/transaction validates exact source/semantic/mapping/authority basis.
- **LSO8-AC-ANCHORS:** structural insertion matrices place or refuse without fallback.
- **LSO8-AC-FOREIGN:** foreign edits use foreign context and cannot land in current/file-top synthetic positions.
- **LSO8-AC-CONFLICT:** overlap/path/failure injection proves deterministic all-or-nothing behavior.
- **LSO8-AC-ADAPTERS:** LSP/CLI projections preserve one transaction digest and semantics.
- **LSO8-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **LSO8-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **LSO8-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **LSO8-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Performance and bounded work

- Planning/materialization work is linear in normalized intents/files plus bounded mapping/parse facts; no hidden provider/semantic recheck.
- Target ceiling: 800 production LOC, 8 production files, and 2 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Mandatory rescope and abort conditions

- Abort if an intent lacks enough authored provenance/preconditions to validate.
- Abort if an adapter cannot preserve required atomicity and a best-effort partial fallback is proposed.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. Intent normalization/order/overlap/conflict mutation matrix.
1. Import anchor and current/foreign/self/no-script/recovery/stale-map suites.
1. Concurrent edit/config/profile drift, client capability, atomic failure injection, cancellation, allocation, staged-byte, and memory tests.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Consumers and unlocks

- Provides applicable edit transactions to LSP/CLI and future refactors.
- Unlocks complete LSO9 conformance.
- Serves NCK7/LRA0 diagnostic fix intents.

## Source reconciliation

- `docs/arch/provider-completion-resolve-design.md` import re-anchor clauses.
- Framework import-placement and goto-definition edit provenance decisions.

Durable clauses are transferred as digest-bound requirement atoms. Historical path archaeology, obsolete branches, and implementation journals are not copied into the charter. Git history remains the archive.


---

<!-- unified-charter-v2
id=LSO9
name=Vertical language-service conformance and coexistence matrix
phase=expansion
train=expansion.language-service
product=language_service
kind=proof
semantic_role=delivery
class=successor
predecessors=LSO1,LSO3,LSO4,LSO5,LSO6,LSO7,LSO8,VIM1,COX0
conditional_predecessors=NCK7:when-opened
owner=expansion.language-service:one authored-coordinate semantic-operation and edit-transaction authority
conflict_domains=vertical_manifest,capability_catalog,performance_evidence
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
charter=charters/expansion-language-service/LSO9.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# LSO9 - Vertical language-service conformance and coexistence matrix

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

The internal subblocks below are binding decomposition and review checkpoints. They do not receive independent dispatch, leases, receipts, or deletion ownership unless the pre-scope architect proves that one subblock is independently acceptable or the block crosses a mandatory rescope trigger. In that case, amend the DAG before production mutation rather than treating a train-sized subblock as an implementation checklist.

## Independently acceptable outcome

Generate and execute the authoritative vertical language-service conformance matrix across operations, profiles, providers, recovery states, coexistence modes, coordinate encodings, and consumer surfaces. LSO9 certifies operation families and identifies residual external ownership; it implements no new feature semantics.

The current owner is **scattered feature tests, provider-specific fixtures, legacy editor designs, manually maintained capability claims, and sampled integration checks**. The final and sole owner is **one versioned operation/profile/provider conformance manifest, deterministic generated tests/receipts, and exact capability maturity table**.

## Architectural role and end state

LSO9 is the proof boundary before deletion. It prevents “works in one editor/provider” from being mistaken for a universal language-service architecture. Missing semantic behavior reopens LSO1-LSO8 or vertical owners.

## Expected production surfaces

- `docs/arch/refactor/rev11` VIM/catalog/generated authority
- `crates/verter_session`/`crates/verter_lsp` conformance harnesses
- `crates/verter_vue_conformance` and `crates/verter_svelte_conformance`
- `crates/verter_type_runtime` gated provider canaries
- `crates/verter_bench` and audit receipts

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.

## Named APIs and data boundaries

- `LanguageServiceConformanceManifest`, `OperationConformanceRow`, and stable row IDs
- `OperationCoverage::{Required, Optional, Unsupported, ExternalOwner}`
- `ConformanceExpectation` over targets/occurrences/fragments/intents/outcomes/work
- `ProviderTopology`, `RecoveryState`, `CoexistenceMode`, and `EncodingProfile`
- `OperationCertificationReceipt` and generated capability/maturity input

## Exact predecessor contracts

- **LSO1:** consume tolerant recovery and two-rail behavior.
- **LSO3:** consume navigation engine.
- **LSO4:** consume references/hierarchy occurrence planner.
- **LSO5:** consume semantic rename planning.
- **LSO6:** consume completion/resolve intents.
- **LSO7:** consume presentation composition.
- **LSO8:** consume authored transaction engine.
- **VIM1:** consume deterministic manifest compiler/conformance generator.
- **COX0:** consume exact coexistence/participation modes.
- **NCK7:when-opened:** when opened, include the shared native diagnostic service in operation/surface conformance; when unopened, prove no checker dependency or hidden work.

External custody: none beyond the package activation boundary.

## Binding architecture

- The manifest enumerates semantic expectations and operational outcomes, not just request success or message counts.
- Rows are stable, versioned, hermetic by default, and generated into tests/receipts/capability maturity.
- Provider topology is a dimension, not separate hand-authored suites; unavailable topologies are explicit.
- Recovery/coexistence/encoding/profile dimensions cover exact applicable subsets and zero-work requirements.
- Performance expectations use equivalent-work counters and bounded allocations/retention, not wall time alone.
- A green matrix certifies only listed operation/profile rows; unsupported/external ownership remains truthful.
- When NCK7 is unopened, diagnostics rows remain external/native-parser/lint according to existing authority and perform zero NCK work.

## Internal subblocks

### LSO9-SB1 - Manifest schema and stable row taxonomy

**Independently testable outcome:** Every required operation/profile behavior has one stable row and exact applicability.

**Architecture:**

- Define row dimensions, expected semantic IDs/results/outcomes/work, fixtures, owners, and maturity.
- Separate required/optional/unsupported/external ownership.
- Version row changes and prevent silent deletion.

**Expected changes:**

- Extend VIM0/VIM1 generator for language-service rows.
- Import durable legacy acceptance cases into rows.

**Discriminating proof:**

- Bijection/completeness guard catches missing/duplicate/renumbered rows.
- Reordering inputs does not change row identity/generated artifacts.

### LSO9-SB2 - Hermetic fixture and oracle corpus

**Independently testable outcome:** Core conformance runs without network/editor installation and uses exact authored expected products.

**Architecture:**

- Create compact Vue/Svelte/native/project/barrel/recovery/global-component/edit fixtures.
- Store semantic target/occurrence/fragment/intent expectations in typed snapshots.
- Use provider oracles only behind exact gated topology.

**Expected changes:**

- Generate fixture runners for operations.
- Delete redundant branch-era fixture prose after transfer.

**Discriminating proof:**

- Fixtures are deterministic across machines/paths.
- A planted wrong target/role/anchor/intent is detected.

### LSO9-SB3 - Provider, profile, recovery, and coexistence matrix

**Independently testable outcome:** Each applicable topology has exact behavior and capability/zero-work evidence.

**Architecture:**

- Enumerate provider off/tsgo/tsserver/extension/shared where available.
- Enumerate Vue/Svelte and future profile rows, clean/broken states, Full/WorkspaceOnly/Disabled/auto coexistence.
- Cover UTF-8/UTF-16, CRLF, emoji, embedded maps.

**Expected changes:**

- Generate matrix cases and receipts.
- Mark unsupported/shared harness gaps explicitly rather than assuming parity.

**Discriminating proof:**

- Capability claims equal passing applicable rows.
- Disabled/inapplicable combinations prove zero parse/index/provider/semantic work.

### LSO9-SB4 - Consumer-surface equivalence

**Independently testable outcome:** LSP/custom methods/CLI/library surfaces preserve the same core semantic products where opened.

**Architecture:**

- Compare core IDs, basis, completeness, provenance, intents and outcomes before rendering.
- Allow presentation/encoding differences only at adapter layer.
- Include NCK7 diagnostics conditionally.

**Expected changes:**

- Generate cross-surface adapters/tests.
- Identify any surface-specific semantic DTO as blocking.

**Discriminating proof:**

- Equivalent operations match core results across surfaces.
- Missing inputs yield NeedInputs/unsupported consistently.

### LSO9-SB5 - Performance, cancellation, churn, and memory evidence

**Independently testable outcome:** Certified operation rows are bounded under cold/warm/incremental/cancel/churn workloads.

**Architecture:**

- Capture parse/index/resolve/provider/map/target/occurrence/intent counters, allocations, latency distributions, RSS.
- Run repeated edits, profile/provider changes, project open/close, abandoned cursors/resolve keys.
- Require incremental equals fresh.

**Expected changes:**

- Generate PER0 scenarios and receipts.
- Route regressions to owning implementation node.

**Discriminating proof:**

- Warm work and retained memory meet ratified thresholds.
- Cancelled/stale/partial work is never published/admitted as complete.

### LSO9-SB6 - Certification, capability generation, and residual ledger

**Independently testable outcome:** Passing rows produce immutable operation certification and truthful capability maturity; gaps remain named.

**Architecture:**

- Bind implementation/manifest/fixture/provider/toolchain/evidence digests.
- Generate PUB0/COX0 capability input.
- Record residual external/unsupported rows with owner and reopening criteria.

**Expected changes:**

- Emit certification receipts and generated matrix docs.
- Prevent manual capability promotion.

**Discriminating proof:**

- Any source/row/evidence change invalidates receipt.
- Public claims exactly match certified applicable rows.

## Data, identity, invalidation, and publication laws

- Core operation identity is rooted in project/profile/source basis, semantic subject, operation kind, demand, and capability epoch; presentation encoding is not semantic identity.
- Generated coordinates, provider handles, and editor DTOs may exist only inside typed adapters and never become public semantic truth.
- Cancellation, stale, superseded, partial, NeedInputs, unsupported, and ambiguous outcomes remain distinct and are never collapsed to empty success.
- All returned targets and edits carry exact authored provenance and validate the snapshot/mapping chain used to derive them.
- Conformance row identity is stable and independent of generated test file location.
- Certification is row-scoped and cannot be inferred from aggregate pass percentage.
- External provider observations never become hermetic expectations without explicit pinned oracle basis.

## Migration and cutover

- Seed manifest from current tests/legacy clauses, then close gaps per operation owner.
- Run hermetic matrix continuously and gated provider/real-editor canaries separately.
- Do not delete legacy routes until applicable required rows are certified.

## Deletions

- Delete duplicated feature/provider test matrices superseded by generated rows only after coverage equivalence.
- Delete manual capability/maturity tables and sampled parity claims.
- Delete branch-era legacy design acceptance prose after atom/row transfer.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

## Forbidden designs

- Aggregate green count used as semantic certification.
- Network-dependent mandatory tests.
- Manual row IDs/capability promotion outside the generator.
- Ignoring unsupported topology while claiming universal parity.
- Fixing semantic defects locally in the proof block.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

## Acceptance IDs and discriminating proof

- **LSO9-AC-MANIFEST:** exact required row completeness/bijection and stable generation.
- **LSO9-AC-MATRIX:** provider/profile/recovery/coexistence/encoding applicability and zero-work are explicit.
- **LSO9-AC-SURFACES:** opened consumers preserve core semantic products/outcomes.
- **LSO9-AC-PERF:** incremental/fresh/cancel/churn/allocation/RSS receipts satisfy PER0.
- **LSO9-AC-CAPABILITY:** generated public capability/maturity equals certified rows exactly.
- **LSO9-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **LSO9-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **LSO9-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **LSO9-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Performance and bounded work

- Conformance overhead is test/offline; production capability lookup uses immutable generated tables and performs no fixture/oracle work.
- Target ceiling: 800 production LOC, 8 production files, and 2 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Mandatory rescope and abort conditions

- Abort certification if a required row lacks authoritative expected semantics or exact applicable topology.
- Abort if a proof failure is patched in the harness instead of owning implementation.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. Manifest generator determinism/bijection/source coverage.
1. Full hermetic operation matrix and gated provider topology matrix.
1. Cross-surface, zero-work, incremental/fresh, cancellation, churn, allocation, latency, and RSS receipts.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Consumers and unlocks

- Unlocks LSO10 terminal/deletion.
- Feeds PUB0/COX0 capability truth and future vertical release manifests.
- Provides exact residual ownership ledger.

## Source reconciliation

- All legacy navigation/completion/recovery/editor acceptance clauses classified by reconciliation.
- VIM/PER0/COX0 authority.

Durable clauses are transferred as digest-bound requirement atoms. Historical path archaeology, obsolete branches, and implementation journals are not copied into the charter. Git history remains the archive.


---

<!-- unified-charter-v2
id=LSO10
name=Language-service convergence and legacy route deletion
phase=expansion
train=expansion.language-service
product=language_service
kind=terminal
semantic_role=delivery
class=successor
predecessors=LSO9,PER0,UAI0,UAP0,BR0
conditional_predecessors=
owner=expansion.language-service:one authored-coordinate semantic-operation and edit-transaction authority
conflict_domains=semantic_authority,mapping_geometry,lsp_publication,program_authority
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
release_gating=product
source_refs=live:docs/arch/refactor/rev11/sources/legacy-arch-reconciliation.md
external_requirements=
activation_gate=ORC0
charter=charters/expansion-language-service/LSO10.md
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# LSO10 - Language-service convergence and legacy route deletion

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

The internal subblocks below are binding decomposition and review checkpoints. They do not receive independent dispatch, leases, receipts, or deletion ownership unless the pre-scope architect proves that one subblock is independently acceptable or the block crosses a mandatory rescope trigger. In that case, amend the DAG before production mutation rather than treating a train-sized subblock as an implementation checklist.

## Independently acceptable outcome

Converge and promote the language-service product after exact required conformance, performance, consumer, and deletion proofs. Remove every displaced feature route, mapper fallback, raw-edit path, duplicate target/occurrence/presentation authority, and legacy architecture document. LSO10 adds no new feature semantics.

The current owner is **accepted LSO nodes plus residual feature-specific handlers, provider/native merge paths, mapping fallbacks, raw edit builders, duplicated tests/docs, and manual capability claims**. The final and sole owner is **one promoted language-service product receipt, one operation capability snapshot, and structurally enforced use of the canonical target/occurrence/presentation/transaction authorities**.

## Architectural role and end state

LSO10 is a terminal proof and deletion block. Discovering missing semantic behavior, unsupported required rows, or incorrect performance sends work back to LSO1-LSO9 or a vertical owner; terminal cleanup may not implement it opportunistically.

## Expected production surfaces

- Language-service/session/LSP/type-runtime/action modules named in the terminal route inventory
- Rev11 authority, generated conformance/capability/deletion receipts
- Legacy `docs/arch` feature and editor architecture paths classified for deletion/relocation
- Performance/audit evidence and public capability tables

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.

## Named APIs and data boundaries

- `LanguageServiceProductReceipt`
- `LanguageServiceCapabilitySnapshot` and exact certified row/implementation digests
- `LegacyRouteDeletionManifest` and no-bypass architecture guard
- `LanguageServiceResidualLedger` for unsupported/external operation families

## Exact predecessor contracts

- **LSO9:** consume exact required operation certifications, residual ledger, and generated capability table.
- **PER0:** consume terminal equivalent-work/latency/allocation/RSS methodology.
- **UAI0:** consume final identity/carrier/parser/coordinate contract lock.
- **UAP0:** consume capability/coexistence/rule-action/public contract lock.
- **BR0:** consume successor product promotion authority.

External custody: none beyond the package activation boundary.

## Binding architecture

- Terminal work proves, deletes, and promotes; it does not add targets, occurrence roles, rename policy, candidate logic, presentation semantics, or edit algorithms.
- Every legacy/bypass route has one exact deletion owner and a structural negative guard.
- Residual external/unsupported ownership is explicit and capability-visible.
- Deleting TypeScript provider feature routes is per certified operation family; providers remain for residual owners.
- Product receipt binds exact DAG/charter/manifest/implementation/evidence/capability/deletion digests.
- Public/editor documentation is relocated outside `docs/arch`; Git history is the archive.

## Internal subblocks

### LSO10-SB1 - Terminal certification and residual closure

**Independently testable outcome:** All required rows have accepted receipts and every non-required row has exact residual owner/maturity.

**Architecture:**

- Validate LSO9 manifest/receipts against current implementation tree.
- Reject stale/partial/sampled certification.
- Generate residual ledger and public capability snapshot.

**Expected changes:**

- Run terminal validator and freeze candidate.
- Reopen owning node for any gap.

**Discriminating proof:**

- Every required row maps to exact implementation/evidence receipts.
- Capability claims contain no unproven operation/profile topology.

### LSO10-SB2 - Feature route and authority deletion

**Independently testable outcome:** No consumer/handler bypasses canonical LSO authorities for migrated operations.

**Architecture:**

- Generate call/path/symbol inventory for old target, occurrence, merge, mapper, presentation, edit routes.
- Delete routes/stores/flags/helpers and register negative guards.
- Retain only typed provider adapters behind canonical services.

**Expected changes:**

- Perform bounded deletions by exact manifest.
- Remove dead compatibility shims in same candidate.

**Discriminating proof:**

- Planting each deleted route fails architecture tests.
- No direct provider/raw edit/current-file mapper bypass remains.

### LSO10-SB3 - Legacy architecture cleanup and product-doc relocation

**Independently testable outcome:** All durable clauses are in Rev11 and product/editor docs live beside products rather than as competing architecture.

**Architecture:**

- Validate blob-SHA disposition for every legacy path.
- Relocate as-built editor usage/packaging docs to editor/product directories.
- Delete historical plans/backlogs/ledgers after source atom transfer.

**Expected changes:**

- Apply legacy disposition and permanent tree guard.
- Do not create archive/old/legacy docs directories.

**Discriminating proof:**

- No unclassified file remains outside Rev11 under docs/arch.
- No live authority references deleted paths.

### LSO10-SB4 - Cross-surface and coexistence terminal

**Independently testable outcome:** Opened editor/public surfaces and coexistence modes consume canonical operations and capability withdrawal correctly.

**Architecture:**

- Run LSO9 exact matrix on landing-frozen candidate.
- Test dynamic register/unregister, provider/profile transitions, stale clearing, and zero-work disabled modes.
- Verify consumer render differences do not alter core semantics.

**Expected changes:**

- Capture terminal surface receipt.
- Delete manual client branches displaced by generated descriptors.

**Discriminating proof:**

- Only overlapping capabilities withdraw under auto coexistence.
- No stale results survive authority/capability transitions.

### LSO10-SB5 - Performance/cancellation/memory terminal

**Independently testable outcome:** The product is bounded under representative cold/warm/incremental/churn/parallel/cancel workloads.

**Architecture:**

- Run equivalent-work counters and latency/allocation/RSS gates.
- Test project open/close, provider swap, broken edits, cursor/resolve-key abandonment, large candidate sets.
- Require memory release and no hidden eager work.

**Expected changes:**

- Capture PER0 terminal receipt.
- Reopen owning node for regressions; no blind terminal micro-optimization.

**Discriminating proof:**

- Warm operations meet ratified work thresholds.
- Long churn plateaus and teardown releases retained state.

### LSO10-SB6 - Product receipt and promotion

**Independently testable outcome:** Promotion is exact, immutable, honest, and invalidated by any authority/evidence change.

**Architecture:**

- Bind DAG/charter/source/manifest/implementation/review/gate/performance/deletion/capability digests.
- State residual provider/unsupported operations explicitly.
- Publish maturity/capability through PUB0/COX0.

**Expected changes:**

- Emit product receipt and successor promotion state.
- Remove temporary migration flags.

**Discriminating proof:**

- Receipt validation fails on any changed input.
- Public claims exactly match certified scope.

## Data, identity, invalidation, and publication laws

- Core operation identity is rooted in project/profile/source basis, semantic subject, operation kind, demand, and capability epoch; presentation encoding is not semantic identity.
- Generated coordinates, provider handles, and editor DTOs may exist only inside typed adapters and never become public semantic truth.
- Cancellation, stale, superseded, partial, NeedInputs, unsupported, and ambiguous outcomes remain distinct and are never collapsed to empty success.
- All returned targets and edits carry exact authored provenance and validate the snapshot/mapping chain used to derive them.
- LSO10 may not introduce a new operation family or semantic algorithm.
- A retained provider adapter must have an exact residual operation owner and capability row.
- Deletion receipts bind both absence and structural rejection.

## Migration and cutover

- Run only after LSO9 required certifications are accepted.
- Freeze candidate, run route/source inventory, perform deletions/relocations, rerun complete gates/reviews.
- Stop and reopen predecessors for any semantic/performance gap.

## Deletions

- Delete all displaced language-service routes/stores/flags/helpers/tests/docs named by terminal manifests.
- Delete remaining mapping/0:0/nearest/current-file/raw-edit fallbacks.
- Delete manual capability claims and migration shims.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

## Forbidden designs

- Implementing missing semantics in terminal cleanup.
- Retaining duplicate routes “for safety”.
- Claiming universal/full parity beyond certified rows.
- Deleting residual provider capabilities without separate certification.
- Archiving legacy docs under another docs/arch folder.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

## Acceptance IDs and discriminating proof

- **LSO10-AC-CERTIFIED:** every required operation/profile topology has current exact certification.
- **LSO10-AC-DELETED:** route/source manifests prove absence and structural rejection of displaced authority.
- **LSO10-AC-SURFACES:** opened consumers/coexistence modes pass exact terminal matrix.
- **LSO10-AC-PERF:** equivalent-work/latency/allocation/RSS/cancel/churn terminal passes.
- **LSO10-AC-HONEST:** residual provider/unsupported ownership and public capability claims are exact.
- **LSO10-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **LSO10-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **LSO10-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **LSO10-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Performance and bounded work

- Terminal thresholds are ratified equivalent-work replacement thresholds, not unsupported blanket zero-delta claims.
- Target ceiling: 300 production LOC, 3 production files, and 1 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Mandatory rescope and abort conditions

- Abort if route/source inventory is incomplete or deletion cannot be structurally guarded.
- Abort if any required certification/evidence receipt is stale.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. Full authority/source/route/deletion/capability validation.
1. Complete LSO9 matrix plus terminal performance/cancellation/churn/memory suite.
1. Configured architecture review and immutable product receipt validation.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Consumers and unlocks

- Promotes the authored-coordinate language-service product.
- Provides stable substrate for future framework/editor operations.
- Does not by itself retire external TypeScript semantics outside certified operation families.

## Source reconciliation

- All LSO charters, LSO9 manifests/receipts, legacy disposition, PER0/UAI0/UAP0/BR0 contracts.

Durable clauses are transferred as digest-bound requirement atoms. Historical path archaeology, obsolete branches, and implementation journals are not copied into the charter. Git history remains the archive.


# Module `expansion-engine-provisioning`


---

<!-- unified-charter-v2
id=EPR0
name=External engine provisioning policy and trust constitution
phase=expansion
train=expansion.engine-provisioning
product=engine_provisioning
kind=constitution
semantic_role=delivery
class=successor
predecessors=UAK1,CFG0,H2,PUB0,TCM4
conditional_predecessors=
owner=expansion.engine-provisioning:explicit policy-controlled engine acquisition, resolution, and activation authority
conflict_domains=provider_lifecycle,public_protocol,program_authority
resource_class=docs-light
review_profile=security-3
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
charter=charters/expansion-engine-provisioning/EPR0.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# EPR0 - External engine provisioning policy and trust constitution

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

The internal subblocks below are binding decomposition and review checkpoints. They do not receive independent dispatch, leases, receipts, or deletion ownership unless the pre-scope architect proves that one subblock is independently acceptable or the block crosses a mandatory rescope trigger. In that case, amend the DAG before production mutation rather than treating a train-sized subblock as an implementation checklist.

## Independently acceptable outcome

Ratify an explicit external-engine provisioning and trust constitution. The policy may authorize project-local, system, editor-shared, managed-download, bundled-sidecar, or no automatic acquisition. Network and bundled channels remain closed until separately authorized; missing engines produce typed outcomes rather than hidden fallback.

The current owner is **partially implemented discovery tiers, blocked future documents, release-package invariants, environment overrides, editor sharing, and implicit product assumptions**. The final and sole owner is **one captured EngineProvisioningPolicy with explicit source authorization, trust/update/offline/enterprise law, typed outcomes, and separate acquisition/resolution/activation authorities**.

## Architectural role and end state

EPR0 prevents implementation convenience from deciding security and product policy. It records whether Verter may download or bundle executable engines, which origins are trusted, how enterprise/offline environments behave, and what honest capability means when no engine is available.

## Expected production surfaces

- Rev11 authority/catalogs and declarative configuration under CFG0
- `crates/verter_identity` and `crates/verter_protocol` for policy/source/outcome identities
- `crates/verter_session`/ProviderHub future policy consumption
- `crates/verter_tsgo_api`/`crates/verter_type_runtime` as implementation consumers only
- release/packaging/editor products as conditionally authorized channels

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.

## Named APIs and data boundaries

- `EngineProvisioningPolicy`, `EngineSourcePolicy`, and `EngineSourceKind`
- `EngineAcquisitionPermission::{Forbidden, ManualOnly, Allowed}`
- `EngineUpdatePolicy`, `EngineOfflinePolicy`, `EngineProxyPolicy`, and `EngineRollbackPolicy`
- `EngineNeed`, `EngineRequirement`, and `EngineProvisioningOutcome`
- `TrustedEngineOrigin`, `EngineTrustRootId`, and policy epoch
- `EngineCapabilityState` and truthful source/availability reporting

## Exact predecessor contracts

- **UAK1:** consume universal-tooling constitution and product split.
- **CFG0:** consume declarative captured ecosystem configuration and project/profile selection.
- **H2:** consume ProviderHub project-scoped binding lifecycle.
- **PUB0:** consume typed public outcomes and truthful capability vocabulary.
- **TCM4:** consume certified TypeScript engine binding and observation identity.

External custody: none beyond the package activation boundary.

## Binding architecture

- Automatic download and bundled distribution are product/security choices, not implementation defaults.
- A valid policy may forbid both EPR2 and EPR3 while still requiring deterministic manual/project/system/editor discovery.
- Source order and authorization are explicit data; environment overrides cannot silently bypass forbidden source classes.
- Network behavior is opt-in, observable, proxy/enterprise compatible, cancellable, and absent from ordinary resolution when disallowed.
- Bundled artifacts require explicit shipping ownership, license/provenance/size/update policy, and platform coverage.
- Missing engine yields NeedInputs/Unsupported/Unavailable according to context and never a fake “provider off but success” result.
- Native checker/language-service certified families may reduce engine demand, but do not change provisioning policy implicitly.

## Internal subblocks

### EPR0-SB1 - Engine source taxonomy and authorization matrix

**Independently testable outcome:** Every possible engine source has an exact authorization and trust owner.

**Architecture:**

- Define environment/manual, project-local, editor-shared, system/PATH, managed cache/download, and bundled sources.
- Separate discovery visibility from authorization to select/execute.
- Bind source policy by project/profile/enterprise context.

**Expected changes:**

- Add machine-readable source policy catalog and generated order table.
- Classify existing discovery tiers and release invariants.

**Discriminating proof:**

- Unclassified source or unauthorized source selection fails.
- Policy reorder changes exact epoch/selection expectations deterministically.

### EPR0-SB2 - Network and managed acquisition policy

**Independently testable outcome:** The constitution states whether network acquisition is forbidden, manual, or allowed and under what origin/TLS/proxy rules.

**Architecture:**

- Define allowed registries/origins/version channels/trust roots.
- Define proxy/custom CA/offline/air-gap/telemetry behavior.
- Require explicit first-use/update user/admin policy where applicable.

**Expected changes:**

- Register the external requirement consumed by optional EPR2.
- Record dependency and security review obligations.

**Discriminating proof:**

- With acquisition forbidden, no network code path is reachable and zero network work is proven.
- An unapproved origin or TLS bypass is structurally rejected.

### EPR0-SB3 - Bundled distribution policy

**Independently testable outcome:** Bundling is either explicitly forbidden or owned by named release artifacts with exact platform/license/update rules.

**Architecture:**

- Define which package/VSIX/platform artifact may contain an engine.
- Define size, SBOM, license notice, signature/provenance, update and rollback policy.
- Reconcile existing “tsgo never packaged” guards deliberately.

**Expected changes:**

- Register external requirement for EPR3 and release owner.
- Classify current packaging channels and invariants.

**Discriminating proof:**

- Unopened bundle channel remains absent and zero-work.
- A bundled artifact in an unauthorized package fails release validation.

### EPR0-SB4 - Offline, enterprise, and privacy behavior

**Independently testable outcome:** Air-gapped/corporate/proxy environments receive deterministic no-network behavior and actionable typed status.

**Architecture:**

- Define offline-first and deny-network modes.
- Define proxy/custom trust configuration without weakening verification.
- Prohibit telemetry or registry calls unrelated to explicit acquisition.

**Expected changes:**

- Add public configuration/status fields under CFG0/PUB0.
- Define support/escalation diagnostics without exposing secrets.

**Discriminating proof:**

- Offline/deny-network fixtures make no DNS/socket attempts.
- Proxy/auth secrets never enter logs, cache identity, or public result payloads.

### EPR0-SB5 - Version/update/rollback constitution

**Independently testable outcome:** Engine compatibility, update, pinning, downgrade, and rollback are explicit and reproducible.

**Architecture:**

- Define supported ranges and channel/pin precedence.
- Separate policy update from automatic artifact replacement.
- Require retained prior known-good receipt or explicit no-rollback policy.

**Expected changes:**

- Add policy schema/version and migration law.
- Define revocation/emergency disable path.

**Discriminating proof:**

- Policy changes invalidate resolution/activation exactly.
- Rollback never selects an unverified or incompatible artifact.

### EPR0-SB6 - Typed outcomes and capability truth

**Independently testable outcome:** Every provisioning state reports exact reason/source/actionability without collapsing failures.

**Architecture:**

- Define Available, NeedInputs, Forbidden, Offline, NotFound, Incompatible, IntegrityFailure, TrustFailure, Cancelled, and OperationalFailure.
- Separate no candidate from candidate rejected and candidate activation failed.
- Map states to honest capabilities and user/admin remediation.

**Expected changes:**

- Amend PUB0/COX0/provider status contracts.
- Delete vague “auto/off/no provider” ambiguity from authority.

**Discriminating proof:**

- Outcome serialization/mutations preserve distinctions.
- Capabilities are unavailable until selection and activation receipts exist.

### EPR0-SB7 - Legacy decision/source reconciliation

**Independently testable outcome:** Blocked provisioning documents become exact policy choices and DAG nodes, then are deleted.

**Architecture:**

- Transfer download/bundle tier facts and unresolved decisions into EPR charters/source atoms.
- Bind existing discovery/packaging guards and source SHAs.
- Record rejected/deferred channels as policy, not orphan future docs.

**Expected changes:**

- Populate legacy disposition and external requirement catalogs.
- Name security guards for no unverified execution/no hidden network.

**Discriminating proof:**

- Legacy deletion fails until every durable clause/decision has a target.
- A hidden network or package bypass mutation fails architecture/release guards.

## Data, identity, invalidation, and publication laws

- Engine acquisition, resolution, selection, and activation are distinct authorities with distinct receipts; no stage infers success from a later stage.
- No executable is run before exact origin, compatibility, integrity, and trust validation succeeds.
- Unavailable, unauthorized, incompatible, corrupt, offline, cancelled, and operationally failed outcomes remain distinct and capability-visible.
- All caches/installations are content/version/platform scoped, race-safe, and fail closed on symlink/reparse/permission/ownership violations.
- Policy snapshots are immutable, captured, and part of resolution identity.
- Secrets/credentials are runtime inputs and never stored in policy receipts or logs.
- A policy change cancels/supersedes in-flight acquisition/resolution/activation work.

## Migration and cutover

- Land policy with current usable sources represented exactly and network/bundle forbidden unless explicitly authorized.
- Do not implement EPR2/EPR3 in this block.
- Replace prose tier assumptions with generated source policy/status tables.

## Deletions

- Delete orphan blocked download/bundle architecture docs after source transfer.
- Delete undocumented tier ordering and implicit automatic acquisition claims.
- Delete conflicting packaging invariants only through explicit EPR0/EPR3 authorization, never incidentally.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

## Forbidden designs

- Silent network download, update, telemetry, or registry probe.
- Executing an unverified candidate because it exists on PATH/project/editor/cache.
- Treating bundle/download authorization as implied by prior tier numbering.
- Collapsing integrity/trust failure to not-found fallback.
- Secrets in logs, digests, receipts, or public DTOs.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

## Acceptance IDs and discriminating proof

- **EPR0-AC-POLICY:** every source/network/bundle/update/offline state has exact authorization and owner.
- **EPR0-AC-ZERO-NETWORK:** forbidden/manual-only policy proves zero automatic network attempts.
- **EPR0-AC-OUTCOMES:** all absence/rejection/failure states remain typed and capability-truthful.
- **EPR0-AC-LEGACY:** download/bundle docs and packaging invariants have complete source disposition.
- **EPR0-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **EPR0-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **EPR0-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **EPR0-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Performance and bounded work

- Policy evaluation and source filtering are allocation-free/bounded after snapshot construction and perform no filesystem/network work themselves.
- Target ceiling: 0 production LOC, 0 production files, and 0 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Mandatory rescope and abort conditions

- Abort if the maintainer decision on network or bundling is inferred rather than explicit.
- Abort if enterprise/offline behavior or executable trust root cannot be specified.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. Policy/source/external-requirement/source-coverage validation.
1. Negative no-network/no-bundle/no-unverified-execution architecture tests.
1. Typed outcome/capability/configuration schema and secret-redaction tests.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Consumers and unlocks

- Unlocks EPR1 and optional EPR2/EPR3.
- Provides exact policy consumed by EPR4 resolution and EPR5 activation.
- Allows a valid no-download/no-bundle end state.

## Source reconciliation

- `docs/arch/future/engine-provisioning-download-tier.md`.
- `docs/arch/future/engine-provisioning-bundled-sidecar-and-shipping-channel.md`.
- Current toolchain discovery/packaging guards and ProviderHub/TCM contracts.

Durable clauses are transferred as digest-bound requirement atoms. Historical path archaeology, obsolete branches, and implementation journals are not copied into the charter. Git history remains the archive.


---

<!-- unified-charter-v2
id=EPR1
name=Engine artifact identity, compatibility, integrity, and cache contract
phase=expansion
train=expansion.engine-provisioning
product=engine_provisioning
kind=contract
semantic_role=delivery
class=successor
predecessors=EPR0,VID0
conditional_predecessors=
owner=expansion.engine-provisioning:explicit policy-controlled engine acquisition, resolution, and activation authority
conflict_domains=provider_lifecycle,source_lineage,program_authority
resource_class=docs-light
review_profile=security-3
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
charter=charters/expansion-engine-provisioning/EPR1.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# EPR1 - Engine artifact identity, compatibility, integrity, and cache contract

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

The internal subblocks below are binding decomposition and review checkpoints. They do not receive independent dispatch, leases, receipts, or deletion ownership unless the pre-scope architect proves that one subblock is independently acceptable or the block crosses a mandatory rescope trigger. In that case, amend the DAG before production mutation rather than treating a train-sized subblock as an implementation checklist.

## Independently acceptable outcome

Define the exact identity, compatibility, origin, integrity, installation, cache, revocation, and validation contract for any executable engine candidate. This block is contract-only and applies uniformly to project/system/editor/download/bundled sources.

The current owner is **path/version probes, source-specific validation, bundle manifests, consume-only cache checks, package metadata, and ad hoc compatibility rules**. The final and sole owner is **one EngineArtifactDescriptor/ValidationReceipt law and one trusted cache/install layout consumed by every source adapter before selection or execution**.

## Architectural role and end state

EPR1 makes “found an engine path” insufficient. Every candidate must become an immutable artifact identity with origin and validation evidence, and every cache/install must be safe under concurrent writers, corruption, tampering, and revocation.

## Expected production surfaces

- `crates/verter_identity` for artifact/platform/origin/digest IDs
- `crates/verter_tsgo_api::toolchain` and `crates/verter_type_runtime` validation contracts
- `crates/verter_protocol` public status/provenance projections
- cache/install manifest schemas and release artifact metadata
- security/audit tests and revocation catalogs

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.

## Named APIs and data boundaries

- `EngineArtifactId`, `EnginePlatform`, `EngineFlavor`, and `EngineVersion`
- `EngineOrigin`, `EngineOriginReceipt`, and `EngineArtifactDescriptor`
- `EngineCompatibilityRequirement` and `EngineCompatibilityVerdict`
- `EngineIntegrityEvidence`, `EngineSignatureEvidence`, and `EngineValidationReceipt`
- `EngineInstallLayout`, `EngineReadyMarker`, `EngineCacheKey`, and `EngineCacheEntry`
- `EngineRejection`, `EngineRevocation`, and exact rejection reason codes

## Exact predecessor contracts

- **EPR0:** consume explicit source/trust/update/offline policy.
- **VID0:** consume orthogonal identities and exact-release law.

External custody: none beyond the package activation boundary.

## Binding architecture

- Artifact identity includes exact engine/version/flavor/platform/build/origin/content digest and policy-compatible metadata.
- Path is a locator, never identity; replacing bytes at one path invalidates validation.
- Compatibility is checked before execution and includes protocol/API/feature constraints, not version string alone.
- Integrity/signature/origin evidence is source-specific but normalized to one validation receipt.
- Cache/install entries are private, non-symlink/reparse, ownership/permission checked, immutable after READY, and atomically installed.
- READY is written last only after validation; incomplete/corrupt entries are never candidates.
- Revocation and policy epoch invalidate candidate/validation caches immediately.

## Internal subblocks

### EPR1-SB1 - Artifact and platform identity

**Independently testable outcome:** Every candidate has one collision-resistant structural identity independent of path.

**Architecture:**

- Define engine flavor/version/build/platform/ABI/protocol/content/origin components.
- Use full structural fields or content digest where digest is the artifact itself, not lossy replacement for semantic axes.
- Canonicalize platform triples and executable layout.

**Expected changes:**

- Add identity types and serialization/catalog schemas.
- Migrate source-specific version/path tuples.

**Discriminating proof:**

- Different bytes/build/origin/platform never alias.
- Same verified artifact reached through two locators canonicalizes appropriately while origin receipts remain distinct.

### EPR1-SB2 - Compatibility and feature contract

**Independently testable outcome:** An engine is selectable only when its exact API/protocol/features satisfy the requester.

**Architecture:**

- Define version ranges, protocol versions, command/API capabilities, project/toolchain compatibility.
- Separate compatible, unsupported, too-old/new, wrong-platform/flavor, and unknown.
- Bind compatibility policy version.

**Expected changes:**

- Centralize compatibility evaluator.
- Remove source adapter/version-string-only decisions.

**Discriminating proof:**

- Boundary/mutation matrix detects each incompatibility.
- Compatibility changes invalidate selection without reusing stale receipt.

### EPR1-SB3 - Origin, integrity, signature, and provenance evidence

**Independently testable outcome:** Validation proves what bytes were obtained, from which authorized channel, under which trust root.

**Architecture:**

- Normalize registry integrity, release checksum/signature/attestation, bundle manifest, manual/project/system evidence.
- Require digest over executed artifact and critical sidecar files.
- Record SBOM/license/provenance references where policy demands.

**Expected changes:**

- Implement receipt schemas and source adapter obligations.
- Ban self-asserted “trusted” booleans.

**Discriminating proof:**

- Byte mutation, origin substitution, signature/trust-root mismatch, and manifest omission fail.
- Receipt is deterministic and redacts secrets/local absolute roots where required.

### EPR1-SB4 - Safe cache/install layout and concurrent writers

**Independently testable outcome:** Install/cache entries cannot expose partial, mutable, symlinked, or attacker-controlled executables.

**Architecture:**

- Use private root, temp sibling, no-follow creation, ownership/permission checks, bounded extraction, atomic rename, READY-last.
- Define cross-process lock/loser cleanup and immutable versioned entries.
- Reject group/world-writable or reparse/symlink components.

**Expected changes:**

- Ratify layout consumed by EPR2/EPR3 and existing cache readers.
- Add corruption/quarantine cleanup policy.

**Discriminating proof:**

- Crash at every install step never yields a selectable partial entry.
- Concurrent installers converge to one verified entry without overwrite races.

### EPR1-SB5 - Validation cache and exact invalidation

**Independently testable outcome:** Expensive validation reuses receipts only while every artifact/origin/policy/revocation fact matches.

**Architecture:**

- Key by artifact locator stat identity/content evidence/origin/policy/revocation epoch.
- Revalidate mutable/manual/system/project locators as policy requires.
- Keep immutable downloaded/bundled entries fast after READY.

**Expected changes:**

- Implement bounded validation receipt cache and counters.
- Do not cache rejected unknowns across facts that could change.

**Discriminating proof:**

- Replace bytes/metadata/policy/revocation forces validation.
- Warm immutable validation performs zero rehash/stat beyond the ratified trust boundary.

### EPR1-SB6 - Revocation, corruption, and quarantine

**Independently testable outcome:** Known-bad or newly revoked artifacts are never selected and are handled without silent fallback.

**Architecture:**

- Define revocation catalog/epoch and emergency policy.
- Distinguish integrity failure from revocation/incompatibility/operational failure.
- Quarantine/remove only entries owned by managed channels; never mutate user project/system installs.

**Expected changes:**

- Add rejection/status/audit paths.
- Define retry/update/rollback interaction.

**Discriminating proof:**

- Revocation race cancels activation and invalidates caches.
- Managed corruption is quarantined; manual corruption is reported without destructive mutation.

### EPR1-SB7 - Public validation status and secret/path hygiene

**Independently testable outcome:** Users/operators receive actionable source/version/status without leaking secrets or unstable machine roots into portable receipts.

**Architecture:**

- Define public summary versus private diagnostic detail.
- Normalize/redact paths, proxy credentials, tokens, and trust material.
- Provide stable reason/action codes.

**Expected changes:**

- Amend PUB0 status schema and logs/audit.
- Add portability guards.

**Discriminating proof:**

- Golden tests contain no secrets/machine roots.
- Every rejection has stable typed reason and remediation class.

## Data, identity, invalidation, and publication laws

- Engine acquisition, resolution, selection, and activation are distinct authorities with distinct receipts; no stage infers success from a later stage.
- No executable is run before exact origin, compatibility, integrity, and trust validation succeeds.
- Unavailable, unauthorized, incompatible, corrupt, offline, cancelled, and operationally failed outcomes remain distinct and capability-visible.
- All caches/installations are content/version/platform scoped, race-safe, and fail closed on symlink/reparse/permission/ownership violations.
- Validation receipts are immutable and bind exact policy/trust/revocation epochs.
- Managed cache writer owns only managed roots; project/system/editor artifacts are read-only.
- A rejected candidate cannot be reclassified as not-found to continue fallback silently.

## Migration and cutover

- Characterize current source validators and bundle/cache manifests.
- Introduce normalized descriptor/receipt while preserving current source order under EPR0.
- Migrate all source adapters before EPR4 selection.

## Deletions

- Delete path/version-only candidate identity and duplicated compatibility/integrity decisions.
- Delete READY/manifest trust that does not bind executed bytes.
- Delete unsafe mutable/symlink-permissive cache paths.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

## Forbidden designs

- Path as artifact identity.
- Executing to discover compatibility before validation.
- Checksum/signature verification after installation/execution.
- Following symlinks/reparse points in managed install roots.
- Silent fallback after integrity/trust/revocation failure.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

## Acceptance IDs and discriminating proof

- **EPR1-AC-IDENTITY:** artifact identity collision/substitution matrix is exact.
- **EPR1-AC-COMPAT:** compatibility boundaries and feature requirements are mutation-tested.
- **EPR1-AC-INTEGRITY:** byte/origin/signature/manifest mutations fail before execution.
- **EPR1-AC-INSTALL:** crash/concurrency/symlink/permission tests never expose partial/untrusted entries.
- **EPR1-AC-REVOCATION:** revocation invalidates validation/selection/activation immediately.
- **EPR1-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **EPR1-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **EPR1-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **EPR1-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Performance and bounded work

- Immutable READY entries may use bounded receipt validation; mutable locators revalidate according to explicit policy without repeated full scans.
- Target ceiling: 0 production LOC, 0 production files, and 0 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Mandatory rescope and abort conditions

- Abort if any source cannot produce evidence sufficient for its authorized trust class.
- Abort if a managed install cannot be created with no-follow/atomic/private semantics on a supported platform.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. Artifact identity/compatibility/integrity/signature/origin mutation matrix.
1. Cross-platform cache layout, permission, symlink/reparse, crash, concurrency, quarantine, revocation tests.
1. Public status redaction/portability and warm validation work counters.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Consumers and unlocks

- Unlocks optional EPR2/EPR3 and required EPR4.
- Provides uniform validation receipts to selection/activation.
- Owns safe managed cache/install contract.

## Source reconciliation

- Existing toolchain discovery/bundle/cache validation code and future provisioning docs.
- VID0 exact-release and portability/security guards.

Durable clauses are transferred as digest-bound requirement atoms. Historical path archaeology, obsolete branches, and implementation journals are not copied into the charter. Git history remains the archive.


---

<!-- unified-charter-v2
id=EPR2
name=Managed download and verified atomic installation channel
phase=expansion
train=expansion.engine-provisioning
product=engine_provisioning
kind=implementation
semantic_role=delivery
class=successor
predecessors=EPR1,G5
conditional_predecessors=
owner=expansion.engine-provisioning:explicit policy-controlled engine acquisition, resolution, and activation authority
conflict_domains=provider_lifecycle,scheduler_admission,source_lineage
resource_class=rust-mixed
review_profile=security-3
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
optional=true
release_gating=non_release
source_refs=live:docs/arch/refactor/rev11/sources/legacy-arch-reconciliation.md
external_requirements=maintainer_managed_engine_acquisition
activation_gate=ORC0
charter=charters/expansion-engine-provisioning/EPR2.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# EPR2 - Managed download and verified atomic installation channel

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

The internal subblocks below are binding decomposition and review checkpoints. They do not receive independent dispatch, leases, receipts, or deletion ownership unless the pre-scope architect proves that one subblock is independently acceptable or the block crosses a mandatory rescope trigger. In that case, amend the DAG before production mutation rather than treating a train-sized subblock as an implementation checklist.

## Independently acceptable outcome

Optionally implement managed network acquisition of an authorized engine artifact: resolve a policy-compatible release, download through approved HTTP/TLS/proxy infrastructure, verify integrity/signature before exposure, safely extract/install under EPR1, and publish an immutable acquisition receipt. The block remains closed unless `maintainer_managed_engine_acquisition` is present.

The current owner is **a consume-only cache reader, no HTTP/TLS writer, blocked download-tier prose, and implicit npm registry assumptions**. The final and sole owner is **one policy-gated EngineAcquirer and verified atomic installer with no hidden network behavior and exact origin/integrity receipts**.

## Architectural role and end state

EPR2 is deliberately optional because it adds network and executable supply-chain dependencies. It owns acquisition only; candidate ranking belongs to EPR4 and activation/execution belongs to EPR5.

## Expected production surfaces

- `crates/verter_tsgo_api::toolchain::acquire` or a narrower dedicated acquisition crate
- approved HTTP/TLS/archive dependencies and dependency-policy records
- managed cache/install root from EPR1
- registry/release metadata adapters
- audit/security/proxy/offline test harnesses

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.

## Named APIs and data boundaries

- `EngineAcquisitionRequest`, `EngineReleaseRequirement`, and `EngineAcquisitionPlan`
- `EngineReleaseIndex`, `EngineReleaseDescriptor`, and exact origin metadata
- `EngineDownload`, `EngineDownloadProgress`, and cancellation/deadline controls
- `VerifiedArchive`, `SafeExtractionPlan`, and `EngineAcquisitionReceipt`
- `AcquisitionFailure::{Forbidden, Offline, Proxy, TLS, Origin, Integrity, Archive, Install, Cancelled}`

## Exact predecessor contracts

- **EPR1:** consume exact artifact/origin/integrity/cache/install/validation contract.
- **G5:** consume bounded I/O/CPU execution pools, cancellation, and owner-affine commands.

External custody: maintainer_managed_engine_acquisition. Dispatch fails until the canonical authorization receipt exists.

## Binding architecture

- No acquisition code is reachable unless the captured EPR0 policy and external authorization allow managed download.
- Release metadata and artifacts come only from declared authorized origins; redirects are bounded and revalidated.
- TLS verification is never disabled; custom enterprise roots/proxy configuration are explicit runtime inputs.
- Integrity/signature is verified on downloaded bytes before extraction and again against installed executable evidence as required.
- Archive extraction is path-safe, bounded, no-follow, and restricted to the temporary private root.
- Installation follows EPR1 temp/lock/atomic rename/READY-last semantics.
- Acquisition receipt does not imply selection or activation; EPR4/EPR5 revalidate required facts.
- No telemetry, registry ping, or auto-update occurs outside an explicit acquisition request.

## Internal subblocks

### EPR2-SB1 - Dependency, threat, and policy review

**Independently testable outcome:** Network/TLS/archive dependencies and threat boundaries are explicitly accepted before code lands.

**Architecture:**

- Inventory dependency trees, platform support, proxy/custom CA behavior, archive formats, memory/CPU risks.
- Document SSRF/redirect/path traversal/decompression bomb/race/tamper threats.
- Bind approved origins/trust roots and dependency versions.

**Expected changes:**

- Add dependency policy amendment and security review receipt.
- Fail dispatch if authorization/review is missing.

**Discriminating proof:**

- Static dependency/license/advisory review passes.
- Negative configuration cannot disable TLS or authorize arbitrary origin.

### EPR2-SB2 - Release metadata and version resolution

**Independently testable outcome:** The acquirer selects a stable compatible release descriptor without executing or trusting unverified package code.

**Architecture:**

- Query approved metadata endpoint with bounded response/time/redirects.
- Parse exact version/platform/artifact URL/integrity/signature metadata.
- Apply EPR0 update/channel policy and EPR1 compatibility before download.

**Expected changes:**

- Implement source adapter and hermetic metadata fixtures.
- Cache metadata only under exact origin/policy/expiry rules.

**Discriminating proof:**

- Malicious/oversized/malformed/redirected metadata fails closed.
- Version ordering/prerelease/platform mutation matrix selects exactly.

### EPR2-SB3 - Private cancellable download

**Independently testable outcome:** Artifact bytes are written only to a private temporary file with bounded size/time and no partial cache visibility.

**Architecture:**

- Use bounded streaming, content-length/actual-size limits, cancellation/deadline, fsync policy.
- Never execute/source/import downloaded content.
- Handle proxy/auth without logging credentials.

**Expected changes:**

- Implement I/O-pool command and progress/status events.
- Clean temp files on failure/cancel.

**Discriminating proof:**

- Cancel/timeout/disk-full/network-drop leaves no candidate/READY entry.
- Secret-redaction and zero-network-when-forbidden tests pass.

### EPR2-SB4 - Integrity, signature, and origin verification

**Independently testable outcome:** Downloaded bytes are cryptographically tied to authorized metadata/trust before extraction.

**Architecture:**

- Verify registry integrity/checksum and signature/attestation when policy requires.
- Bind redirect final origin and metadata receipt.
- Reject missing/weak/downgraded evidence.

**Expected changes:**

- Produce EPR1 integrity/origin evidence.
- Quarantine/delete only managed temporary data on failure.

**Discriminating proof:**

- One-byte/signature/trust-root/origin substitution mutations fail before extraction.
- Failure remains Integrity/Trust, not NotFound fallback.

### EPR2-SB5 - Safe extraction and atomic installation

**Independently testable outcome:** Only expected platform artifact files reach a private immutable managed entry.

**Architecture:**

- Reject absolute/parent/symlink/hardlink/reparse/device entries.
- Bound file count, path length, uncompressed size, and permissions.
- Validate executable/manifest then atomic rename and READY-last under lock.

**Expected changes:**

- Implement safe extractor/installer over EPR1 APIs.
- Loser installers verify winner or discard temp.

**Discriminating proof:**

- Traversal/bomb/symlink/crash/concurrent install matrix never escapes root or exposes partial entry.
- Installed descriptor/digest equals acquisition receipt.

### EPR2-SB6 - Enterprise proxy/offline/update behavior

**Independently testable outcome:** Managed acquisition behaves predictably under proxy/custom CA/offline/air-gap/update/rollback policy.

**Architecture:**

- Support explicit proxy/no-proxy/custom root inputs without persisting secrets.
- Honor offline/deny-network immediately.
- Separate explicit install/update requests; no background update.

**Expected changes:**

- Add hermetic proxy/TLS simulators and configuration integration.
- Expose stable remediation status.

**Discriminating proof:**

- Offline makes zero socket/DNS attempts.
- Proxy/auth failures are typed and do not fall back to unapproved origins.

### EPR2-SB7 - Acquisition observability and zero-work proof

**Independently testable outcome:** Acquisition work is explicit, auditable, bounded, and absent from normal resolution unless requested.

**Architecture:**

- Count metadata requests/download bytes/verification/extraction/files/install attempts/cleanup.
- Emit audit events without secrets.
- Prove EPR4 warm resolution never triggers network.

**Expected changes:**

- Add PER0/security receipts and long-cancel cleanup tests.
- Expose acquisition command/status through approved application surface only.

**Discriminating proof:**

- Policy-forbidden/unopened channel performs zero network/filesystem install work.
- Repeated explicit acquisition reuses valid immutable entry or performs exact declared update check.

## Data, identity, invalidation, and publication laws

- Engine acquisition, resolution, selection, and activation are distinct authorities with distinct receipts; no stage infers success from a later stage.
- No executable is run before exact origin, compatibility, integrity, and trust validation succeeds.
- Unavailable, unauthorized, incompatible, corrupt, offline, cancelled, and operationally failed outcomes remain distinct and capability-visible.
- All caches/installations are content/version/platform scoped, race-safe, and fail closed on symlink/reparse/permission/ownership violations.
- Acquisition requests are explicit side effects and never run inside a semantic/query hot path.
- Downloaded metadata/artifacts are untrusted until EPR1 validation receipts exist.
- Managed acquisition may mutate only its private managed root.

## Migration and cutover

- Create module/dependencies only after external authorization.
- Implement against hermetic local test server before any real registry canary.
- Keep existing manual/project/system/editor sources unchanged; add managed source only to EPR4 after acceptance.

## Deletions

- Delete blocked consume-only download-tier prose after this charter/source atoms land.
- Delete any ad hoc shell/npm/npx download execution path.
- Delete temporary insecure dependency/config experiments.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

## Forbidden designs

- Background/implicit download or update.
- TLS verification disable, arbitrary URL, unbounded redirect/response/archive.
- Executing package scripts or downloaded code during install.
- Extracting before integrity verification or outside private temp root.
- Treating integrity/trust failure as a reason to try another origin silently.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

## Acceptance IDs and discriminating proof

- **EPR2-AC-AUTHORIZED:** channel is unreachable without exact policy/external authorization.
- **EPR2-AC-ZERO-HIDDEN-NET:** normal resolution and forbidden/offline policies make zero network attempts.
- **EPR2-AC-VERIFY-FIRST:** byte/origin/signature mutations fail before extraction/execution.
- **EPR2-AC-SAFE-EXTRACT:** traversal/bomb/link/crash/concurrency matrix is contained and atomic.
- **EPR2-AC-ENTERPRISE:** proxy/custom CA/offline/secret-redaction behavior is exact.
- **EPR2-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **EPR2-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **EPR2-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **EPR2-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Performance and bounded work

- Metadata/download/extraction have explicit byte/file/time/CPU limits; normal warm resolution performs no EPR2 work.
- Target ceiling: 800 production LOC, 8 production files, and 2 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Mandatory rescope and abort conditions

- Abort if dependency/security review is not accepted.
- Abort if a platform/archive cannot satisfy safe no-follow/atomic install semantics.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. Hermetic HTTP/TLS/proxy/redirect/metadata/download failure matrix.
1. Integrity/signature/origin and safe archive/extraction/crash/concurrency tests.
1. Policy/authorization/no-network/secret-redaction/cleanup/equivalent-work receipts.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Consumers and unlocks

- When opened, supplies managed verified candidates to EPR4.
- Does not by itself advertise engine availability or activate a provider.
- Can remain permanently unopened under a no-download policy.

## Source reconciliation

- `docs/arch/future/engine-provisioning-download-tier.md`.
- EPR0/EPR1 policy and cache/install contracts.

Durable clauses are transferred as digest-bound requirement atoms. Historical path archaeology, obsolete branches, and implementation journals are not copied into the charter. Git history remains the archive.


---

<!-- unified-charter-v2
id=EPR3
name=Bundled sidecar shipping and distribution channel
phase=expansion
train=expansion.engine-provisioning
product=engine_provisioning
kind=implementation
semantic_role=delivery
class=successor
predecessors=EPR1
conditional_predecessors=
owner=expansion.engine-provisioning:explicit policy-controlled engine acquisition, resolution, and activation authority
conflict_domains=provider_lifecycle,program_authority,source_lineage
resource_class=rust-mixed
review_profile=security-3
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
optional=true
release_gating=non_release
source_refs=live:docs/arch/refactor/rev11/sources/legacy-arch-reconciliation.md
external_requirements=maintainer_bundled_engine_shipping
activation_gate=ORC0
charter=charters/expansion-engine-provisioning/EPR3.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# EPR3 - Bundled sidecar shipping and distribution channel

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

The internal subblocks below are binding decomposition and review checkpoints. They do not receive independent dispatch, leases, receipts, or deletion ownership unless the pre-scope architect proves that one subblock is independently acceptable or the block crosses a mandatory rescope trigger. In that case, amend the DAG before production mutation rather than treating a train-sized subblock as an implementation checklist.

## Independently acceptable outcome

Optionally implement a bundled engine sidecar distribution channel owned by explicit release artifacts. Build/release stages acquire a pinned verified engine, stage it into authorized platform packages, emit manifest/SBOM/license/provenance evidence, validate installed layout end to end, and make the immutable bundled candidate visible to EPR4. The block remains closed unless `maintainer_bundled_engine_shipping` is present.

The current owner is **a complete runtime bundle-location/manifest reader, release packages that ship the server, and build guards explicitly forbidding tsgo-shaped artifacts**. The final and sole owner is **one authorized per-platform bundled engine artifact family with exact release provenance, installed validation, size/update/rollback policy, and no unauthorized package inclusion**.

## Architectural role and end state

EPR3 resolves the contradiction between “tier 4 exists” and “tsgo is never packaged” only through an explicit product/release decision. It owns shipping, not runtime selection or activation.

## Expected production surfaces

- release workflow/build matrices and platform package staging scripts
- `packages/verter-lsp` or another explicitly named artifact family
- VSIX/editor packages only if separately authorized by EPR0 policy
- `crates/verter_tsgo_api::toolchain::bundle` validation contract
- SBOM/license/provenance manifests and installed-package E2E tests

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.

## Named APIs and data boundaries

- `BundledEngineReleaseSpec`, `BundledEngineInputReceipt`, and platform target matrix
- `BundledEngineManifest`, file digest table, and install-relative layout
- `BundledEngineProvenance`, `BundledEngineSbomRef`, and license notice set
- `BundledPackageReceipt` and `InstalledBundleValidationReceipt`
- `BundleShippingPolicy` and authorized package IDs

## Exact predecessor contracts

- **EPR1:** consume exact artifact/platform/origin/integrity/install/validation contract.

External custody: maintainer_bundled_engine_shipping. Dispatch fails until the canonical authorization receipt exists.

## Binding architecture

- Bundling is authorized per package family/platform/version; a general “bundling enabled” flag is insufficient.
- Release inputs are pinned and verified independently; build machines do not fetch latest unpinned artifacts.
- The shipped bytes and manifest are validated after packaging/extraction, not only before staging.
- Package whitelists/guards are amended deliberately to allow only exact authorized paths/digests/layouts.
- SBOM, license, provenance, size, update cadence, rollback, and security response are release acceptance inputs.
- Runtime sees the bundled candidate through the same EPR1 descriptor/validation as other sources.
- Unavailable platform rows remain explicit; no package claims a bundle that it does not contain.
- Bundled presence never overrides EPR0 source policy or EPR4 selection rules implicitly.

## Internal subblocks

### EPR3-SB1 - Shipping owner and artifact-family decision

**Independently testable outcome:** One exact release artifact family owns the bundle, and all other packages reject it.

**Architecture:**

- Select lsp platform package, dedicated engine package, VSIX, or another explicit channel.
- Define consumers, install-relative layout, duplication policy, and platform coverage.
- Reconcile existing whitelist/never-package guards.

**Expected changes:**

- Amend release authority and package guard tests.
- Fail dispatch without owner authorization.

**Discriminating proof:**

- Bundle in unauthorized package/path fails.
- Authorized package absence/presence matches platform matrix exactly.

### EPR3-SB2 - Pinned verified release input

**Independently testable outcome:** The release build consumes an exact engine version/platform artifact with independent origin/integrity receipt.

**Architecture:**

- Pin source version/digest/signature/provenance.
- Disallow latest/unversioned/nightly unless policy explicitly names it.
- Separate release-input acquisition from runtime managed acquisition.

**Expected changes:**

- Implement deterministic staging input process and cached release receipt.
- Reuse EPR1 validation before staging.

**Discriminating proof:**

- Input substitution/version drift fails reproducible build gate.
- No package scripts/unverified code execute during staging.

### EPR3-SB3 - Platform staging and package manifest

**Independently testable outcome:** Each package contains only expected engine files at the exact runtime-relative layout.

**Architecture:**

- Stage executable/support files and bundle integrity manifest.
- Normalize permissions/executable bit/platform naming.
- Keep server and engine identities distinct.

**Expected changes:**

- Update per-platform build matrix and package manifests.
- Retain strict whitelist for every unrelated entry.

**Discriminating proof:**

- Installed archive listing equals declared manifest.
- Wrong platform/name/permission/layout fails package tests.

### EPR3-SB4 - SBOM, license, provenance, and security response

**Independently testable outcome:** The shipped engine is legally and operationally traceable and revocable.

**Architecture:**

- Generate/include SBOM and license notices.
- Record upstream source/build/release provenance and security contact/update SLA.
- Define revocation/withdrawal/emergency release process.

**Expected changes:**

- Integrate artifact attestations and release metadata.
- Bind records into package receipt.

**Discriminating proof:**

- Missing/stale notices/provenance block release.
- Revoked input cannot be promoted or selected.

### EPR3-SB5 - Installed-package end-to-end validation

**Independently testable outcome:** The actual consumer-installed package exposes a candidate that passes EPR1 and can handshake under EPR5 canaries.

**Architecture:**

- Install/extract each platform package in a clean sandbox.
- Locate via runtime relative path and validate manifest/digests/compatibility.
- Run bounded version/protocol handshake without project semantics.

**Expected changes:**

- Add CI per-platform package E2E.
- Test VSIX/editor packaging separately where authorized.

**Discriminating proof:**

- Pre-stage success cannot mask post-package corruption/omission.
- Installed layout matches runtime discovery exactly.

### EPR3-SB6 - Size, update, rollback, and channel coexistence

**Independently testable outcome:** Bundle cost and lifecycle are explicit and do not create duplicate/conflicting engine copies accidentally.

**Architecture:**

- Measure compressed/uncompressed/install size and platform package impact.
- Define update synchronization with server/plugin compatibility.
- Retain/restore prior known-good package release via package manager, not mutable in-place bundle.

**Expected changes:**

- Add release thresholds and compatibility matrix.
- Define source precedence with project/editor/system/managed candidates in EPR4.

**Discriminating proof:**

- Size/compatibility thresholds and rollback canary pass.
- Duplicate package engines have explicit identities and selection law.

### EPR3-SB7 - Unauthorized absence and zero-work proof

**Independently testable outcome:** When EPR3 is unopened or policy forbids bundle use, no build/runtime bundle path is silently active.

**Architecture:**

- Keep guards rejecting bundle-shaped artifacts in unauthorized channels.
- Generate opened/unopened release matrix.
- Prove runtime does not scan/hash bundle paths when source class disabled.

**Expected changes:**

- Add negative package/runtime tests.
- Remove stale tier claims when permanently unopened.

**Discriminating proof:**

- Unauthorized package injection fails.
- Disabled bundle source performs zero bundle filesystem work.

## Data, identity, invalidation, and publication laws

- Engine acquisition, resolution, selection, and activation are distinct authorities with distinct receipts; no stage infers success from a later stage.
- No executable is run before exact origin, compatibility, integrity, and trust validation succeeds.
- Unavailable, unauthorized, incompatible, corrupt, offline, cancelled, and operationally failed outcomes remain distinct and capability-visible.
- All caches/installations are content/version/platform scoped, race-safe, and fail closed on symlink/reparse/permission/ownership violations.
- Bundled artifacts are immutable release contents; runtime never updates them in place.
- A package receipt binds exact staged and final packaged bytes.
- Package source/provenance does not grant runtime selection priority outside EPR4 policy.

## Migration and cutover

- Decide and authorize package owner before weakening any existing guard.
- Stage one canary platform, validate package/install/runtime layout, then expand platform matrix.
- Expose source to EPR4 only after all declared platform rows pass.

## Deletions

- Delete blocked bundled-sidecar prose after explicit decision/authority transfer.
- Delete obsolete “never packaged” guards only for exact authorized channels; keep/rewrite negative guards for all others.
- Delete ad hoc manual bundle staging scripts.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

## Forbidden designs

- Quietly relaxing package whitelists.
- Fetching latest/unverified engine during release.
- Shipping without SBOM/license/provenance or installed-package validation.
- Runtime mutable update of bundled bytes.
- Claiming offline floor on platforms/packages without bundle rows.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

## Acceptance IDs and discriminating proof

- **EPR3-AC-OWNER:** exact authorized package/platform matrix and negative rejection elsewhere.
- **EPR3-AC-REPRO:** pinned input and final package bytes/manifests are reproducible and verified.
- **EPR3-AC-INSTALLED:** clean installed package passes runtime discovery/EPR1 validation/handshake.
- **EPR3-AC-SUPPLY:** SBOM/license/provenance/revocation evidence is complete.
- **EPR3-AC-COST:** size/update/compatibility/rollback thresholds are accepted.
- **EPR3-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **EPR3-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **EPR3-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **EPR3-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Performance and bounded work

- Runtime bundle discovery is disabled-zero-work or bounded relative-path validation; no recursive package scanning.
- Target ceiling: 800 production LOC, 8 production files, and 2 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Mandatory rescope and abort conditions

- Abort if no release artifact owner accepts size/license/update/security obligations.
- Abort if supported platform packaging cannot reproduce the exact validated runtime layout.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. Release input reproducibility/integrity/provenance tests.
1. Per-platform package listing/permission/layout/install/runtime validation.
1. Unauthorized package injection, disabled zero-work, size, compatibility, rollback, and revocation tests.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Consumers and unlocks

- When opened, supplies bundled verified candidates to EPR4.
- May provide offline floor under explicit EPR0 policy.
- Can remain permanently unopened with tier removed from public policy.

## Source reconciliation

- `docs/arch/future/engine-provisioning-bundled-sidecar-and-shipping-channel.md`.
- Current package/VSIX build guards and bundle runtime contract.

Durable clauses are transferred as digest-bound requirement atoms. Historical path archaeology, obsolete branches, and implementation journals are not copied into the charter. Git history remains the archive.


---

<!-- unified-charter-v2
id=EPR4
name=Exact authorized engine candidate resolution and selection
phase=expansion
train=expansion.engine-provisioning
product=engine_provisioning
kind=implementation
semantic_role=delivery
class=successor
predecessors=EPR1,H2
conditional_predecessors=EPR2:when-opened,EPR3:when-opened
owner=expansion.engine-provisioning:explicit policy-controlled engine acquisition, resolution, and activation authority
conflict_domains=provider_lifecycle,performance_evidence,source_lineage
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
charter=charters/expansion-engine-provisioning/EPR4.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# EPR4 - Exact authorized engine candidate resolution and selection

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

The internal subblocks below are binding decomposition and review checkpoints. They do not receive independent dispatch, leases, receipts, or deletion ownership unless the pre-scope architect proves that one subblock is independently acceptable or the block crosses a mandatory rescope trigger. In that case, amend the DAG before production mutation rather than treating a train-sized subblock as an implementation checklist.

## Independently acceptable outcome

Implement the exact authorized engine candidate resolver and deterministic selection plan. It enumerates only EPR0-authorized source adapters, converts every found locator into an EPR1-validated descriptor, records every rejection, ranks compatible candidates by explicit policy, and returns one selection or a typed no-selection report. It does not spawn or activate the engine.

The current owner is **tier-ordered path enumeration, source-specific validation/fallback, mixed discovery and spawn logic, cache scans, and incomplete status reporting**. The final and sole owner is **one EngineResolver with authorized source adapters, normalized validated candidates, deterministic comparator, complete rejection evidence, and warm bounded/zero-network behavior**.

## Architectural role and end state

EPR4 separates “which executable should be used” from acquisition and activation. It keeps source policy, validation, selection, and operational health honest and independently testable.

## Expected production surfaces

- `crates/verter_tsgo_api::toolchain` source adapters and resolution coordinator
- `crates/verter_type_runtime`/session ProviderHub request boundary
- `crates/verter_identity` resolver/candidate/plan identities
- `crates/verter_protocol` status/selection report projections
- performance/audit counters and hermetic filesystem fixtures

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.

## Named APIs and data boundaries

- `EngineResolutionRequest`, `EngineRequirement`, and `EngineResolutionBasis`
- `EngineSourceAdapter` closed registration/capability contract
- `EngineCandidate`, `ValidatedEngineCandidate`, and `EngineCandidateRejection`
- `EngineSelectionPolicy`, `EngineCandidateComparator`, and `EngineSelectionPlan`
- `EngineResolutionReport { selected, considered, rejected, outcome, basis }`
- `EngineResolverSnapshot`, `EngineResolutionEpoch`, and complete-only cache admission

## Exact predecessor contracts

- **EPR1:** consume exact candidate artifact/compatibility/origin/integrity/cache validation.
- **H2:** consume project-scoped ProviderHub requirements and provider binding lifecycle.
- **EPR2:when-opened:** when opened, enumerate only already installed verified managed entries; resolution itself performs no network.
- **EPR3:when-opened:** when opened, enumerate exact authorized bundled relative-path candidates; when unopened prove zero bundle work.

External custody: none beyond the package activation boundary.

## Binding architecture

- Resolution enumerates only source classes authorized by the captured EPR0 policy for the request context.
- Source adapters discover locators and source evidence; EPR1 validates and normalizes before comparison.
- Every candidate rejection is retained with typed reason; integrity/trust/revocation failure is not silently downgraded to absence.
- Selection comparator is explicit, deterministic, versioned, and independent of filesystem enumeration order.
- No source adapter spawns, executes, downloads, updates, or mutates a project/system/editor artifact.
- Managed/download and bundled channels are optional inputs only when opened and authorized; normal resolution never triggers EPR2 network work.
- Warm resolution uses exact source/policy/filesystem/provider requirement facts and performs zero repeated broad scans/hashes/network.
- A no-selection outcome distinguishes forbidden, offline, not-found, rejected, incompatible, and cancelled states and provides exact remediation.

## Internal subblocks

### EPR4-SB1 - Resolution request and requirement model

**Independently testable outcome:** Every resolution names exact project/profile/engine flavor/features/platform/policy and cancellation/budget basis.

**Architecture:**

- Define requirement axes and request identity.
- Separate mandatory/optional engine needs and feature capability requirements.
- Bind captured policy/provider/project epochs.

**Expected changes:**

- Add resolver request and result schemas.
- Replace global/default discovery calls with project-scoped requests.

**Discriminating proof:**

- Different project/profile/feature/policy requirements never warm-hit each other.
- Missing ambiguous project context yields NeedInputs.

### EPR4-SB2 - Authorized source adapter registry

**Independently testable outcome:** Only declared source adapters execute, in policy-defined selection groups with exact zero-work for disabled sources.

**Architecture:**

- Define adapters for explicit override/manual, project-local, editor-shared, system/PATH, managed cache, and bundle.
- Expose bounded enumeration and source snapshot/read sets.
- Keep adapter output as locators/evidence, not trusted candidates.

**Expected changes:**

- Centralize registration and remove hard-coded tier chain.
- Generate source/capability matrix from EPR0.

**Discriminating proof:**

- Disabled/unopened adapters are never called.
- Planting an unregistered adapter or hidden source path fails.

### EPR4-SB3 - Candidate discovery and validation composition

**Independently testable outcome:** Every discovered locator is normalized and validated under EPR1 before selection.

**Architecture:**

- Canonicalize paths/layout without following forbidden links.
- Capture mutable source stat/manifest/read-set basis.
- Run compatibility/integrity/origin/revocation validation and retain rejection.

**Expected changes:**

- Implement concurrent/bounded validation under scheduler pools where beneficial.
- Delete source-specific trust shortcuts.

**Discriminating proof:**

- Malicious/invalid first candidate cannot suppress reporting or become not-found fallback.
- Validation order permutations produce same candidate/rejection set.

### EPR4-SB4 - Deterministic selection comparator

**Independently testable outcome:** One exact comparator selects the best compatible authorized candidate without hidden source or latest bias.

**Architecture:**

- Define source preference, explicit override law, project pin/locality, editor sharing, version stability, compatibility, policy update channel, and tie-breakers.
- Version comparator and emit explanation trace.
- Preserve explicit override failures rather than silently ignoring them when policy says strict.

**Expected changes:**

- Implement pure comparator and property/mutation matrix.
- Remove first-found/tier-return behavior.

**Discriminating proof:**

- Enumeration/order permutation yields same selection.
- Every comparator dimension has discriminating positive/negative fixture.

### EPR4-SB5 - Resolution report and truthful remediation

**Independently testable outcome:** Selected and rejected candidates are fully explainable and no-selection outcomes are actionable without leaking secrets.

**Architecture:**

- Return selected descriptor/receipt ref and ordered considered/rejected summaries.
- Distinguish Forbidden, Offline, NeedInputs, NotFound, Incompatible, Trust/Integrity failure, Revoked, Cancelled.
- Expose safe public status versus private audit detail.

**Expected changes:**

- Add PUB0/status adapters and logs.
- Delete vague “no provider” fallback reports.

**Discriminating proof:**

- Outcome/status mutation tests preserve exact reason/remediation.
- Explicit corrupt override is reported loudly according to policy.

### EPR4-SB6 - Resolution cache, invalidation, and zero-work

**Independently testable outcome:** Repeated resolution reuses exact validated source snapshots while changes invalidate only affected adapters/candidates.

**Architecture:**

- Cache by full request/policy/source snapshot/revocation/compatibility basis.
- Use source-specific watchers/epochs or bounded stat facts.
- Never cache incomplete/budget/cancelled negative as complete.

**Expected changes:**

- Implement project-scoped resolver snapshots/singleflight and counters.
- Release caches on project/policy/provider teardown.

**Discriminating proof:**

- Warm unchanged request performs zero network, zero broad scan, and ratified minimal stat/hash work.
- Adding/removing/replacing candidates or policy/revocation change invalidates exactly.

### EPR4-SB7 - Resolver security and adversarial filesystem proof

**Independently testable outcome:** Resolution remains safe under symlink/reparse/permission/path races and hostile project/cache layouts.

**Architecture:**

- Use no-follow/open-by-handle patterns where required by EPR1.
- Bound directory/file enumeration and path lengths.
- Detect TOCTOU between validation receipt and selection handoff.

**Expected changes:**

- Add adversarial cross-platform fixture harness.
- Pass immutable descriptor/validation receipt to EPR5, not only path.

**Discriminating proof:**

- Path substitution/race/symlink/permission mutations cannot produce a selected executable.
- Large hostile trees remain bounded and cancellable.

## Data, identity, invalidation, and publication laws

- Engine acquisition, resolution, selection, and activation are distinct authorities with distinct receipts; no stage infers success from a later stage.
- No executable is run before exact origin, compatibility, integrity, and trust validation succeeds.
- Unavailable, unauthorized, incompatible, corrupt, offline, cancelled, and operationally failed outcomes remain distinct and capability-visible.
- All caches/installations are content/version/platform scoped, race-safe, and fail closed on symlink/reparse/permission/ownership violations.
- Selection plan is pure data and does not imply process health or activation.
- An explicit strict override rejection may block fallback according to policy; fallback behavior is never hard-coded in adapters.
- Resolution cache admission requires complete enumeration of every policy-applicable source group needed for the decision.

## Migration and cutover

- Wrap current source discovery as adapters under the new resolver in current policy order.
- Characterize selections/rejections and make intentional trust/correctness changes explicit.
- Add optional managed/bundle adapters only when opened; delete tier chain after parity.

## Deletions

- Delete hard-coded numeric tier/first-return discovery and mixed discovery-spawn logic.
- Delete source-specific silent fallback after integrity/trust failure.
- Delete broad recursive cache/project scans and process-global selection caches.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

## Forbidden designs

- Download/update/spawn inside resolution.
- First-found or filesystem-order selection.
- Path/version-only unvalidated candidates.
- Collapsing rejected candidate to not-found.
- Warm cache without exact source/policy/revocation basis.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

## Acceptance IDs and discriminating proof

- **EPR4-AC-SOURCES:** only authorized/opened adapters execute and disabled sources prove zero work.
- **EPR4-AC-CANDIDATES:** every selected candidate has current EPR1 validation and every rejection remains typed.
- **EPR4-AC-COMPARATOR:** enumeration permutations and dimension mutation matrix yield exact deterministic selection.
- **EPR4-AC-NO-NETWORK:** resolution never performs network acquisition/update.
- **EPR4-AC-CACHE:** incremental/warm resolution equals fresh and invalidates exactly under source/policy/revocation changes.
- **EPR4-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **EPR4-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **EPR4-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **EPR4-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Performance and bounded work

- Resolution work is bounded per authorized adapter; warm unchanged resolution performs zero broad scans/hashes/network and plateaus in memory.
- Target ceiling: 800 production LOC, 8 production files, and 2 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Mandatory rescope and abort conditions

- Abort if a source adapter cannot provide complete bounded enumeration/read-set facts required for a selection claim.
- Abort if path substitution between validation and activation cannot be prevented/detected.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. Source authorization/zero-work and adapter registry guards.
1. Comparator permutation/mutation and complete rejection/outcome matrix.
1. Adversarial filesystem/TOCTOU/symlink/reparse/permission/cancel/cache/invalidation/performance tests.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Consumers and unlocks

- Supplies exact selection plans/validated artifacts to EPR5.
- Feeds public engine status and CLI diagnostics without spawning.
- Makes optional acquisition/bundle sources composable without changing activation.

## Source reconciliation

- Current toolchain discovery policy/code and EPR0-EPR3 contracts.
- ProviderHub/TCM engine requirement identities.

Durable clauses are transferred as digest-bound requirement atoms. Historical path archaeology, obsolete branches, and implementation journals are not copied into the charter. Git history remains the archive.


---

<!-- unified-charter-v2
id=EPR5
name=Engine activation epochs, health, and truthful capability publication
phase=expansion
train=expansion.engine-provisioning
product=engine_provisioning
kind=convergence
semantic_role=delivery
class=successor
predecessors=EPR4,H3,PUB0,COX0
conditional_predecessors=
owner=expansion.engine-provisioning:explicit policy-controlled engine acquisition, resolution, and activation authority
conflict_domains=provider_lifecycle,lsp_publication,public_protocol,capability_catalog
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
charter=charters/expansion-engine-provisioning/EPR5.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# EPR5 - Engine activation epochs, health, and truthful capability publication

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

The internal subblocks below are binding decomposition and review checkpoints. They do not receive independent dispatch, leases, receipts, or deletion ownership unless the pre-scope architect proves that one subblock is independently acceptable or the block crosses a mandatory rescope trigger. In that case, amend the DAG before production mutation rather than treating a train-sized subblock as an implementation checklist.

## Independently acceptable outcome

Implement engine activation as a separate atomic lifecycle: revalidate the selected artifact handoff, spawn under bounded owner-affine control, perform version/protocol/capability handshake and health checks, bind a new ProviderEpoch into ProviderHub atomically, publish truthful capabilities only after success, and handle swap/restart/crash/rollback without stale mixed service.

The current owner is **source-specific spawn helpers, provider constructors, LSP initialize capability assumptions, partial shared/editor attach logic, and mixed discovery/activation failures**. The final and sole owner is **one EngineActivator and project-scoped activation state machine with exact receipts, deadlines, health, ProviderEpoch, atomic ProviderHub binding, and capability publication**.

## Architectural role and end state

EPR5 ensures that a selected executable is not treated as available until it has successfully handshaken and become the exact active project binding. It owns operational lifecycle, not artifact selection or semantic provider implementation.

## Expected production surfaces

- `crates/verter_session` ProviderHub/project service graph
- `crates/verter_type_runtime` provider process/transport adapters
- `crates/verter_tsgo_api` actor/process lifecycle where applicable
- `crates/verter_lsp` capability/status publication through shared host
- `crates/verter_protocol` activation/status/receipt schemas

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.

## Named APIs and data boundaries

- `EngineActivationRequest`, `EngineActivationBasis`, and `EngineActivationPlan`
- `EngineActivator`, `EngineProcessHandle`, and owner-affine lifecycle commands
- `EngineHandshake`, `EngineCapabilitySet`, and `EngineHealthState`
- `EngineActivationReceipt`, `ProviderEpoch`, and `ProviderBinding`
- `EngineSwapPlan`, `EngineRollbackPlan`, and `EngineDeactivationReceipt`
- `EngineActivationFailure::{StaleSelection, Spawn, Handshake, Protocol, Capability, Health, Timeout, Crash, Cancelled}`

## Exact predecessor contracts

- **EPR4:** consume deterministic selection plan and exact validated artifact receipt.
- **H3:** consume stale-safe publication/supersession semantics.
- **PUB0:** consume typed public outcomes/capability truth.
- **COX0:** consume dynamic profile capability registration/withdrawal and zero-work modes.

External custody: none beyond the package activation boundary.

## Binding architecture

- Activation revalidates the exact artifact/path facts needed to close TOCTOU before spawn.
- Spawn occurs under bounded process/resource/deadline/cancellation policy and uses owner-affine lifecycle commands.
- Availability requires successful protocol/version/capability handshake; process existence alone is insufficient.
- ProviderHub binding and ProviderEpoch publication are atomic; requests never see a half-initialized or mixed old/new provider.
- Capabilities are derived from the active handshake plus profile/coexistence policy and published only after binding success.
- Swap/restart/rollback keeps old binding until new one is healthy, or withdraws truthfully when no valid binding remains.
- Crash/hang/deadline failures cancel affected requests, invalidate epoch-bound handles, and never reuse stale results.
- Shared/editor-attached topology follows the same applied-snapshot/epoch/health receipt law as child processes.

## Internal subblocks

### EPR5-SB1 - Activation request and stale-selection revalidation

**Independently testable outcome:** Activation binds exact selected artifact, policy, project/profile, and current source/provider requirement basis.

**Architecture:**

- Define request/plan identity and revalidate EPR1/EPR4 receipt/path facts.
- Reject stale selection/policy/revocation/project requirement changes.
- Separate activate, attach, swap, restart, deactivate.

**Expected changes:**

- Add activation coordinator input and TOCTOU checks.
- Remove path-only spawn entry points.

**Discriminating proof:**

- Replacing/revoking artifact between selection/spawn fails before execution.
- Same exact plan is deterministic and singleflight.

### EPR5-SB2 - Bounded process/transport startup

**Independently testable outcome:** Engine startup is cancellable/deadline-bounded, owner-affine, and leaks no orphan process/transport.

**Architecture:**

- Spawn with exact executable/args/env/workdir/sandbox policy.
- Bound stdout/stderr/message sizes and startup resources.
- Support child and approved shared/editor transport adapters.

**Expected changes:**

- Centralize startup/cleanup under ProviderHub lifecycle.
- Remove source-specific unmanaged spawn helpers.

**Discriminating proof:**

- Timeout/cancel/spawn failure leaves no active binding/orphan process.
- Argument/env/path injection and secret logging tests fail closed.

### EPR5-SB3 - Handshake, compatibility, and capability verification

**Independently testable outcome:** The running engine proves exact identity/protocol/features before it can serve requests.

**Architecture:**

- Query version/build/protocol/capabilities and compare with selected descriptor/requirements.
- Detect wrong binary/wrapper/protocol downgrade.
- Capture handshake evidence in activation receipt.

**Expected changes:**

- Implement provider-neutral handshake result and adapter mappings.
- Refuse capability lies/unknown required features.

**Discriminating proof:**

- Wrong-version/protocol/capability mutation kills candidate before binding.
- Handshake receipt matches selected artifact identity.

### EPR5-SB4 - Atomic ProviderHub binding and epoch publication

**Independently testable outcome:** A healthy engine becomes visible in one atomic project-scoped state transition.

**Architecture:**

- Create new ProviderEpoch and immutable binding after handshake.
- Swap binding pointer/service graph atomically and invalidate old epoch handles.
- Coordinate in-flight request settlement/cancellation.

**Expected changes:**

- Integrate with H2 ProviderHub and H3 publication.
- Delete global mutable provider/current-engine fields.

**Discriminating proof:**

- Failure injection yields old or new complete binding, never half state.
- Requests/results/resolve keys from old epoch fail closed after swap.

### EPR5-SB5 - Health, crash, hang, restart, and rollback

**Independently testable outcome:** Operational degradation is detected and handled without stale publication or retry storms.

**Architecture:**

- Define Starting/Healthy/Degraded/Failed/Stopping states and heartbeat/request-deadline signals.
- Bound restart/backoff under explicit policy; no infinite/sleep-poll correctness loop.
- Rollback to prior validated selection/binding only under policy.

**Expected changes:**

- Implement health supervisor/audit and deterministic transition table.
- Cancel affected flights and withdraw capabilities when unavailable.

**Discriminating proof:**

- Crash/hang/restart/rollback race matrix publishes no stale result.
- Repeated failure is bounded and capability status remains truthful.

### EPR5-SB6 - Truthful capability/status publication

**Independently testable outcome:** LSP/CLI/public surfaces advertise only capabilities actually available from active engine plus profile/coexistence policy.

**Architecture:**

- Compose handshake capabilities, certified native replacements, profile masks, and client participation.
- Register/unregister dynamically and clear owned stale diagnostics/results.
- Expose exact source/version/status/rejection/remediation safely.

**Expected changes:**

- Route capability generation through PUB0/COX0.
- Remove initialize-time assumptions based only on configured provider mode.

**Discriminating proof:**

- No active engine means provider capabilities false/NeedInputs, not dishonest true.
- Engine/native family transitions withdraw only displaced capabilities.

### EPR5-SB7 - Activation cache/work and lifecycle memory proof

**Independently testable outcome:** Repeated activation/healthy use avoids redundant validation/spawn while teardown fully releases resources.

**Architecture:**

- Singleflight same activation plan; reuse only healthy exact binding.
- Count spawn/handshake/restart/swap/requests/orphans/retained handles.
- Release process, transport, snapshots, resolve keys, caches on project close/policy change.

**Expected changes:**

- Add PER0 lifecycle receipts and soak tests.
- Ensure resolution does not rerun inside every request.

**Discriminating proof:**

- Warm healthy requests perform zero resolution/activation work.
- Long churn/project teardown leaves no orphan processes or retained growth.

## Data, identity, invalidation, and publication laws

- Engine acquisition, resolution, selection, and activation are distinct authorities with distinct receipts; no stage infers success from a later stage.
- No executable is run before exact origin, compatibility, integrity, and trust validation succeeds.
- Unavailable, unauthorized, incompatible, corrupt, offline, cancelled, and operationally failed outcomes remain distinct and capability-visible.
- All caches/installations are content/version/platform scoped, race-safe, and fail closed on symlink/reparse/permission/ownership violations.
- ProviderEpoch is the identity of an applied healthy binding, not a configured mode or discovered path.
- Activation receipts bind selected artifact/validation/handshake/policy/project/profile facts.
- Request/result/resolve caches and handles are epoch-scoped and invalid after swap/deactivation.

## Migration and cutover

- Wrap current provider startup behind activation state machine while keeping current source selections.
- Migrate one topology at a time: child process, project-local/system, editor-shared, managed/bundled when opened.
- Move capability publication after atomic binding and delete old constructors/flags.

## Deletions

- Delete mixed discovery-spawn helpers, global provider state, initialize-time capability guesses, and unbounded restart/poll loops.
- Delete epoch-less resolve/request handles and stale-result reuse.
- Delete source-specific activation semantics after adapter migration.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

## Forbidden designs

- Publishing capability before successful handshake/binding.
- Spawning from path without current validation/selection receipt.
- Half-swapped provider binding or mixing old/new epoch results.
- Infinite retry, sleep/poll readiness as correctness, orphan process/transport.
- Treating process alive as healthy/compatible.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

## Acceptance IDs and discriminating proof

- **EPR5-AC-REVALIDATE:** path/artifact/policy/revocation drift between selection/spawn is rejected.
- **EPR5-AC-HANDSHAKE:** wrong version/protocol/capability never binds.
- **EPR5-AC-ATOMIC:** swap/failure injection proves old-or-new ProviderEpoch only.
- **EPR5-AC-HEALTH:** crash/hang/restart/rollback is bounded and stale-safe.
- **EPR5-AC-CAPABILITY:** public capability/status exactly reflects active handshake plus native/profile/coexistence authority.
- **EPR5-AC-TEARDOWN:** no orphan processes/transports or retained epoch handles after churn/close.
- **EPR5-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **EPR5-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **EPR5-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **EPR5-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Performance and bounded work

- Healthy active requests perform zero resolution/activation work; lifecycle overhead is bounded to explicit transitions and monitored under PER0.
- Target ceiling: 800 production LOC, 8 production files, and 2 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Mandatory rescope and abort conditions

- Abort if a provider topology cannot expose exact handshake/epoch/applied-snapshot evidence.
- Abort if atomic swap/withdrawal cannot be guaranteed for a supported public surface.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. TOCTOU/revalidation/spawn/argument/environment/timeout/cancel tests.
1. Handshake/version/protocol/capability and atomic swap/epoch invalidation matrix.
1. Crash/hang/restart/rollback/capability/publication/project teardown/soak/performance tests.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Consumers and unlocks

- Unlocks EPR6 terminal conformance.
- Provides exact active engine binding/status to TypeScript observation, language service, CLI, and diagnostics.
- Supports native replacement by truthful capability composition rather than all-or-nothing provider shutdown.

## Source reconciliation

- H2/H3/TCM/provider lifecycle contracts and current engine spawn/attach behavior.
- Legacy editor/shared provider and provisioning documents.

Durable clauses are transferred as digest-bound requirement atoms. Historical path archaeology, obsolete branches, and implementation journals are not copied into the charter. Git history remains the archive.


---

<!-- unified-charter-v2
id=EPR6
name=Offline, enterprise, and supply-chain conformance terminal
phase=expansion
train=expansion.engine-provisioning
product=engine_provisioning
kind=terminal
semantic_role=delivery
class=successor
predecessors=EPR5,VIM1,PER0,BR0
conditional_predecessors=CLI4:when-opened
owner=expansion.engine-provisioning:explicit policy-controlled engine acquisition, resolution, and activation authority
conflict_domains=provider_lifecycle,performance_evidence,program_authority
resource_class=rust-mixed
review_profile=security-3
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
release_gating=product
source_refs=live:docs/arch/refactor/rev11/sources/legacy-arch-reconciliation.md
external_requirements=
activation_gate=ORC0
charter=charters/expansion-engine-provisioning/EPR6.md
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# EPR6 - Offline, enterprise, and supply-chain conformance terminal

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

The internal subblocks below are binding decomposition and review checkpoints. They do not receive independent dispatch, leases, receipts, or deletion ownership unless the pre-scope architect proves that one subblock is independently acceptable or the block crosses a mandatory rescope trigger. In that case, amend the DAG before production mutation rather than treating a train-sized subblock as an implementation checklist.

## Independently acceptable outcome

Close and promote engine provisioning only after policy, artifact validation, every opened source channel, deterministic resolution, atomic activation, offline/enterprise behavior, supply-chain evidence, capability truth, performance, teardown, and legacy deletion are proven on exact platform/topology rows. EPR6 adds no new acquisition or lifecycle behavior.

The current owner is **accepted EPR nodes plus residual tier logic, provider startup paths, package/discovery docs, manual status/capability claims, and unproven platform/offline topologies**. The final and sole owner is **one promoted engine-provisioning product receipt, exact source/platform/topology capability matrix, and structurally enforced acquisition-resolution-activation authority separation**.

## Architectural role and end state

EPR6 is a security-sensitive terminal. Any source, validation, selection, activation, enterprise, packaging, or performance defect reopens its owning EPR node; terminal work may not patch it locally.

## Expected production surfaces

- Rev11 EPR/VIM/PER0 authority and receipts
- toolchain resolver/activation/ProviderHub source and route inventories
- opened release packages/channels and installed-package evidence
- public engine status/capability/configuration docs
- legacy provisioning/editor architecture paths classified for deletion/relocation

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.

## Named APIs and data boundaries

- `EngineProvisioningProductReceipt`
- `EngineProvisioningConformanceManifest` and stable platform/topology/source rows
- `EngineSourceCapabilitySnapshot` and `EngineResidualLedger`
- `EngineProvisioningRouteDeletionManifest`
- `SupplyChainClosureReceipt` and `LifecycleSoakReceipt`

## Exact predecessor contracts

- **EPR5:** consume exact active engine lifecycle and capability publication.
- **VIM1:** consume deterministic conformance manifest generation.
- **PER0:** consume equivalent-work/latency/allocation/RSS methodology.
- **BR0:** consume successor product promotion authority.
- **CLI4:when-opened:** when opened, include CLI LSP/MCP engine status/launch adapters; when unopened prove no hidden CLI dependency.

External custody: none beyond the package activation boundary.

## Binding architecture

- Terminal certifies exact policy-applicable sources/platforms/topologies; unopened optional channels must prove absence/zero-work, not fake success.
- Supply-chain closure is required for every opened executable channel.
- Resolution and activation route inventories must show no bypass or unvalidated path-only execution.
- Offline/deny-network/proxy/custom-CA/enterprise behavior is mandatory where policy exposes it.
- Capabilities/status are generated from exact conformance and active receipts, never manually promoted.
- Product receipt binds policy, trust roots, revocation, platform matrix, packages, implementation, evidence, review, deletion, and residual digests.
- Legacy docs are deleted/relocated; Git history is the archive.

## Internal subblocks

### EPR6-SB1 - Conformance manifest and applicability closure

**Independently testable outcome:** Every policy-applicable source/platform/topology has an exact row; optional unopened channels have negative rows.

**Architecture:**

- Define rows for manual/project/system/editor/managed/bundle, OS/arch/libc, child/shared, online/offline/proxy, swap/crash/rollback.
- Mark required/opened/unopened/unsupported with owner.
- Bind exact toolchain/package fixtures.

**Expected changes:**

- Generate VIM rows/tests/receipts.
- Reject silent platform/topology omission.

**Discriminating proof:**

- Bijection/completeness guard passes.
- Public capability matrix equals applicable accepted rows.

### EPR6-SB2 - Supply-chain and installed-artifact terminal

**Independently testable outcome:** Every opened executable channel has current origin/integrity/provenance/license/SBOM/revocation/installed validation.

**Architecture:**

- Validate EPR1 receipts, EPR2 acquisition or EPR3 package receipts.
- Check current trust roots/revocation and final installed bytes.
- Keep unopened sources absent.

**Expected changes:**

- Capture supply-chain closure receipt.
- Withdraw/reopen on stale/revoked input.

**Discriminating proof:**

- Byte/package/origin/revocation mutation invalidates terminal.
- No execution inventory entry lacks validation receipt.

### EPR6-SB3 - Resolver/activation route deletion and no-bypass proof

**Independently testable outcome:** No code path selects/spawns/attaches an engine outside EPR4/EPR5.

**Architecture:**

- Generate call/path/symbol inventory for old tier/which/path/spawn/provider constructor routes.
- Delete routes/flags/helpers and add negative guards.
- Retain source/transport adapters only behind canonical authorities.

**Expected changes:**

- Perform bounded deletions in frozen candidate.
- Remove migration shims and stale tests/docs.

**Discriminating proof:**

- Planting path-only spawn/hidden source/first-found selection fails.
- Inventory has zero unexplained bypasses.

### EPR6-SB4 - Offline/enterprise/security terminal

**Independently testable outcome:** Deny-network/offline/proxy/custom-CA/secret hygiene and failure remediation pass exact adversarial matrix.

**Architecture:**

- Run network attempt monitors, malicious origin/redirect/archive/path fixtures, secret logging scans.
- Validate policy behavior under corporate proxy/air-gap/read-only cache.
- Test integrity/trust/revocation loud failure.

**Expected changes:**

- Capture security review/receipt under security-3.
- Reopen EPR0-EPR5 for findings.

**Discriminating proof:**

- Forbidden/offline modes make zero network attempts.
- No secret/path leak or silent trust downgrade.

### EPR6-SB5 - Lifecycle, capability, cancellation, and teardown terminal

**Independently testable outcome:** Selection/activation/swap/crash/restart/rollback/capabilities remain atomic and bounded across churn.

**Architecture:**

- Run concurrent project/provider/policy/edit transitions and long soak.
- Validate old epoch handle invalidation and stale-safe publication.
- Ensure teardown removes processes/transports/caches/resolve keys.

**Expected changes:**

- Capture lifecycle soak and capability receipts.
- Delete manual capability/status branches.

**Discriminating proof:**

- No stale/mixed epoch result or orphan process.
- Memory and resource counts plateau/release.

### EPR6-SB6 - Performance and zero-work terminal

**Independently testable outcome:** Warm resolution/healthy operation and disabled/unopened sources meet exact work/allocation/latency/RSS thresholds.

**Architecture:**

- Measure source calls/stats/hashes/network/spawn/handshake/allocations/retained bytes.
- Compare fresh/incremental/warm/disabled/offline/project churn.
- Separate explicit acquisition cost from ordinary resolution.

**Expected changes:**

- Capture PER0 terminal receipt.
- Reopen owner for unexplained regression.

**Discriminating proof:**

- Warm healthy requests perform zero resolution/activation.
- Unopened/disabled sources perform zero filesystem/network/package work.

### EPR6-SB7 - Legacy cleanup, product receipt, and promotion

**Independently testable outcome:** All provisioning facts are in Rev11/product docs, residual policy is honest, and promotion is immutable.

**Architecture:**

- Validate legacy disposition and relocate user/admin setup docs.
- Delete blocked future/tier/status architecture docs and obsolete tier numbering.
- Bind exact residual unsupported/platform rows.

**Expected changes:**

- Emit product receipt and permanent docs/route guards.
- Do not create archive directories.

**Discriminating proof:**

- No unclassified legacy path or live reference remains.
- Receipt invalidates on any authority/policy/trust/package/evidence change.

## Data, identity, invalidation, and publication laws

- Engine acquisition, resolution, selection, and activation are distinct authorities with distinct receipts; no stage infers success from a later stage.
- No executable is run before exact origin, compatibility, integrity, and trust validation succeeds.
- Unavailable, unauthorized, incompatible, corrupt, offline, cancelled, and operationally failed outcomes remain distinct and capability-visible.
- All caches/installations are content/version/platform scoped, race-safe, and fail closed on symlink/reparse/permission/ownership violations.
- EPR6 may not open an optional channel or add source/selection/activation behavior.
- Unopened EPR2/EPR3 are valid terminal states only when policy/capability/docs remove corresponding promises.
- Deletion/negative proof covers both source absence and structural inability to bypass validation/activation.

## Migration and cutover

- Run after EPR5 and every opened EPR2/EPR3 channel is accepted.
- Freeze candidate, generate complete conformance/route/source inventory, perform deletion/relocation, rerun security/performance/reviews.
- Reopen owning node on any defect.

## Deletions

- Delete displaced tier/discovery/selection/spawn/provider constructor/capability/status routes and legacy docs named by manifests.
- Delete stale policy claims for unopened channels.
- Delete temporary migration/config compatibility shims.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

## Forbidden designs

- Implementing missing channel/lifecycle behavior in terminal.
- Claiming offline/managed/bundled support without accepted applicable rows.
- Retaining path-only/first-found/unvalidated spawn fallback.
- Accepting stale supply-chain/security/performance evidence.
- Archiving legacy architecture in another folder.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

## Acceptance IDs and discriminating proof

- **EPR6-AC-MATRIX:** every policy-applicable source/platform/topology is accepted, unsupported, or unopened with exact evidence.
- **EPR6-AC-SUPPLY:** every opened executable channel has current installed-byte/origin/integrity/provenance/revocation closure.
- **EPR6-AC-NO-BYPASS:** route inventory proves all selection/activation flows use EPR4/EPR5.
- **EPR6-AC-SECURITY:** offline/proxy/air-gap/adversarial/secret/no-hidden-network matrix passes.
- **EPR6-AC-LIFECYCLE:** swap/crash/restart/rollback/capability/teardown soak is stale-safe and leak-free.
- **EPR6-AC-PERF:** warm/disabled/unopened equivalent-work and RSS thresholds pass.
- **EPR6-AC-HONEST:** public policy/capability/docs exactly match opened and certified scope.
- **EPR6-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **EPR6-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **EPR6-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **EPR6-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Performance and bounded work

- Terminal thresholds distinguish explicit acquisition side effects from ordinary resolver/healthy-request hot paths and require zero hidden work for disabled/unopened sources.
- Target ceiling: 300 production LOC, 3 production files, and 1 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Mandatory rescope and abort conditions

- Abort if any executable path lacks current validation/supply-chain/activation receipt.
- Abort if optional unopened source remains promised by public policy/docs.
- Abort if route/source inventory is incomplete or security review is not clean.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. Full VIM source/platform/topology and installed artifact matrix.
1. No-bypass route inventory/mutation guards and complete security-3 review.
1. Offline/proxy/adversarial/lifecycle/capability/cancel/teardown/performance/RSS terminal suites and immutable product receipt validation.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Consumers and unlocks

- Promotes exact engine provisioning/activation product.
- Provides stable truthful engine availability to CLI/LSP/language-service/diagnostics.
- Supports future engine flavors only through new policy/source/compatibility amendments.

## Source reconciliation

- All EPR authority/receipts, VIM/PER0/BR0 contracts, legacy provisioning disposition, release/package evidence.

Durable clauses are transferred as digest-bound requirement atoms. Historical path archaeology, obsolete branches, and implementation journals are not copied into the charter. Git history remains the archive.
