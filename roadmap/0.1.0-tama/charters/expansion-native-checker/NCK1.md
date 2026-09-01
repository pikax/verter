<!-- unified-charter-v2
id=NCK1
name=Executable-region and typed semantic-contribution contract
predecessors=NCK0,UAI0,PAR0,IDX0
phase=expansion
train=expansion.native-checker
product=native_checker
kind=contract
semantic_role=delivery
class=successor
owner=expansion.native-checker:sole native semantic diagnostic authority and certified-family cutover
conflict_domains=semantic_authority,carrier_parser,source_lineage
resource_class=docs-light
gate_profile=docs-domain
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
charter=charters/expansion-native-checker/NCK1.md
size=M
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# NCK1 — Executable-region and typed semantic-contribution contract

Readiness is derived only from implemented-ledger rows for the node ancestors. Commit message, approximate timezone-bearing date, and optional PR are loose locator hints; the CLI performs no Git, GitHub, SHA, tree, ancestry, receipt, lease, or digest validation.

## Independently acceptable outcome

Specify the framework-neutral executable-region graph and typed semantic-contribution ingress that the checker will consume, while preserving current semantic ownership and preventing adapters or indexes from becoming resolvers.

The current owner is **function-only flow structures, parser-specific body identities, framework-specific template analysis, and informal ProgramAnalysisContributor seams**. The final and sole owner is **one validated ExecutableRegionGraph identity model and one typed SemanticContributionBatch ingress consumed by the existing semantic graph and checker**.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_identity/src`, `crates/verter_language/src`, `crates/verter_parser`, `crates/verter_semantic`, `crates/verter_session`, `crates/verter_protocol`.
- Pack production inventory:
- `crates/verter_identity/src` and `crates/verter_language/src` for region/profile identities
- `crates/verter_parser` and framework parser outputs for region discovery descriptors
- `crates/verter_semantic` for region graph and typed contribution contracts
- `crates/verter_session` for validated contribution ingestion and project-scoped snapshots
- `crates/verter_protocol` only where PUB0 exposes region/provenance diagnostics, not internal graph internals

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.
- Named API/data boundaries:
- `ExecutableRegionId`, `ExecutableRegionKind`, `ExecutableRegionGraph`, and `ExecutableRegionSnapshot`
- `RegionStableHash`, `RegionRevision`, and explicit parent/owner source identities
- `SemanticContributionBatch` and closed typed `SemanticContribution` arms
- `ContributionProvenance`, `ContributionReadSet`, `ContributionValidation`
- `FrameworkRegionDescriptor` and `SemanticContributor` capability contract
- `ComponentContract` as a framework-neutral semantic contribution, not a checker-specific side table

## Exact predecessor contracts

- **NCK0:** implemented ledger row for “Native diagnostic authority and parity-certification constitution”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **UAI0:** implemented ledger row for “Identity, carrier, parser, and coordinate contract lock”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **PAR0:** implemented ledger row for “Parser decision, ownership, reuse, and lineage contract”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **IDX0:** implemented ledger row for “Atomic semantic contributions and workspace index”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

### Binding architecture

- A function is one ExecutableRegionKind, not the definition of an executable region.
- Region discovery is syntax/lowering work; type and diagnostic meaning are resolved later by the one semantic engine.
- Region nodes are compact and structural. Types, diagnostics, effects, and target-specific presentation live in side tables or query results.
- Stable identity is content-derived from semantic body structure and source lineage, cosmetic-insensitive where safe, and never a raw source offset alone.
- Contributors emit typed facts and demands. They cannot receive ProjectSemanticDispatch, raw resolver internals, or a callback that resolves types privately.
- Every contribution carries profile, source, environment, provenance, dependency read set, and validation status.
- IDX0 may index contribution identities and candidates but may not answer checker semantics.

### Internal subblocks

#### NCK1-SB1 - Region identity and taxonomy

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

#### NCK1-SB2 - Sparse executable-region graph shape

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

#### NCK1-SB3 - Typed contribution vocabulary

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

#### NCK1-SB4 - Provenance, read sets, and validation

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

#### NCK1-SB5 - Contributor capability boundary

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

#### NCK1-SB6 - Migration and compatibility proof

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

### Identity, invalidation, and publication

- Region IDs are project/profile/source qualified and never alias across files, framework profiles, or parser epochs.
- Region graph admission is independent of diagnostic demand; a graph may exist without checking and a check may demand only a slice.
- Contribution batches are immutable, sorted, validated, and atomically replaced by basis.
- A candidate index may point to contribution identity but cannot store final relation/call/checker verdicts.
- FrameworkRegion kinds remain open through profile registration; core code does not branch on Vue or Svelte names.

### Migration and cutover

- Reserve types and ownership first; NCK2 and NCK5 implement builders and ingestion.
- Map existing function-region identity with an explicit compatibility test, then remove legacy naming only when the final consumer moves.
- Admit no framework contribution until its profile epoch and validation contract are available.

### Consumers and unlocks

- Unlocks NCK2 diagnostic queries and NCK5 framework ingress.
- Provides region/target identity input to LSO2 without coupling language-service operations to checker internals.
- Provides a future common substrate for compiler and lint consumers that explicitly demand executable regions.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **NCK1-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or reproduced defect that existing evidence does not discriminate.
- **NCK1-AC2 — positive contract:** the named API/data boundary preserves exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **NCK1-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm; otherwise bind a terse not-applicable rationale.
- **NCK1-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, provider, filesystem, network, allocation, copy, or retained-candidate work; otherwise bind a terse not-applicable rationale.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; do not add implementation mirrors, duplicate permutations, or universal test quotas.
- Test homes: `crates/verter_session/tests/cases`, `crates/verter_protocol/tests`, `packages/typescript-plugin/src`, and the exact generated vertical fixture selected by this node.


### Pack-specific proof obligations

- **NCK1-AC-REGION:** region taxonomy and identity are exact, collision-tested, and preserve FunctionFlowGraph compatibility.
- **NCK1-AC-CONTRIBUTION:** every contribution arm has sole ownership, provenance, validation, and no text/fake-AST path.
- **NCK1-AC-BOUNDARY:** contributor contexts expose no resolver or provider capability.
- **NCK1-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **NCK1-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **NCK1-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **NCK1-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Deletions and forbidden designs

- Delete the function-is-the-only-region assumption from live architecture.
- Delete any adapter proposal that receives raw semantic dispatch or synthesizes TSX as semantic truth.
- Delete duplicate ProgramAnalysis contribution stores after atomic migration.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

- Source-offset-only region IDs, mutable region nodes, or per-node owned collections in hot structural storage.
- A framework-specific core enum branch or checker engine.
- Index-backed final semantic verdicts.
- Unvalidated injected narrowing/contextual/relation facts.
- Whole-workspace region graph construction as a prerequisite for an interactive leaf query.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

- Delete or structurally reject every compatibility path that would preserve a second owner after cutover.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 0 production LOC, 0 production files, 0 related crates/packages.
- Mandatory rescope above 1500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, identity aliasing, or unauthorized executable work.
- Performance budget: when preflight identifies touched authority or a hot path, use the ratified replacement SLO and equivalent-work counters below; otherwise performance evidence is not applicable and no soak is invented solely to populate evidence.

- Target ceiling: 0 production LOC, 0 production files, and 0 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Abort conditions

- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. `node roadmap/0.1.0-tama/tools/validate-program-dag.mjs --strict`
2. Run every final command in the bound `docs-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

### Pack-specific verification inventory

1. Authority/schema tests for region and contribution taxonomies.
1. Property tests for stable region identity and deterministic contribution digests.
1. Static API-surface negative tests for contributor access to resolver/session internals.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN evidence when applicable, the configured independent review profile, and the owning final gate on the squashed review candidate.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's predeclared row in `authority/state/implemented.toml` from `status = "pending"` to `status = "implemented"` with the planned squash commit message, approximate date with timezone, and optional pull-request number. The transitioned row is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.
