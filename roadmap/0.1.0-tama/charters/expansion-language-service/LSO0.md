<!-- unified-charter-v2
id=LSO0
name=Authored-coordinate semantic operation constitution
predecessors=UAI0,UAP0,TCM4,H3
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
external_requirements=
charter=charters/expansion-language-service/LSO0.md
size=M
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# LSO0 — Authored-coordinate semantic operation constitution

Readiness is derived only from implemented-ledger rows for the node ancestors. Commit message, approximate timezone-bearing date, and optional PR are loose locator hints; the CLI performs no Git, GitHub, SHA, tree, ancestry, receipt, lease, or digest validation.

## Independently acceptable outcome

Ratify one framework-neutral, provider-neutral, authored-coordinate semantic-operation constitution for navigation, occurrences, rename, completion, presentation, and edits. This block establishes ownership and public contracts only; it implements no feature execution.

The current owner is **feature-specific LSP handlers, generated-TSX mapping branches, provider-specific DTOs, framework-specific target heuristics, and separate rename/import edit paths**. The final and sole owner is **one language-service operation constitution, one authored target/provenance vocabulary, and one typed authored edit-intent boundary consumed by thin LSP/CLI/editor adapters**.

## Concrete surfaces and APIs

- Future production-owner inventory (read-only in this contract): `crates/verter_identity/src`, `crates/verter_protocol`, `crates/verter_session`, `crates/verter_span`, `crates/verter_lsp`.
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
- `CurrentProductOperationInventory` and `OperationObligation::{RequiredCurrent, Required, Optional, Unsupported, ExternalOwner, Removed}`
- `ProviderObservationRef` as opaque provenance only, never provider JSON or routing state

## Exact predecessor contracts

- **UAI0:** implemented ledger row for “Identity, carrier, parser, and coordinate contract lock”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **UAP0:** implemented ledger row for “Capability, coexistence, rule/action, formatter, and public contract lock”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **TCM4:** implemented ledger row for “Atomic activation and deletion”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **H3:** implemented ledger row for “Atomic readiness and stale-safe publication”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

### Binding architecture

- The core operates on authored source units, semantic subjects, and exact profile identity; LSP URI/Position, TSX paths, and editor client state are edge concerns.
- One canonical target/provenance graph serves definition, type-definition, implementation, references, hierarchy, rename, hover links, and completion resolve.
- A provider may contribute typed observations but does not define core request/result shapes.
- Every edit-producing feature emits typed edit intents first; only LSO8 validates and materializes an authored transaction.
- No feature accepts nearest-position, current-file-mapper fallback, Range::default, 0:0, or fabricated source anchors.
- Capabilities are profile/document-selector scoped and dynamically truthful under COX0; installing one conflicting editor extension must not disable unrelated operations.
- Every operation advertised or implemented by the current Verter product enters the inventory as `RequiredCurrent`. It can leave that class only when a named predecessor explicitly removes/unadvertises the operation and its public capability; `ExternalOwner`, `Unsupported`, or a residual-ledger row cannot satisfy it.
- External providers may remain observations behind the canonical Verter operation. Provider availability never changes a `RequiredCurrent` operation into external semantic ownership.
- Interactive leaf operations are demand-bounded and cannot trigger an implicit whole-workspace crawl.

### Internal subblocks

#### LSO0-SB1 - Operation taxonomy and authority matrix

**Independently testable outcome:** Every language-service operation has one owner, one result domain, and an explicit provider/native/framework composition law.

**Architecture:**

- Define navigation, occurrence, rename, completion, presentation, and edit transaction operation families.
- Name semantic authority versus observation/provider authority for each operation.
- Separate workspace discovery/index planning from authoritative semantic resolution.
- Seed the versioned current-product inventory from server capabilities, registered handlers, custom methods, editor/package consumers, and shipped documentation; silent omission is a schema failure.

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

- **LSO0-AC1 — sole-owner outcome:** the contract artifacts establish the named ownership rules and inventory every displaced production route with its later production-capable deletion owner; this zero-production node performs no runtime migration. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or reproduced defect that existing evidence does not discriminate.
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

This contract may remove superseded authority prose after preserving its durable clauses. Every production deletion listed here is an inventory obligation: bind it to an exact later production-capable owner and acceptance criterion before completing this contract. It authorizes no runtime route deletion.

- LSP or provider DTOs in the core operation API.
- Feature-specific target identity, mapper fallback, or edit application engines.
- Generated TSX/source text as semantic truth.
- Global enable/disable flags in place of profile-scoped capabilities.
- Raw edits returned before authored transaction validation.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

- Inventory and assign a later deletion owner for every compatibility path that would preserve a second production owner after cutover.
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

1. `node roadmap/0.1.0-tama/tools/validate-program-dag.mjs --strict`
2. Run every final command in the bound `docs-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

### Pack-specific verification inventory

1. Authority/catalog/source coverage validation.
1. Schema and negative architecture tests for authored-only contracts, typed outcomes, one target graph, and edit-intent routing.
1. Generated coexistence/capability table determinism test.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN evidence when applicable, the configured independent review profile, and the owning final gate on the squashed review candidate.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's predeclared row in `authority/state/implemented.toml` from `status = "pending"` to `status = "implemented"` with the planned squash commit message, approximate date with timezone, and optional pull-request number. The transitioned row is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.
