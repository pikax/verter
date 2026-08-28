<!-- unified-charter-v2
id=LSO0
name=Authored-coordinate semantic operation constitution
predecessors=UAI0,UAP0,TCM4,H3
conditional_predecessors=
phase=expansion
train=expansion.language-service
product=language_service
kind=constitution
semantic_role=delivery
class=successor
owner=expansion.language-service:one authored-coordinate semantic-operation and edit-transaction authority
conflict_domains=public_protocol,mapping_geometry,source_lineage
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
source_refs=source:successor-dag-amendment.md:L1,source:legacy-arch-reconciliation.md:L1
external_requirements=
activation_gate=ORC0
charter=charters/expansion-language-service/LSO0.md
size=M
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# LSO0 — Authored-coordinate semantic operation constitution

Authority state is derived at dispatch. The canonical CLI validates the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

## Independently acceptable outcome

Ratify one framework-neutral, provider-neutral, authored-coordinate semantic-operation constitution for navigation, occurrences, rename, completion, presentation, and edits. This block establishes ownership and public contracts only; it implements no feature execution.

The current owner is **feature-specific LSP handlers, generated-TSX mapping branches, provider-specific DTOs, framework-specific target heuristics, and separate rename/import edit paths**. The final and sole owner is **one language-service operation constitution, one authored target/provenance vocabulary, and one typed authored edit-intent boundary consumed by thin LSP/CLI/editor adapters**.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_identity/src`, `crates/verter_protocol`, `crates/verter_session`, `crates/verter_span`, `crates/verter_lsp`.
- Pack production inventory:
- `crates/verter_identity/src` for stable operation, target, occurrence, and transaction identities
- `crates/verter_protocol` for versioned request/result/outcome and capability contracts
- `crates/verter_session` for operation coordination over immutable project snapshots
- `crates/verter_span` and mapping owners for authored-coordinate basis types
- `crates/verter_lsp` only as a future protocol adapter, not semantic authority

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.
- Named API/data boundaries:
- `SemanticOperationKind`, `SemanticOperationRequest`, `SemanticOperationOutcome`, and `OperationBasis`
- `AuthoredTargetId`, `AuthoredTarget`, `TargetKind`, and `TargetProvenance`
- `OccurrenceId`, `OccurrenceRole`, and `OccurrenceSet`
- `EditIntentId`, `AuthoredEditIntent`, `EditSafetyClass`, and `EditPrecondition`
- `OperationCapability`, `OperationMaturity`, and exact profile participation masks
- `ProviderObservationRef` as opaque provenance only, never provider JSON or routing state

## Exact predecessor contracts

- **UAI0:** exact current receipt ID and digest for “Identity, carrier, parser, and coordinate contract lock”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **UAP0:** exact current receipt ID and digest for “Capability, coexistence, rule/action, formatter, and public contract lock”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **TCM4:** exact current receipt ID and digest for “Atomic activation and deletion”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **H3:** exact current receipt ID and digest for “Atomic readiness and stale-safe publication”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

### Binding architecture

- The core operates on authored source units, semantic subjects, and exact profile identity; LSP URI/Position, TSX paths, and editor client state are edge concerns.
- One canonical target/provenance graph serves definition, type-definition, implementation, references, hierarchy, rename, hover links, and completion resolve.
- A provider may contribute typed observations but does not define core request/result shapes.
- Every edit-producing feature emits typed edit intents first; only LSO8 validates and materializes an authored transaction.
- No feature accepts nearest-position, current-file-mapper fallback, Range::default, 0:0, or fabricated source anchors.
- Capabilities are profile/document-selector scoped and dynamically truthful under COX0; installing one conflicting editor extension must not disable unrelated operations.
- Interactive leaf operations are demand-bounded and cannot trigger an implicit whole-workspace crawl.

### Internal subblocks

#### LSO0-SB1 - Operation taxonomy and authority matrix

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

#### LSO0-SB2 - Typed request, outcome, and basis vocabulary

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

#### LSO0-SB3 - Authored target and provenance constitution

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

#### LSO0-SB4 - Occurrence and edit-intent constitution

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

#### LSO0-SB5 - Capability and coexistence law

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

#### LSO0-SB6 - Legacy source transfer and critical guards

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

### Identity, invalidation, and publication

- Core operation identity is rooted in project/profile/source basis, semantic subject, operation kind, demand, and capability epoch; presentation encoding is not semantic identity.
- Generated coordinates, provider handles, and editor DTOs may exist only inside typed adapters and never become public semantic truth.
- Cancellation, stale, superseded, partial, NeedInputs, unsupported, and ambiguous outcomes remain distinct and are never collapsed to empty success.
- All returned targets and edits carry exact authored provenance and validate the snapshot/mapping chain used to derive them.
- Operation and target IDs are stable across serialization and independent of message wording or LSP encoding.
- Indexes return bounded candidates and memberships; they never return authoritative semantic operation answers.
- A provider observation must name provider epoch and input basis, but provider epoch is not part of native semantic subject identity.

### Migration and cutover

- Land the constitution and amend PUB0/COX0/LRA0 contracts before implementing operations.
- Inventory existing feature routes and classify each as retained observation, migration source, or deletion target.
- Do not move any user-visible feature in LSO0.

### Consumers and unlocks

- Unlocks LSO1-LSO8 implementation.
- Defines the operation vocabulary consumed by VIM/COX/PUB and future editor clients.
- Provides the contract needed to delete the legacy feature-design corpus.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **LSO0-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or reproduced defect that existing evidence does not discriminate.
- **LSO0-AC2 — positive contract:** the named API/data boundary preserves exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **LSO0-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm; otherwise bind a terse not-applicable rationale.
- **LSO0-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, provider, filesystem, network, allocation, copy, or retained-candidate work; otherwise bind a terse not-applicable rationale.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; do not add implementation mirrors, duplicate permutations, or universal test quotas.
- Test homes: `crates/verter_session/tests/cases`, `crates/verter_protocol/tests`, `packages/typescript-plugin/src`, and the exact generated vertical fixture selected by this node.


### Pack-specific proof obligations

- **LSO0-AC-CATALOG:** every registered operation and legacy clause has exact ownership and result-domain classification.
- **LSO0-AC-AUTHORED:** public contracts contain no generated coordinate, TSX path, provider handle, or LSP position.
- **LSO0-AC-OUTCOMES:** typed outcome mutations distinguish empty success from every refusal/partial state.
- **LSO0-AC-EDIT-LAW:** every edit producer is statically required to route through LSO8.
- **LSO0-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **LSO0-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **LSO0-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **LSO0-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Deletions and forbidden designs

- Delete legacy architecture prose only after requirement atoms target LSO0-LSO10.
- Delete proposed per-feature mapping tables, raw-edit authorities, and provider-specific core DTO designs from live authority.
- Delete claims that V3/source maps alone are semantic target identity.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

- LSP or provider DTOs in the core operation API.
- Feature-specific target identity, mapper fallback, or edit application engines.
- Generated TSX/source text as semantic truth.
- Global enable/disable flags in place of profile-scoped capabilities.
- Raw edits returned before authored transaction validation.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

- Delete or structurally reject every compatibility path that would preserve a second owner after cutover.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 0 production LOC, 0 production files, 0 related crates/packages.
- Mandatory rescope above 1500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, identity aliasing, or unauthorized executable work.
- Performance budget: when preflight identifies touched authority or a hot path, use the ratified replacement SLO and equivalent-work counters below; otherwise performance evidence is not applicable and no soak is invented solely to populate evidence.

- The constitution declares explicit zero-work behavior and bounded-demand axes for every operation family.
- Target ceiling: 0 production LOC, 0 production files, and 0 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Abort conditions

- Abort if one operation cannot name a sole semantic owner and sole public result owner.
- Abort if a legacy feature requires approximate coordinates to preserve current behavior.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. `node docs/arch/refactor/rev11/tools/validate-program-dag.mjs --strict`
2. Run every final command in the bound `docs-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

