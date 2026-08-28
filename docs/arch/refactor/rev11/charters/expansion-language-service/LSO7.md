<!-- unified-charter-v2
id=LSO7
name=Hover, signature-help, and inlay presentation composition
predecessors=LSO0,LSO2,H2,TCM4,PUB0
conditional_predecessors=
phase=expansion
train=expansion.language-service
product=language_service
kind=implementation
semantic_role=delivery
class=successor
owner=expansion.language-service:one authored-coordinate semantic-operation and edit-transaction authority
conflict_domains=provider_lifecycle,public_protocol,lsp_publication,semantic_authority
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
charter=charters/expansion-language-service/LSO7.md
size=M
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# LSO7 — Hover, signature-help, and inlay presentation composition

Authority state is derived at dispatch. The canonical CLI validates the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

## Independently acceptable outcome

Implement one presentation composition service for hover, signature help, and inlay hints. It combines authoritative native/framework facts and provider observations under explicit per-fragment authority, then returns authored-range semantic presentation fragments independent of editor markup/protocol.

The current owner is **feature-local native/provider merge rules, early returns, provider text dominance heuristics, generated helper stripping, and LSP-specific markup construction**. The final and sole owner is **one PresentationService with stable subjects/fragments, explicit authority/provenance, exact authored ranges, and thin LSP/editor renderers**.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_session`, `crates/verter_semantic`, `crates/verter_type_runtime`, `crates/verter_protocol`, `crates/verter_lsp`.
- Pack production inventory:
- `crates/verter_session` presentation coordination
- `crates/verter_semantic` native fact/subject extraction
- `crates/verter_type_runtime` provider observation adapters
- `crates/verter_protocol` fragment/result schemas
- `crates/verter_lsp` Markdown/MarkupContent/SignatureHelp/InlayHint projection only

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.
- Named API/data boundaries:
- `PresentationRequest`, `PresentationKind`, and `PresentationSubject`
- `PresentationFragment`, `FragmentKind`, `FragmentAuthority`, and `FragmentProvenance`
- `HoverPresentation`, `SignaturePresentation`, and `InlayPresentation`
- `ActiveSignature`, `ActiveParameter`, and exact call-site basis
- `InlayHintIntent` with authored anchor and optional target/edit intent refs
- `PresentationPolicy` keyed by profile/capability/configuration epoch

## Exact predecessor contracts

- **LSO0:** exact current receipt ID and digest for “Authored-coordinate semantic operation constitution”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **LSO2:** exact current receipt ID and digest for “Canonical authored target and provenance graph”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **H2:** exact current receipt ID and digest for “Project-scoped ProviderHub bindings”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **TCM4:** exact current receipt ID and digest for “Atomic activation and deletion”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **PUB0:** exact current receipt ID and digest for “Versioned public request/result and capability truth”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

### Binding architecture

- Fragments carry semantic kind/authority/provenance; formatting and Markdown are edge concerns.
- Provider display text, native resolved types, source-literal annotations, framework labels, and docs are distinct fragments and may coexist only under explicit policy.
- A native result cannot be silently discarded by a provider early return, nor can two authorities present contradictory “sole type” blocks without declared composition.
- Signature active parameter/index is derived from exact call/context basis and validated against provider/native signature identity.
- Inlay hints are semantic intents at authored anchors and cannot carry generated positions or direct edits.
- Provider absence/staleness degrades only provider-owned fragments and updates completeness/capability truth.
- Helper/synthetic names are excluded by structured provenance, not arbitrary string stripping in core semantics.

### Internal subblocks

#### LSO7-SB1 - Presentation subject classification

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

#### LSO7-SB2 - Fragment authority and composition policy

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

#### LSO7-SB3 - Hover semantic assembly

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

#### LSO7-SB4 - Signature help assembly

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

#### LSO7-SB5 - Inlay hint intents and resolution

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

#### LSO7-SB6 - Rendering adapters, caching, and bounded work

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

### Identity, invalidation, and publication

- Core operation identity is rooted in project/profile/source basis, semantic subject, operation kind, demand, and capability epoch; presentation encoding is not semantic identity.
- Generated coordinates, provider handles, and editor DTOs may exist only inside typed adapters and never become public semantic truth.
- Cancellation, stale, superseded, partial, NeedInputs, unsupported, and ambiguous outcomes remain distinct and are never collapsed to empty success.
- All returned targets and edits carry exact authored provenance and validate the snapshot/mapping chain used to derive them.
- Presentation fragment identity is semantic kind/subject/authority/basis, not rendered Markdown text.
- Source-literal annotations are explicitly distinguished from resolved types.
- Provider text is observation data and cannot enter native semantic cache identity.

### Migration and cutover

- Introduce fragment model behind current hover, then migrate signature and inlay.
- Characterize provider on/off and framework child-hover behavior.
- Delete feature-local merge/early-return/render logic after conformance.

### Consumers and unlocks

- Feeds LSO9 presentation conformance and thin editor adapters.
- Reuses LSO2 targets and PUB0 public contracts.
- Provides stable presentation substrate for future frameworks.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **LSO7-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or reproduced defect that existing evidence does not discriminate.
- **LSO7-AC2 — positive contract:** the named API/data boundary preserves exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **LSO7-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm; otherwise bind a terse not-applicable rationale.
- **LSO7-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, provider, filesystem, network, allocation, copy, or retained-candidate work; otherwise bind a terse not-applicable rationale.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; do not add implementation mirrors, duplicate permutations, or universal test quotas.
- Test homes: `crates/verter_session/tests/cases`, `crates/verter_protocol/tests`, `packages/typescript-plugin/src`, and the exact generated vertical fixture selected by this node.


### Pack-specific proof obligations

- **LSO7-AC-FRAGMENTS:** every fragment has exact kind/authority/provenance and deterministic policy.
- **LSO7-AC-HOVER:** provider/native/framework/source fragments compose without hidden early returns.
- **LSO7-AC-SIGNATURE:** canonical signatures/active parameter are provider-order independent.
- **LSO7-AC-INLAY:** hints use authored anchors, stable IDs, truthful capabilities, and zero work when disabled.
- **LSO7-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **LSO7-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **LSO7-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **LSO7-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Deletions and forbidden designs

- Delete provider-baked hover/signature/inlay core DTOs and implicit merge precedence.
- Delete early-return paths that bypass shared composition.
- Delete core string hacks used to infer semantic provenance.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

- Rendered Markdown/text as semantic result identity.
- Provider response order deciding fragment authority.
- Generated helper names exposed as semantic subjects.
- Direct TextEdits/commands without typed intents.
- Whole-workspace/provider work for a leaf presentation request.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

- Delete or structurally reject every compatibility path that would preserve a second owner after cutover.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 800 production LOC, 8 production files, 2 related crates/packages.
- Mandatory rescope above 1500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, identity aliasing, or unauthorized executable work.
- Performance budget: when preflight identifies touched authority or a hot path, use the ratified replacement SLO and equivalent-work counters below; otherwise performance evidence is not applicable and no soak is invented solely to populate evidence.

- Fragment assembly is proportional to demanded presentation and does not materialize unrelated public type graphs.
- Target ceiling: 800 production LOC, 8 production files, and 2 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Abort conditions

- Abort if a semantic distinction can only be represented by formatted provider text.
- Abort if provider/native contradictory authority cannot be settled by explicit policy.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. `node docs/arch/refactor/rev11/tools/validate-program-dag.mjs --strict`
2. Run every final command in the bound `targeted-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

### Pack-specific verification inventory

1. Fragment policy mutation tests and cross-renderer snapshots.
1. Hover/signature/inlay provider/profile/recovery/coexistence fixtures.
1. Stale provider/cancel/cache/allocation/memory and zero-work tests.

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

No clause targets this file directly. Applicable contract clauses are selected by the validated `applicable_nodes` ledger and embedded verbatim in cold packets.
