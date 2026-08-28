<!-- unified-charter-v2
id=LSO4
name=References, hierarchy, and bounded occurrence planning
predecessors=LSO2,IDX0
conditional_predecessors=
phase=expansion
train=expansion.language-service
product=language_service
kind=implementation
semantic_role=delivery
class=successor
owner=expansion.language-service:one authored-coordinate semantic-operation and edit-transaction authority
conflict_domains=semantic_authority,mapping_geometry,performance_evidence
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
source_refs=source:successor-dag-amendment.md:L1,source:legacy-arch-reconciliation.md:L1
external_requirements=
activation_gate=ORC0
charter=charters/expansion-language-service/LSO4.md
size=M
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# LSO4 — References, hierarchy, and bounded occurrence planning

Authority state is derived at dispatch. The canonical CLI validates the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

## Independently acceptable outcome

Implement bounded semantic occurrence planning for references, call/type hierarchy, incoming/outgoing relationships, and rename candidate discovery. LSO4 returns role-typed occurrences and hierarchy edges over exact targets; it does not decide replacement text or materialize edits.

The current owner is **provider reference arrays, native binding scans, generated-text occurrences, feature-local workspace traversal, and untyped ranges reused by rename**. The final and sole owner is **one OccurrencePlanner over LSO2/IDX0 with typed occurrence roles, exact target identity, bounded enumeration, hierarchy edges, and explicit completeness**.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_semantic`, `crates/verter_session`, `crates/verter_language`, `crates/verter_identity`, `crates/verter_type_runtime`, `crates/verter_lsp`.
- Pack production inventory:
- `crates/verter_semantic` for occurrence roles and hierarchy edge semantics
- `crates/verter_session` for project-scoped planning and semantic validation
- `crates/verter_language`/`crates/verter_identity` for profile/target identities
- `crates/verter_type_runtime` adapters for provider observations
- `crates/verter_lsp` thin references/hierarchy adapters

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.
- Named API/data boundaries:
- `OccurrencePlanner`, `OccurrenceRequest`, and `OccurrenceDemand`
- `SemanticOccurrence { id, target, role, authored_anchor, basis }`
- `OccurrenceRole` including declaration/read/write/call/type/import/export/tag/attribute/string-contract roles
- `OccurrencePage`, `OccurrenceCursor`, and `OccurrenceCompleteness`
- `HierarchyEdge`, `HierarchyKind`, and `HierarchyResult`
- `CandidateSource::{Index, LocalSemantic, ProviderObservation, FrameworkContribution}`

## Exact predecessor contracts

- **LSO2:** exact current receipt ID and digest for “Canonical authored target and provenance graph”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **IDX0:** exact current receipt ID and digest for “Atomic semantic contributions and workspace index”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

### Binding architecture

- IDX0 narrows files/symbol candidates; every returned occurrence is semantically validated against the canonical target.
- Occurrence roles are explicit and preserved through mapping; rename may select by role without parsing text.
- Generated/provider occurrences normalize to authored anchors under exact snapshot provenance before entering the result.
- Incomplete enumeration, budget exhaustion, cancellation, stale inputs, and unsupported provider capabilities are typed and never negative-cached as complete.
- References and hierarchy may stream/pages; ordering and cursor identity are deterministic on an immutable basis.
- A leaf/local request does not scan all project files when the index can prove bounded candidates.
- Hierarchy edges are semantic target relationships, not textual name matches.

### Internal subblocks

#### LSO4-SB1 - Occurrence role and identity model

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

#### LSO4-SB2 - Bounded candidate planning

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

#### LSO4-SB3 - Semantic occurrence validation

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

#### LSO4-SB4 - Reference result assembly and pagination

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

#### LSO4-SB5 - Hierarchy relationship planning

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

#### LSO4-SB6 - Provider/framework parity and work evidence

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

### Identity, invalidation, and publication

- Core operation identity is rooted in project/profile/source basis, semantic subject, operation kind, demand, and capability epoch; presentation encoding is not semantic identity.
- Generated coordinates, provider handles, and editor DTOs may exist only inside typed adapters and never become public semantic truth.
- Cancellation, stale, superseded, partial, NeedInputs, unsupported, and ambiguous outcomes remain distinct and are never collapsed to empty success.
- All returned targets and edits carry exact authored provenance and validate the snapshot/mapping chain used to derive them.
- Occurrence cursors are snapshot-scoped capabilities and are not serializable long-term cache keys.
- A source span may host multiple occurrence roles; range deduplication alone is forbidden.
- Incomplete candidate enumeration cannot publish a complete empty occurrence set.

### Migration and cutover

- Migrate same-file references, then indexed project references, then provider/framework contributions.
- Move call/type hierarchy after occurrence and target identity are stable.
- Keep rename materialization in LSO5/LSO8.

### Consumers and unlocks

- Unlocks LSO5 semantic rename planning.
- Feeds LSO9 references/hierarchy conformance.
- Provides occurrence sets to future refactor/search operations.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **LSO4-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or reproduced defect that existing evidence does not discriminate.
- **LSO4-AC2 — positive contract:** the named API/data boundary preserves exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **LSO4-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm; otherwise bind a terse not-applicable rationale.
- **LSO4-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, provider, filesystem, network, allocation, copy, or retained-candidate work; otherwise bind a terse not-applicable rationale.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; do not add implementation mirrors, duplicate permutations, or universal test quotas.
- Test homes: `crates/verter_session/tests/cases`, `crates/verter_protocol/tests`, `packages/typescript-plugin/src`, and the exact generated vertical fixture selected by this node.


### Pack-specific proof obligations

- **LSO4-AC-ROLES:** every occurrence has exact role/target/basis and planted same-name false positives fail.
- **LSO4-AC-BOUNDED:** candidate/validation work is bounded and zero for inapplicable profiles.
- **LSO4-AC-PAGES:** pagination permutations reconstruct one deterministic exact set.
- **LSO4-AC-HIERARCHY:** hierarchy edges are semantic, cycle-safe, and share target rendering.
- **LSO4-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **LSO4-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **LSO4-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **LSO4-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Deletions and forbidden designs

- Delete eager workspace/name-only reference scans and generated-range result construction.
- Delete feature-local occurrence dedup/order/pagination.
- Delete hierarchy text matching and current-file mapper fallback.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

- Index candidates treated as authoritative references.
- Name-only/string search as semantic validation.
- Range-only occurrence identity/dedup.
- Unbounded project enumeration on interactive requests.
- Caching a budget-exhausted negative as complete.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

- Delete or structurally reject every compatibility path that would preserve a second owner after cutover.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 800 production LOC, 8 production files, 2 related crates/packages.
- Mandatory rescope above 1500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, identity aliasing, or unauthorized executable work.
- Performance budget: when preflight identifies touched authority or a hot path, use the ratified replacement SLO and equivalent-work counters below; otherwise performance evidence is not applicable and no soak is invented solely to populate evidence.

- Warm occurrence queries reuse index/semantic facts; memory remains bounded across cancelled/abandoned cursors.
- Target ceiling: 800 production LOC, 8 production files, and 2 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Abort conditions

- Abort if a required occurrence role can only be inferred from generated text or regex.
- Abort if the index cannot name a complete/read-set basis for a claimed complete result.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. `node docs/arch/refactor/rev11/tools/validate-program-dag.mjs --strict`
2. Run every final command in the bound `targeted-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

