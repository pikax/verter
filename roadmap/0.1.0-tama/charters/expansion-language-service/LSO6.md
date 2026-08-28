<!-- unified-charter-v2
id=LSO6
name=Completion candidates and provider-neutral resolve intents
predecessors=LSO0,LSO2,H2,TCM4,PUB0
phase=expansion
train=expansion.language-service
product=language_service
kind=implementation
semantic_role=delivery
class=successor
owner=expansion.language-service:one authored-coordinate semantic-operation and edit-transaction authority
conflict_domains=provider_lifecycle,mapping_geometry,public_protocol,semantic_authority
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
charter=charters/expansion-language-service/LSO6.md
size=M
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# LSO6 — Completion candidates and provider-neutral resolve intents

Readiness is derived only from implemented-ledger rows for the node ancestors. Commit message, approximate timezone-bearing date, and optional PR are loose locator hints; the CLI performs no Git, GitHub, SHA, tree, ancestry, receipt, lease, or digest validation.

## Independently acceptable outcome

Implement one provider-neutral completion pipeline: authored completion context classification, bounded candidate composition, typed lazy resolve handles, exact provider-epoch validation, and authored import/fix intents. Completion resolve never emits unchecked generated-file edits.

The current owner is **provider-specific completion parsing, opaque JSON data envelopes, LSP-baked routing flags, generated TSX import edits, and separate workspace component candidates**. The final and sole owner is **one CompletionService with normalized candidates and typed resolve intents, plus thin provider adapters and LSO8-authored transaction materialization**.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_session`, `crates/verter_type_runtime`, `crates/verter_semantic`, `crates/verter_language`, `crates/verter_protocol`, `crates/verter_lsp`.
- Pack production inventory:
- `crates/verter_session` completion coordination and candidate composition
- `crates/verter_type_runtime` provider-specific candidate/resolve adapters
- `crates/verter_semantic`/`crates/verter_language` native/framework candidates and context
- `crates/verter_protocol` normalized completion contracts
- `crates/verter_lsp` envelope serialization and completionItem adapter only

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.
- Named API/data boundaries:
- `CompletionRequest`, `CompletionContext`, `CompletionCandidate`, and `CompletionSet`
- `CompletionOrigin`, `CompletionCandidateId`, `CompletionKind`, and `SortGroup`
- `CompletionResolveKey::{Provider, Native, Framework, Workspace}` with typed payloads
- `CompletionResolveRequest`, `CompletionResolveResult`, and exact epoch/basis validation
- `ImportIntent`, `AdditionalEditIntent`, and `CompletionDocumentation`
- `CompletionCapability` and honest resolve support

## Exact predecessor contracts

- **LSO0:** implemented ledger row for “Authored-coordinate semantic operation constitution”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **LSO2:** implemented ledger row for “Canonical authored target and provenance graph”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **H2:** implemented ledger row for “Project-scoped ProviderHub bindings”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **TCM4:** implemented ledger row for “Atomic activation and deletion”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **PUB0:** implemented ledger row for “Versioned public request/result and capability truth”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

### Binding architecture

- Completion candidates normalize into provider-neutral identity/kind/target/origin; provider opaque data stays inside a typed resolve key.
- Resolve keys are valid only for the exact provider/native/framework epoch and request basis that created them.
- The LSP envelope contains routing serialization only and is rejected on provider/profile/session mismatch.
- Additional provider edits normalize to authored edit intents; preamble imports are classified structurally before any strict map result is accepted.
- Workspace/native/framework/provider candidates compose deterministically and dedup by semantic target/candidate identity, not label text alone.
- Advertised resolve capability reflects the active providers/candidate origins actually supported.
- Completion list is demand-bounded and may be incomplete with explicit continuation/completeness; it is never silently truncated as complete.

### Internal subblocks

#### LSO6-SB1 - Authored completion context classifier

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

#### LSO6-SB2 - Normalized candidate identity and composition

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

#### LSO6-SB3 - Typed resolve keys and provider adapters

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

#### LSO6-SB4 - Authored import and additional edit intents

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

#### LSO6-SB5 - Documentation/detail enrichment and capability truth

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

#### LSO6-SB6 - Completion performance, cancellation, and conformance

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

### Identity, invalidation, and publication

- Core operation identity is rooted in project/profile/source basis, semantic subject, operation kind, demand, and capability epoch; presentation encoding is not semantic identity.
- Generated coordinates, provider handles, and editor DTOs may exist only inside typed adapters and never become public semantic truth.
- Cancellation, stale, superseded, partial, NeedInputs, unsupported, and ambiguous outcomes remain distinct and are never collapsed to empty success.
- All returned targets and edits carry exact authored provenance and validate the snapshot/mapping chain used to derive them.
- Resolve key identity is origin-specific but serialization is provider-neutral.
- Import/additional edit intents are not applicable until LSO8 validates the authored transaction.
- Candidate display enrichment does not mutate semantic candidate identity.

### Migration and cutover

- Introduce typed keys while preserving existing candidate output, then migrate each provider.
- Move workspace/framework candidates into shared composition.
- Move provider additional edits to authored intents and delete direct LSP edit replay.

### Consumers and unlocks

- Feeds LSO8 edit transaction materialization and LSO9 conformance.
- Provides thin completion adapters to LSP/editors.
- Preserves external providers without provider-shaped core APIs.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **LSO6-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or reproduced defect that existing evidence does not discriminate.
- **LSO6-AC2 — positive contract:** the named API/data boundary preserves exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **LSO6-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm; otherwise bind a terse not-applicable rationale.
- **LSO6-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, provider, filesystem, network, allocation, copy, or retained-candidate work; otherwise bind a terse not-applicable rationale.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; do not add implementation mirrors, duplicate permutations, or universal test quotas.
- Test homes: `crates/verter_session/tests/cases`, `crates/verter_protocol/tests`, `packages/typescript-plugin/src`, and the exact generated vertical fixture selected by this node.


### Pack-specific proof obligations

- **LSO6-AC-CANDIDATES:** normalized semantic candidate identity/dedup/order is stable across origin order.
- **LSO6-AC-RESOLVE-KEY:** foreign/stale/malformed keys fail closed before provider invocation.
- **LSO6-AC-IMPORT:** preamble/import edits become exact authored intents or typed refusal, never 0:0.
- **LSO6-AC-CAPABILITY:** advertised resolve support is exact for active origins.
- **LSO6-AC-PROVIDERS:** tsgo/tsserver/extension/native/framework matrix preserves equivalent actionable behavior.
- **LSO6-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **LSO6-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **LSO6-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **LSO6-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Deletions and forbidden designs

- Delete `{ tsgo: true, original_data, tsx_path }` and arbitrary provider JSON routing.
- Delete provider-specific completion merge/dedup and direct generated edit translation.
- Delete dishonest resolve capability and current-file/foreign-file import fallbacks.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

- Opaque JSON provider data in core candidate/results.
- Provider ID or generated path encoded in display fields.
- Accepting strict-mapped preamble insertion at carrier file top.
- Final TextEdit/WorkspaceEdit materialization in completion service.
- Label-only candidate dedup or unbounded candidate enumeration.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

- Delete or structurally reject every compatibility path that would preserve a second owner after cutover.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 800 production LOC, 8 production files, 2 related crates/packages.
- Mandatory rescope above 1500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, identity aliasing, or unauthorized executable work.
- Performance budget: when preflight identifies touched authority or a hot path, use the ratified replacement SLO and equivalent-work counters below; otherwise performance evidence is not applicable and no soak is invented solely to populate evidence.

- List/resolve work is context-demanded and bounded; inactive origins perform zero work.
- Target ceiling: 800 production LOC, 8 production files, and 2 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Abort conditions

- Abort if a provider cannot expose a typed stable resolve key.
- Abort if an edit cannot be mapped to an exact authored intent and a heuristic fallback is proposed.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. `node roadmap/0.1.0-tama/tools/validate-program-dag.mjs --strict`
2. Run every final command in the bound `targeted-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

### Pack-specific verification inventory

1. Provider key/epoch swap/malformed negative tests.
1. Candidate identity/order/dedup and global-component/context fixtures.
1. Import intent current/foreign/self/no-script/stale-map matrix; cancellation/cache/memory/performance tests.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN evidence when applicable, the configured independent review profile, and the owning final gate on the squashed review candidate.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch adds one `[[implemented]]` row to `authority/state/implemented.toml` with the node ID, planned squash commit message, approximate date with timezone, and optional pull-request number. Row presence is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.
