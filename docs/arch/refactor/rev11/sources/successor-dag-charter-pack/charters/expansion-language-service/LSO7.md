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
