<!-- unified-charter-v2
id=LSO1
name=Tolerant carrier recovery and two-rail syntax/semantic diagnostics
predecessors=LSO0,PAR0,EMB0,B2,LRA0
phase=expansion
train=expansion.language-service
product=language_service
kind=implementation
semantic_role=delivery
class=successor
owner=expansion.language-service:one authored-coordinate semantic-operation and edit-transaction authority
conflict_domains=carrier_parser,mapping_geometry,diagnostic_action_service,lsp_publication
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
charter=charters/expansion-language-service/LSO1.md
size=M
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# LSO1 — Tolerant carrier recovery and two-rail syntax/semantic diagnostics

Readiness is derived only from implemented-ledger rows for the node ancestors. Commit message, approximate timezone-bearing date, and optional PR are loose locator hints; the CLI performs no Git, GitHub, SHA, tree, ancestry, receipt, lease, or digest validation.

## Independently acceptable outcome

Implement tolerant Vue/Svelte carrier recovery and a two-rail diagnostic model that preserves stable authored tokens, regions, mappings, and semantic work during recoverable edits without inventing semantic facts or weakening strict mapping.

The current owner is **parser/compiler bailouts, aggressive generated-token repair, LSP diagnostic drop behavior, and carrier-specific recovery flags**. The final and sole owner is **one RecoverySnapshot contract with native syntax diagnostics, minimal capability-tagged synthetic repair, stable authored mappings, and exact per-region semantic participation**.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_parser`, `crates/verter_compiler`, `crates/verter_session`, `crates/verter_lsp`, `crates/verter_span`.
- Pack production inventory:
- `crates/verter_parser` and framework parser outputs
- `crates/verter_compiler` IDE projection/recovery chunks
- `crates/verter_session` native syntax diagnostic rail and recovery snapshot storage
- `crates/verter_lsp` only for consuming authored diagnostics/publication
- `crates/verter_span` and mapping contracts

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.
- Named API/data boundaries:
- `RecoverySnapshot`, `RecoveryRegionState`, and `RecoveryParticipation`
- `NativeSyntaxDiagnostic` with authored spans and stable parser identity
- `SyntheticRepairChunk` with verification/navigation/completion capability flags
- `RecoveryBoundary`, `MissingNodeAnchor`, and exact source mapping metadata
- `RecoveredCarrierResult::{Usable, Degraded, Catastrophic}`

## Exact predecessor contracts

- **LSO0:** implemented ledger row for “Authored-coordinate semantic operation constitution”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **PAR0:** implemented ledger row for “Parser decision, ownership, reuse, and lineage contract”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **EMB0:** implemented ledger row for “Embedded codecs and exact authored map chains”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **B2:** implemented ledger row for “Framework parsing recovery diagnostics and stable identities”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **LRA0:** implemented ledger row for “Profile-scoped diagnostics, lint, fixes, and actions”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

### Binding architecture

- Recoverable syntax diagnostics and IDE semantic surface availability are separate channels.
- Authored user tokens remain mapped and semantically visible whenever parser recovery can preserve them; synthetic repair text is unmapped and capability-tagged.
- Native syntax diagnostics are produced from parser errors in authored coordinates independently of provider output.
- Strict mapping remains strict; synthetic provider diagnostics are suppressed by explicit chunk metadata or dropped, never heuristically re-anchored.
- Recovery incompleteness causes fail-open usage analysis and ReturnOnly semantic results where completeness cannot be proven.
- Catastrophic failure is explicit and does not erase previously valid diagnostics without a typed stale/NeedInputs outcome.

### Internal subblocks

#### LSO1-SB1 - Native syntax diagnostic rail

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

#### LSO1-SB2 - Surface-production versus diagnostic decoupling

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

#### LSO1-SB3 - Reference-preserving recovery chunks

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

#### LSO1-SB4 - Fail-open usage and synthetic diagnostic policy

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

#### LSO1-SB5 - Best-effort template and embedded-region recovery

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

#### LSO1-SB6 - Recovery capability and performance proof

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

### Identity, invalidation, and publication

- Core operation identity is rooted in project/profile/source basis, semantic subject, operation kind, demand, and capability epoch; presentation encoding is not semantic identity.
- Generated coordinates, provider handles, and editor DTOs may exist only inside typed adapters and never become public semantic truth.
- Cancellation, stale, superseded, partial, NeedInputs, unsupported, and ambiguous outcomes remain distinct and are never collapsed to empty success.
- All returned targets and edits carry exact authored provenance and validate the snapshot/mapping chain used to derive them.
- Recovery snapshots are content/profile/parser-version keyed and never reused across different broken text.
- Synthetic suppression metadata is structural and code-specific, never message text.
- A degraded region cannot warm-admit a complete semantic result.

### Migration and cutover

- Characterize current Vue/Svelte broken-source behavior and mapping drops.
- Land native syntax rail first, then decouple surface production, then replace reference-altering recovery.
- Delete old bailouts only after per-region parity and catastrophic cases are explicit.

### Consumers and unlocks

- Supplies tolerant input to LSO3-LSO8 and NCK diagnostics.
- Enables truthful editor behavior during active typing.
- Provides VIM broken-carrier conformance rows.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **LSO1-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or reproduced defect that existing evidence does not discriminate.
- **LSO1-AC2 — positive contract:** the named API/data boundary preserves exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **LSO1-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm; otherwise bind a terse not-applicable rationale.
- **LSO1-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, provider, filesystem, network, allocation, copy, or retained-candidate work; otherwise bind a terse not-applicable rationale.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; do not add implementation mirrors, duplicate permutations, or universal test quotas.
- Test homes: `crates/verter_session/tests/cases`, `crates/verter_protocol/tests`, `packages/typescript-plugin/src`, and the exact generated vertical fixture selected by this node.


### Pack-specific proof obligations

- **LSO1-AC-TWO-RAIL:** syntax plus surviving semantic diagnostics are both visible on broken carriers.
- **LSO1-AC-REFERENCE:** authored identifier occurrence sets remain stable across recoverable edits.
- **LSO1-AC-STRICT-MAP:** synthetic positions never acquire approximate authored ranges.
- **LSO1-AC-REGION:** unaffected regions retain exact operation capability and incremental/fresh equivalence.
- **LSO1-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **LSO1-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **LSO1-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **LSO1-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Deletions and forbidden designs

- Delete recoverable-error empty-publication paths.
- Delete reference-altering recovery helpers displaced by structured chunks.
- Delete heuristic synthetic diagnostic re-anchoring or message suppression.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

- Resolver special cases for broken syntax.
- Rewriting authored identifiers into semantically different expressions.
- Weakening strict mapping to keep diagnostics visible.
- Treating parser errors as a reason to clear all diagnostics.
- Repeated parse/recovery retries or hidden whole-file rechecks.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

- Delete or structurally reject every compatibility path that would preserve a second owner after cutover.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 800 production LOC, 8 production files, 2 related crates/packages.
- Mandatory rescope above 1500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, identity aliasing, or unauthorized executable work.
- Performance budget: when preflight identifies touched authority or a hot path, use the ratified replacement SLO and equivalent-work counters below; otherwise performance evidence is not applicable and no soak is invented solely to populate evidence.

- Recovery remains one bounded parse/lowering pass with linear work and no unbounded synthetic chunks.
- Target ceiling: 800 production LOC, 8 production files, and 2 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Abort conditions

- Abort if preserving current behavior requires semantic token fabrication.
- Abort if a proposed suppression cannot identify an explicit synthetic chunk and diagnostic class.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. `node roadmap/0.1.0-tama/tools/validate-program-dag.mjs --strict`
2. Run every final command in the bound `targeted-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

### Pack-specific verification inventory

1. Vue/Svelte broken script/template differential fixtures with provider on/off/crash.
1. Mapping round-trip, reference/liveness, strict-drop, cancellation, incremental/fresh, and adversarial linearity suites.
1. Architecture guard against has_errors-based recoverable surface bailout.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN evidence when applicable, the configured independent review profile, and the owning final gate on the squashed review candidate.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch adds one `[[implemented]]` row to `authority/state/implemented.toml` with the node ID, planned squash commit message, approximate date with timezone, and optional pull-request number. Row presence is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.
