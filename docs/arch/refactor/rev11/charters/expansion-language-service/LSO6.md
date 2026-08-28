<!-- unified-charter-v2
id=LSO6
name=Completion candidates and provider-neutral resolve intents
predecessors=LSO0,LSO2,H2,TCM4,PUB0
conditional_predecessors=
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
source_refs=source:successor-dag-amendment.md:L1,source:legacy-arch-reconciliation.md:L1
external_requirements=
activation_gate=ORC0
charter=charters/expansion-language-service/LSO6.md
size=M
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# LSO6 — Completion candidates and provider-neutral resolve intents

Authority state is derived at dispatch. The canonical CLI validates the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

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

- **LSO0:** exact current receipt ID and digest for “Authored-coordinate semantic operation constitution”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **LSO2:** exact current receipt ID and digest for “Canonical authored target and provenance graph”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **H2:** exact current receipt ID and digest for “Project-scoped ProviderHub bindings”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **TCM4:** exact current receipt ID and digest for “Atomic activation and deletion”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **PUB0:** exact current receipt ID and digest for “Versioned public request/result and capability truth”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

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

1. `node docs/arch/refactor/rev11/tools/validate-program-dag.mjs --strict`
2. Run every final command in the bound `targeted-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

### Pack-specific verification inventory

1. Provider key/epoch swap/malformed negative tests.
1. Candidate identity/order/dedup and global-component/context fixtures.
1. Import intent current/foreign/self/no-script/stale-map matrix; cancellation/cache/memory/performance tests.

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

### SRC-LEGACY-LSO-COMPLETION-001

- Kind: `requirement`
- Source: `legacy-arch-reconciliation.md:117-124`
- Applicability: `LSO6`, `LSO8`
- Exact text SHA-256: `b6d4edc0ea47cf96a0796aedc8df996bdcb9ff1d3ab80937a6e641a156e8f01b`

~~~~markdown
### LSO-COMPLETION-001 — Provider-neutral completion resolve

- Completion candidates and resolve handles are typed and origin/provider-epoch scoped.
- Opaque provider JSON cannot be dispatched to another provider.
- Generated provider edits become authored import/additional edit intents.
- Resolve capability is advertised truthfully.
- Targets: `LSO6`, `LSO8`.
- Source: `docs/arch/provider-completion-resolve-design.md`, blob `83518adcd386d144e057ad98c4893678bd2f1b95`.
~~~~

### SRC-LEGACY-TRANSFER-C4DC7D887CAF

- Kind: `requirement`
- Source: `legacy-architecture-transfers.md:40-45`
- Applicability: `LSO6`, `LSO8`, `LSO9`
- Exact text SHA-256: `9552ae61d1b788dd858761227b78bb9c841f07e90f9979e14fc055e31e5b50fd`

~~~~markdown
### LEGACY-TRANSFER-C4DC7D887CAF

- Original path: `docs/arch/framework-import-placement-design.md`; Git blob: `c4dc7d887caf80cc796df4504f0ed4540527ebdf`; exact source SHA-256: `bbf39c10b39ef7bc8ea3e5a63d7dd56f350f1211bcba7936b695699988eae319`.
- Exact retained source: `sources/legacy-architecture-transfers/framework-import-placement-design.md`.
- Applicable authority: `LSO6`, `LSO8`, `LSO9`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~
