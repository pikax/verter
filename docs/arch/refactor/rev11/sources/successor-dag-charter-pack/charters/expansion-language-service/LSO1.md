<!-- unified-charter-v2
id=LSO1
name=Tolerant carrier recovery and two-rail syntax/semantic diagnostics
phase=expansion
train=expansion.language-service
product=language_service
kind=implementation
semantic_role=delivery
class=successor
predecessors=LSO0,PAR0,EMB0,B2,LRA0
conditional_predecessors=
owner=expansion.language-service:one authored-coordinate semantic-operation and edit-transaction authority
conflict_domains=carrier_parser,mapping_geometry,diagnostic_action_service,lsp_publication
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
charter=charters/expansion-language-service/LSO1.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# LSO1 - Tolerant carrier recovery and two-rail syntax/semantic diagnostics

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

The internal subblocks below are binding decomposition and review checkpoints. They do not receive independent dispatch, leases, receipts, or deletion ownership unless the pre-scope architect proves that one subblock is independently acceptable or the block crosses a mandatory rescope trigger. In that case, amend the DAG before production mutation rather than treating a train-sized subblock as an implementation checklist.

## Independently acceptable outcome

Implement tolerant Vue/Svelte carrier recovery and a two-rail diagnostic model that preserves stable authored tokens, regions, mappings, and semantic work during recoverable edits without inventing semantic facts or weakening strict mapping.

The current owner is **parser/compiler bailouts, aggressive generated-token repair, LSP diagnostic drop behavior, and carrier-specific recovery flags**. The final and sole owner is **one RecoverySnapshot contract with native syntax diagnostics, minimal capability-tagged synthetic repair, stable authored mappings, and exact per-region semantic participation**.

## Architectural role and end state

LSO1 makes broken carriers behave like broken source files: syntax errors remain visible while unaffected semantic diagnostics and operations continue. Recovery is a parser/lowering concern, not a resolver special case.

## Expected production surfaces

- `crates/verter_parser` and framework parser outputs
- `crates/verter_compiler` IDE projection/recovery chunks
- `crates/verter_session` native syntax diagnostic rail and recovery snapshot storage
- `crates/verter_lsp` only for consuming authored diagnostics/publication
- `crates/verter_span` and mapping contracts

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.

## Named APIs and data boundaries

- `RecoverySnapshot`, `RecoveryRegionState`, and `RecoveryParticipation`
- `NativeSyntaxDiagnostic` with authored spans and stable parser identity
- `SyntheticRepairChunk` with verification/navigation/completion capability flags
- `RecoveryBoundary`, `MissingNodeAnchor`, and exact source mapping metadata
- `RecoveredCarrierResult::{Usable, Degraded, Catastrophic}`

## Exact predecessor contracts

- **LSO0:** consume authored operation and typed outcome laws.
- **PAR0:** consume parser ownership and source lineage.
- **EMB0:** consume embedded-codec and exact authored map-chain contracts.
- **B2:** consume accepted framework parsing/recovery diagnostics and stable identities.
- **LRA0:** consume diagnostic provenance and suppression ownership.

External custody: none beyond the package activation boundary.

## Binding architecture

- Recoverable syntax diagnostics and IDE semantic surface availability are separate channels.
- Authored user tokens remain mapped and semantically visible whenever parser recovery can preserve them; synthetic repair text is unmapped and capability-tagged.
- Native syntax diagnostics are produced from parser errors in authored coordinates independently of provider output.
- Strict mapping remains strict; synthetic provider diagnostics are suppressed by explicit chunk metadata or dropped, never heuristically re-anchored.
- Recovery incompleteness causes fail-open usage analysis and ReturnOnly semantic results where completeness cannot be proven.
- Catastrophic failure is explicit and does not erase previously valid diagnostics without a typed stale/NeedInputs outcome.

## Internal subblocks

### LSO1-SB1 - Native syntax diagnostic rail

**Independently testable outcome:** Recoverable script/template/parser errors become stable authored diagnostics for all consumers.

**Architecture:**

- Harvest recoverable parser errors and convert extracted-region spans to carrier-authored spans.
- Assign parser/source/recovery identities and related anchors.
- Keep the rail independent of external provider availability.

**Expected changes:**

- Add parser-to-session diagnostic conversion for Vue and Svelte.
- Route the result through the shared diagnostic/publication service when NCK7 is available or existing native rail otherwise.

**Discriminating proof:**

- Broken script/template fixtures always show syntax diagnostics.
- Provider-off and provider-crash cases retain native syntax diagnostics.

### LSO1-SB2 - Surface-production versus diagnostic decoupling

**Independently testable outcome:** A recoverable error may publish a diagnostic while still producing a degraded but usable semantic projection.

**Architecture:**

- Define Usable/Degraded/Catastrophic outcomes and per-region participation.
- Stop using has_errors as a proxy for cannot-build-IDE-surface.
- Make catastrophic absence explicit and basis-scoped.

**Expected changes:**

- Refactor parse/compile/session result carriers.
- Remove early returns that publish an empty diagnostic set on recoverable failures.

**Discriminating proof:**

- A pre-existing type error survives an unrelated dangling expression.
- Only catastrophic fixtures refuse the semantic surface.

### LSO1-SB3 - Reference-preserving recovery chunks

**Independently testable outcome:** Recovery preserves authored identifier reads/writes and introduces only minimal synthetic structure.

**Architecture:**

- Prefer missing-node/boundary insertion over rewriting user identifiers into different expressions.
- Tag synthetic chunks with operation capabilities and suppression metadata.
- Preserve exact source ranges before and after inserted chunks.