### Pack-specific verification inventory

1. Authority/catalog/source coverage validation.
1. Schema and negative architecture tests for authored-only contracts, typed outcomes, one target graph, and edit-intent routing.
1. Generated coexistence/capability table determinism test.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the exact candidate tree, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact frozen worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired round handle; the complete gate command list; 3 fresh distinct harness review tasks for exactly `adversarial`, `conformance`, `architecture-specialist`, deterministic low|medium|high effort and exact author/task/agent/provider/model bindings; immutable-review-worktree and cleanup policy; and the required terse report-back schema. These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:successor-dag-amendment.md:L1`
- `source:legacy-arch-reconciliation.md:L1`

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-LEGACY-LSO-AUTHORED-001

- Kind: `requirement`
- Source: `legacy-arch-reconciliation.md:72-77`
- Applicability: `LSO0`, `LSO1`, `LSO10`, `LSO2`, `LSO3`, `LSO4`, `LSO5`, `LSO6`, `LSO7`, `LSO8`, `LSO9`
- Exact text SHA-256: `a1f1fa596f84a8e6d8d0ee8f97409e66df02aa16e466c983a13279fb859b4c25`

~~~~markdown
### LSO-AUTHORED-001 — Authored-coordinate public boundary

- Core operations use authored source units, semantic subjects, exact profiles, and typed outcomes.
- LSP positions, generated paths, provider JSON, and final workspace edits are edge concerns.
- Approximate, nearest, `Range::default`, and `0:0` fallbacks are forbidden.
- Targets: `LSO0`, all LSO implementations.
~~~~

### SRC-LEGACY-TRANSFER-9C48DB563E0F

- Kind: `requirement`
- Source: `legacy-architecture-transfers.md:250-255`
- Applicability: `LSO0`, `LSO2`, `LSO3`, `LSO4`, `LSO5`, `LSO8`, `LSO9`, `LSO10`
- Exact text SHA-256: `7aa614bb693d3c19bad3164ffd380fea4a8a8e9dce959cafcdefa31f656e6564`

~~~~markdown
### LEGACY-TRANSFER-9C48DB563E0F

- Original path: `docs/arch/goto-definition-architecture-decision.md`; Git blob: `9c48db563e0f411da1983d1b3cb5374b4f59b0ca`; exact source SHA-256: `7d706c49d70a317a9b218cdef55cc01ec7211ba4d0bdeadac60505e9e4a445c4`.
- Exact retained source: `sources/legacy-architecture-transfers/goto-definition-architecture-decision.md`.
- Applicable authority: `LSO0`, `LSO2`, `LSO3`, `LSO4`, `LSO5`, `LSO8`, `LSO9`, `LSO10`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-83518ADCD386

- Kind: `requirement`
- Source: `legacy-architecture-transfers.md:453-458`
- Applicability: `LSO0`, `LSO6`, `LSO8`, `LSO9`, `LSO10`
- Exact text SHA-256: `8de4e609efe8c048168adf3f843fd89ecbf6b4ce1d355cf6baa291392ff700e8`

~~~~markdown
### LEGACY-TRANSFER-83518ADCD386

- Original path: `docs/arch/provider-completion-resolve-design.md`; Git blob: `83518adcd386d144e057ad98c4893678bd2f1b95`; exact source SHA-256: `816a58bf9d48b36ff8cbe8cc2c0b64cb961d5f4b67fdabac6b8f65f5b5d2948b`.
- Exact retained source: `sources/legacy-architecture-transfers/provider-completion-resolve-design.md`.
- Applicable authority: `LSO0`, `LSO6`, `LSO8`, `LSO9`, `LSO10`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~