### Pack-specific verification inventory

1. Role/identity/same-name negative fixtures; indexed bounded-work tests.
1. Pagination/cursor stale/cancel/memory tests.
1. Provider/framework differential and call/type hierarchy cycle suites.

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

### SRC-LEGACY-LSO-OCCURRENCE-001

- Kind: `requirement`
- Source: `legacy-arch-reconciliation.md:103-108`
- Applicability: `LSO4`, `LSO5`
- Exact text SHA-256: `d6428154331475f108564a8da2f0c1f9fc800420967372617e56192312308e3b`

~~~~markdown
### LSO-OCCURRENCE-001 — Typed bounded occurrences

- References/hierarchy/rename use role-typed semantic occurrences.
- IDX0 narrows candidates but never establishes authority.
- Incomplete/budgeted/cancelled enumeration cannot cache a complete negative result.
- Targets: `LSO4`, `LSO5`.
~~~~

### SRC-LEGACY-TRANSFER-DE8DD6E47BDF

- Kind: `requirement`
- Source: `legacy-architecture-transfers.md:166-171`
- Applicability: `LSO4`, `PER0`
- Exact text SHA-256: `5dab874a35f6fad4983f5b3af4ae8681b03695b38a86f804090087acaed6a8e1`

~~~~markdown
### LEGACY-TRANSFER-DE8DD6E47BDF

- Original path: `docs/arch/future/semantic-dispatch-connected-depth-budget-reset.md`; Git blob: `de8dd6e47bdf9f10d2e556d088f827bbf72f8ad9`; exact source SHA-256: `9e25db54f57b62dab17cba592334b0ddbc0f4af1aa70fd80394898ee0fcf85bd`.
- Exact retained source: `sources/legacy-architecture-transfers/future/semantic-dispatch-connected-depth-budget-reset.md`.
- Applicable authority: `LSO4`, `PER0`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~