**Expected changes:**

- Replace aggressive member/expression token rewrites where they alter liveness or navigation.
- Add structured recovery emit operations.

**Discriminating proof:**

- Identifier references/rename/hover around broken sites remain stable.
- Synthetic punctuation/helper positions map to None.

### LSO1-SB4 - Fail-open usage and synthetic diagnostic policy

**Independently testable outcome:** Incomplete recovery never creates spurious unused/copy diagnostics or drops legitimate source diagnostics.

**Architecture:**

- Treat unknown usage as used while recovery participated.
- Either emit bounded keep-alives or explicit synthetic-code suppression by code/chunk class.
- Forbid message-based suppression and source re-anchoring.

**Expected changes:**

- Centralize recovery-participation flags and synthetic suppression metadata.
- Remove local ad hoc unused-diagnostic workarounds.

**Discriminating proof:**

- TS6133-like diagnostics on synthetic destructures do not leak or erase source diagnostics.
- A genuine authored unused binding remains diagnosable when completeness is known.

### LSO1-SB5 - Best-effort template and embedded-region recovery

**Independently testable outcome:** One malformed template node does not invalidate unrelated regions or the entire carrier.

**Architecture:**

- Recover per node/region with stable missing-node anchors.
- Preserve framework profile and embedded-map chain.
- Mark unsupported/degraded operations per region rather than globally.

**Expected changes:**

- Implement Vue/Svelte parity fixtures and bounded recovery builders.
- Coordinate with EMB0/PAR0 rather than introducing a template-only parser authority.

**Discriminating proof:**

- Malformed branch/attribute/expression retains unaffected navigation and diagnostics.
- Incremental recovery equals fresh parse/recovery for the same broken source.

### LSO1-SB6 - Recovery capability and performance proof

**Independently testable outcome:** Recovery work is bounded, deterministic, and truthful to every operation capability.

**Architecture:**

- Count recovered nodes, synthetic chunks, mapping drops, parser passes, and semantic regions.
- Require one parse/shallow pass per content hash and no retry loops.
- Generate operation participation matrix for broken-state fixtures.

**Expected changes:**

- Add PER0 counters and VIM rows.
- Remove sleep/debounce timing assumptions from correctness tests.

**Discriminating proof:**

- Linear adversarial broken-input tests stay bounded.
- Warm unchanged broken files perform zero additional parse/recovery work.

## Data, identity, invalidation, and publication laws

- Core operation identity is rooted in project/profile/source basis, semantic subject, operation kind, demand, and capability epoch; presentation encoding is not semantic identity.
- Generated coordinates, provider handles, and editor DTOs may exist only inside typed adapters and never become public semantic truth.
- Cancellation, stale, superseded, partial, NeedInputs, unsupported, and ambiguous outcomes remain distinct and are never collapsed to empty success.
- All returned targets and edits carry exact authored provenance and validate the snapshot/mapping chain used to derive them.
- Recovery snapshots are content/profile/parser-version keyed and never reused across different broken text.
- Synthetic suppression metadata is structural and code-specific, never message text.
- A degraded region cannot warm-admit a complete semantic result.

## Migration and cutover

- Characterize current Vue/Svelte broken-source behavior and mapping drops.
- Land native syntax rail first, then decouple surface production, then replace reference-altering recovery.
- Delete old bailouts only after per-region parity and catastrophic cases are explicit.

## Deletions

- Delete recoverable-error empty-publication paths.
- Delete reference-altering recovery helpers displaced by structured chunks.
- Delete heuristic synthetic diagnostic re-anchoring or message suppression.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

## Forbidden designs

- Resolver special cases for broken syntax.
- Rewriting authored identifiers into semantically different expressions.
- Weakening strict mapping to keep diagnostics visible.
- Treating parser errors as a reason to clear all diagnostics.
- Repeated parse/recovery retries or hidden whole-file rechecks.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

## Acceptance IDs and discriminating proof

- **LSO1-AC-TWO-RAIL:** syntax plus surviving semantic diagnostics are both visible on broken carriers.
- **LSO1-AC-REFERENCE:** authored identifier occurrence sets remain stable across recoverable edits.
- **LSO1-AC-STRICT-MAP:** synthetic positions never acquire approximate authored ranges.
- **LSO1-AC-REGION:** unaffected regions retain exact operation capability and incremental/fresh equivalence.
- **LSO1-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **LSO1-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **LSO1-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **LSO1-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Performance and bounded work

- Recovery remains one bounded parse/lowering pass with linear work and no unbounded synthetic chunks.
- Target ceiling: 800 production LOC, 8 production files, and 2 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Mandatory rescope and abort conditions

- Abort if preserving current behavior requires semantic token fabrication.
- Abort if a proposed suppression cannot identify an explicit synthetic chunk and diagnostic class.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. Vue/Svelte broken script/template differential fixtures with provider on/off/crash.
1. Mapping round-trip, reference/liveness, strict-drop, cancellation, incremental/fresh, and adversarial linearity suites.
1. Architecture guard against has_errors-based recoverable surface bailout.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Consumers and unlocks

- Supplies tolerant input to LSO3-LSO8 and NCK diagnostics.
- Enables truthful editor behavior during active typing.
- Provides VIM broken-carrier conformance rows.

## Source reconciliation

- `docs/arch/ide-error-recovery-design.md`.
- B2/PAR0 recovery and diagnostic clauses.

Durable clauses are transferred as digest-bound requirement atoms. Historical path archaeology, obsolete branches, and implementation journals are not copied into the charter. Git history remains the archive.
