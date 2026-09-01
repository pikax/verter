<!-- unified-charter-v2
id=LSO5
name=Semantic rename planning and conflict analysis
predecessors=LSO4,LRA0
phase=expansion
train=expansion.language-service
product=language_service
kind=implementation
semantic_role=delivery
class=successor
owner=expansion.language-service:one authored-coordinate semantic-operation and edit-transaction authority
conflict_domains=semantic_authority,diagnostic_action_service,mapping_geometry
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
charter=charters/expansion-language-service/LSO5.md
size=M
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# LSO5 — Semantic rename planning and conflict analysis

Readiness is derived only from implemented-ledger rows for the node ancestors. Commit message, approximate timezone-bearing date, and optional PR are loose locator hints; the CLI performs no Git, GitHub, SHA, tree, ancestry, receipt, lease, or digest validation.

## Independently acceptable outcome

Implement semantic rename planning as a distinct bounded block: classify the rename subject, select role-eligible occurrences, derive framework/language-aware replacement intents, detect conflicts, and return a typed RenamePlan. LSO5 never creates final workspace edits or writes files.

The current owner is **provider rename edits, native references reused without role policy, component/tag special cases, and direct WorkspaceEdit construction**. The final and sole owner is **one RenamePlanner over canonical targets and typed occurrences, producing authored edit intents plus explicit conflicts/refusals for LSO8 materialization**.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_semantic`, `crates/verter_session`, `crates/verter_actions`, `crates/verter_language`, `crates/verter_lsp`.
- Pack production inventory:
- `crates/verter_semantic` for rename subjects, policies, and conflict semantics
- `crates/verter_session` for project-scoped planning
- `crates/verter_actions` for edit-intent/safety integration
- `crates/verter_language` for profile-specific casing/contract contributions
- `crates/verter_lsp` prepareRename/rename adapters only

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.
- Named API/data boundaries:
- `RenameSubject`, `RenameSubjectKind`, and `RenamePolicy`
- `RenameRequest`, `RenamePlan`, `RenameRefusal`, and `RenameConflict`
- `RenameOccurrenceSelection` over `OccurrenceRole`
- `ReplacementIntent { occurrence, replacement, transform, preconditions }`
- `NameTransform` for exact alias/case/segment transformations
- `RenameSafety::{Safe, Suggested, Unsafe, Unsupported}`

## Exact predecessor contracts

- **LSO4:** implemented ledger row for “References, hierarchy, and bounded occurrence planning”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **LRA0:** implemented ledger row for “Profile-scoped diagnostics, lint, fixes, and actions”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

### Binding architecture

- Rename subject classification is semantic and profile-aware, not token-text heuristics.
- Each subject kind declares eligible occurrence roles, namespace, transformations, and conflict checks.
- The planner emits replacement intents with exact old text/anchor/revision preconditions; it does not emit LSP TextEdits.
- Component Pascal/kebab/local/global relationships are contributed as typed role/transform data, not Vue branches in neutral core.
- String/comment occurrences are excluded unless a language/framework contract explicitly marks them semantic.
- Ambiguous target, incomplete occurrences, stale inputs, unsupported transformation, or conflict yields typed refusal/plan status.
- No partial multi-file plan is represented as safely applicable.

### Internal subblocks

#### LSO5-SB1 - Rename subject classification and prepare contract

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

#### LSO5-SB2 - Role eligibility and occurrence selection

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

#### LSO5-SB3 - Language/framework replacement transforms

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

#### LSO5-SB4 - Conflict and legality analysis

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

#### LSO5-SB5 - Rename plan and edit intents

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

#### LSO5-SB6 - Provider comparison and migration

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

#### LSO5-SB7 - Bounded work and cancellation proof

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

### Identity, invalidation, and publication

- Core operation identity is rooted in project/profile/source basis, semantic subject, operation kind, demand, and capability epoch; presentation encoding is not semantic identity.
- Generated coordinates, provider handles, and editor DTOs may exist only inside typed adapters and never become public semantic truth.
- Cancellation, stale, superseded, partial, NeedInputs, unsupported, and ambiguous outcomes remain distinct and are never collapsed to empty success.
- All returned targets and edits carry exact authored provenance and validate the snapshot/mapping chain used to derive them.
- A safe RenamePlan is complete across its declared scope and exact basis.
- Replacement text is semantic plan data; final position encoding and transaction grouping belong to LSO8.
- Profile transform epochs enter plan identity and invalidation.

### Migration and cutover

- Characterize prepare/rename for bounded subject families.
- Migrate local/native bindings first, then imports/exports/properties, then Vue/Svelte component contracts.
- Keep unsupported families provider-owned until certified.

### Consumers and unlocks

- Unlocks LSO8 authored transaction materialization.
- Feeds LSO9 rename conformance.
- Provides reusable semantic rename plan for CLI/refactor clients.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **LSO5-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or reproduced defect that existing evidence does not discriminate.
- **LSO5-AC2 — positive contract:** the named API/data boundary preserves exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **LSO5-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm; otherwise bind a terse not-applicable rationale.
- **LSO5-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, provider, filesystem, network, allocation, copy, or retained-candidate work; otherwise bind a terse not-applicable rationale.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; do not add implementation mirrors, duplicate permutations, or universal test quotas.
- Test homes: `crates/verter_session/tests/cases`, `crates/verter_protocol/tests`, `packages/typescript-plugin/src`, and the exact generated vertical fixture selected by this node.


### Pack-specific proof obligations

- **LSO5-AC-SUBJECT:** prepare/classification is exact across supported subject kinds and refuses ambiguity.
- **LSO5-AC-ROLES:** role policy prevents same-name/namespace false edits.
- **LSO5-AC-TRANSFORM:** framework/language transforms are typed, round-trip tested, and data-driven.
- **LSO5-AC-CONFLICT:** planted scope/export/path conflicts block safe plans.
- **LSO5-AC-NO-RAW-EDIT:** output contains intents/preconditions only.
- **LSO5-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **LSO5-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **LSO5-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **LSO5-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Deletions and forbidden designs

- Delete direct provider WorkspaceEdit replay for migrated rename families.
- Delete name-only/string replace rename paths and feature-local casing logic.
- Delete semantic planners that materialize final TextEdits.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

- Rename implemented as references plus string replacement.
- Partial/incomplete plan labeled safe.
- Central framework switch for spelling transforms.
- Raw edits, line/column encoding, or file writes in RenamePlanner.
- Silent overlap/conflict resolution.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

- Delete or structurally reject every compatibility path that would preserve a second owner after cutover.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 800 production LOC, 8 production files, 2 related crates/packages.
- Mandatory rescope above 1500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, identity aliasing, or unauthorized executable work.
- Performance budget: when preflight identifies touched authority or a hot path, use the ratified replacement SLO and equivalent-work counters below; otherwise performance evidence is not applicable and no soak is invented solely to populate evidence.

- Planning work is proportional to LSO4 occurrences plus declared conflict scopes; no extra workspace scan.
- Target ceiling: 800 production LOC, 8 production files, and 2 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Abort conditions

- Abort if a subject family lacks complete role/conflict semantics.
- Abort if final edit materialization leaks into this block.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. `node roadmap/0.1.0-tama/tools/validate-program-dag.mjs --strict`
2. Run every final command in the bound `targeted-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

### Pack-specific verification inventory

1. Prepare/subject/role/namespace/conflict/transform mutation suites.
1. Vue/Svelte component/tag/prop/event/slot and global component fixtures.
1. Provider comparison, incremental/fresh, cancellation/budget, memory, and no-raw-edit guards.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN evidence when applicable, the configured independent review profile, and the owning final gate on the squashed review candidate.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's predeclared row in `authority/state/implemented.toml` from `status = "pending"` to `status = "implemented"` with the planned squash commit message, approximate date with timezone, and optional pull-request number. The transitioned row is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.
