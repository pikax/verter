<!-- unified-charter-v2
id=LSO8
name=Authored edit transaction engine for rename, fixes, and imports
predecessors=LSO1,LSO5,LSO6,LRA0,ENCL0
phase=expansion
train=expansion.language-service
product=language_service
kind=implementation
semantic_role=delivery
class=successor
owner=expansion.language-service:one authored-coordinate semantic-operation and edit-transaction authority
conflict_domains=diagnostic_action_service,mapping_geometry,source_lineage,lsp_publication
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
charter=charters/expansion-language-service/LSO8.md
size=M
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# LSO8 — Authored edit transaction engine for rename, fixes, and imports

Readiness is derived only from implemented-ledger rows for the node ancestors. Commit message, approximate timezone-bearing date, and optional PR are loose locator hints; the CLI performs no Git, GitHub, SHA, tree, ancestry, receipt, lease, or digest validation.

## Independently acceptable outcome

Implement the sole authored edit transaction engine for semantic rename plans, completion/import intents, diagnostic fixes, code actions, and future refactors. It validates exact document/project preconditions, resolves insertion anchors and mapping provenance, detects overlap/conflict, classifies safety, and materializes one atomic multi-file transaction.

The current owner is **direct LSP WorkspaceEdit construction, provider-generated file edits, per-feature import re-anchoring, ad hoc overlap handling, and command-local filesystem writes**. The final and sole owner is **one AuthoredEditTransactionBuilder and transaction validator with exact basis/preconditions, deterministic edits, atomic application semantics, and thin protocol/filesystem adapters**.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_actions`, `crates/verter_session`, `crates/verter_span`, `crates/verter_lsp`, `packages`.
- Pack production inventory:
- `crates/verter_actions` for transaction/edit intent/safety authority
- `crates/verter_session` for immutable source/project snapshot validation
- `crates/verter_span` and mapping owners for exact authored anchors
- `crates/verter_lsp` for WorkspaceEdit projection/application negotiation
- `packages`/CLI application service only for thin write adapters when opened

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.
- Named API/data boundaries:
- `AuthoredEditTransaction`, `AuthoredEditTransactionId`, and `TransactionBasis`
- `AuthoredEdit`, `TextReplacementIntent`, `InsertionIntent`, `FileOperationIntent`
- `EditPrecondition::{Revision, Hash, OldText, TargetIdentity, AuthorityEpoch, MappingBasis}`
- `InsertionAnchor`, `ImportPlacementPolicy`, and `AnchorResolution`
- `EditConflict`, `OverlapClass`, `TransactionSafety`, and `TransactionRefusal`
- `TransactionApplyReceipt` and atomic write boundary

## Exact predecessor contracts

- **LSO1:** implemented ledger row for “Tolerant carrier recovery and two-rail syntax/semantic diagnostics”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **LSO5:** implemented ledger row for “Semantic rename planning and conflict analysis”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **LSO6:** implemented ledger row for “Completion candidates and provider-neutral resolve intents”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **LRA0:** implemented ledger row for “Profile-scoped diagnostics, lint, fixes, and actions”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **ENCL0:** implemented ledger row for “LSP and editor coordinate-boundary cutover”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

### Binding architecture

- Every edit is authored-coordinate and validates source revision/hash plus semantic/mapping/authority basis appropriate to its origin.
- Semantic producers cannot materialize final TextEdits/WorkspaceEdits or write files.
- All intents are normalized, sorted, overlap-checked, and either accepted as one transaction or refused; partial safe application is forbidden.
- Insertion placement is syntax/structure-aware through explicit authored anchors, never line-zero, nearest position, or text regex.
- Provider-generated edits are evidence only until normalized to authored intents and validated against exact target-file context.
- Foreign-file mapping uses the foreign file snapshot/mapper/anchors; current-file context is never reused.
- Application is atomic at the adapter boundary or returned as a plan requiring an adapter capable of equivalent atomic precondition checks.

### Internal subblocks

#### LSO8-SB1 - Transaction basis and precondition model

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

#### LSO8-SB2 - Intent normalization and deterministic ordering

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

#### LSO8-SB3 - Structural insertion anchors and import placement

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

#### LSO8-SB4 - Mapping and foreign-file edit validation

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

#### LSO8-SB5 - Overlap, conflict, safety, and atomicity

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

#### LSO8-SB6 - Protocol/application adapters and receipts

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

#### LSO8-SB7 - Transaction conformance, cancellation, and performance

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

### Identity, invalidation, and publication

- Core operation identity is rooted in project/profile/source basis, semantic subject, operation kind, demand, and capability epoch; presentation encoding is not semantic identity.
- Generated coordinates, provider handles, and editor DTOs may exist only inside typed adapters and never become public semantic truth.
- Cancellation, stale, superseded, partial, NeedInputs, unsupported, and ambiguous outcomes remain distinct and are never collapsed to empty success.
- All returned targets and edits carry exact authored provenance and validate the snapshot/mapping chain used to derive them.
- Transaction identity includes normalized intent set and exact basis, not final LSP encoding.
- Only LSO8 or an explicitly delegated equivalent validator may produce applicable workspace edits.
- An apply receipt binds the exact transaction digest and observed preconditions.

### Migration and cutover

- Introduce transaction builder for one completion import path, then code actions, rename, and fixes.
- Characterize exact existing outputs but reject unsafe fallback behavior as intentional correction.
- Delete direct edit builders immediately after the last producer migrates.

### Consumers and unlocks

- Provides applicable edit transactions to LSP/CLI and future refactors.
- Unlocks complete LSO9 conformance.
- Serves NCK7/LRA0 diagnostic fix intents.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **LSO8-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or reproduced defect that existing evidence does not discriminate.
- **LSO8-AC2 — positive contract:** the named API/data boundary preserves exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **LSO8-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm; otherwise bind a terse not-applicable rationale.
- **LSO8-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, provider, filesystem, network, allocation, copy, or retained-candidate work; otherwise bind a terse not-applicable rationale.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; do not add implementation mirrors, duplicate permutations, or universal test quotas.
- Test homes: `crates/verter_session/tests/cases`, `crates/verter_protocol/tests`, `packages/typescript-plugin/src`, and the exact generated vertical fixture selected by this node.


### Pack-specific proof obligations

- **LSO8-AC-PRECONDITIONS:** every edit/transaction validates exact source/semantic/mapping/authority basis.
- **LSO8-AC-ANCHORS:** structural insertion matrices place or refuse without fallback.
- **LSO8-AC-FOREIGN:** foreign edits use foreign context and cannot land in current/file-top synthetic positions.
- **LSO8-AC-CONFLICT:** overlap/path/failure injection proves deterministic all-or-nothing behavior.
- **LSO8-AC-ADAPTERS:** LSP/CLI projections preserve one transaction digest and semantics.
- **LSO8-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **LSO8-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **LSO8-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **LSO8-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Deletions and forbidden designs

- Delete direct WorkspaceEdit/TextEdit construction in semantic rename/completion/fix modules.
- Delete duplicate completion/code-action import re-anchor implementations, file-top fallbacks, current-file mapper reuse, and partial overlap application.
- Delete command-local non-atomic multi-file write paths.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

- Unchecked raw edits or filesystem writes from semantic producers.
- Approximate/nearest/0:0 insertion or mapping fallback.
- Partial application of a plan claimed safe.
- Current-file mapper/anchor used for a foreign edit.
- Regex/text-sniff import placement or silent overlap resolution.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

- Delete or structurally reject every compatibility path that would preserve a second owner after cutover.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 800 production LOC, 8 production files, 2 related crates/packages.
- Mandatory rescope above 1500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, identity aliasing, or unauthorized executable work.
- Performance budget: when preflight identifies touched authority or a hot path, use the ratified replacement SLO and equivalent-work counters below; otherwise performance evidence is not applicable and no soak is invented solely to populate evidence.

- Planning/materialization work is linear in normalized intents/files plus bounded mapping/parse facts; no hidden provider/semantic recheck.
- Target ceiling: 800 production LOC, 8 production files, and 2 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Abort conditions

- Abort if an intent lacks enough authored provenance/preconditions to validate.
- Abort if an adapter cannot preserve required atomicity and a best-effort partial fallback is proposed.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. `node roadmap/0.1.0-tama/tools/validate-program-dag.mjs --strict`
2. Run every final command in the bound `targeted-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

### Pack-specific verification inventory

1. Intent normalization/order/overlap/conflict mutation matrix.
1. Import anchor and current/foreign/self/no-script/recovery/stale-map suites.
1. Concurrent edit/config/profile drift, client capability, atomic failure injection, cancellation, allocation, staged-byte, and memory tests.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN evidence when applicable, the configured independent review profile, and the owning final gate on the squashed review candidate.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's predeclared row in `authority/state/implemented.toml` from `status = "pending"` to `status = "implemented"` with the planned squash commit message, approximate date with timezone, and optional pull-request number. The transitioned row is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.
