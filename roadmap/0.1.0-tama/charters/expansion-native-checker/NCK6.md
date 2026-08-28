<!-- unified-charter-v2
id=NCK6
name=Family-scoped diagnostic authority arbitration and atomic publication
predecessors=NCK4,NCK5,H2,H3,COX0,PUB0
phase=expansion
train=expansion.native-checker
product=native_checker
kind=cutover
semantic_role=delivery
class=successor
owner=expansion.native-checker:sole native semantic diagnostic authority and certified-family cutover
conflict_domains=diagnostic_action_service,provider_lifecycle,lsp_publication,public_protocol
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
charter=charters/expansion-native-checker/NCK6.md
size=M
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# NCK6 — Family-scoped diagnostic authority arbitration and atomic publication

Readiness is derived only from implemented-ledger rows for the node ancestors. Commit message, approximate timezone-bearing date, and optional PR are loose locator hints; the CLI performs no Git, GitHub, SHA, tree, ancestry, receipt, lease, or digest validation.

## Independently acceptable outcome

Implement the sole family-scoped diagnostic authority registry and atomic publication decision layer: exact External/ObserveNative/CertifiedNative/Disabled state, non-publishing shadow comparison, deterministic deduplication, provider/native epoch coordination, and rollback to an explicit prior certified receipt. This block does not integrate individual consumer surfaces.

The current owner is **provider-specific LSP merge branches, ad hoc suppression rules, global provider-enabled flags, and diagnostic message-text deduplication**. The final and sole owner is **one immutable DiagnosticAuthoritySnapshot and one atomic diagnostic publication decision for every project profile, family, and semantic feature slice**.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_diagnostics`, `crates/verter_session`, `crates/verter_type_runtime`, `crates/verter_lsp`, `crates/verter_protocol`.
- Pack production inventory:
- `crates/verter_diagnostics` for authority registry, comparison, deduplication, and publication plans
- `crates/verter_session` for project-scoped immutable authority snapshots and exact basis selection
- `crates/verter_type_runtime` for external observation inputs and provider epoch identity only
- `crates/verter_lsp` publication coordinator only at the shared publication-plan seam, not feature adapters
- `crates/verter_protocol` for authority/certification status exposed under PUB0

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.
- Named API/data boundaries:
- `DiagnosticAuthorityKey { project_profile, family, feature_slice }`
- `DiagnosticAuthorityState::{External, ObserveNative, CertifiedNative, Disabled}`
- `DiagnosticAuthoritySnapshot`, `DiagnosticAuthorityEpoch`, and immutable transition receipts
- `DiagnosticObservationBatch`, `DiagnosticComparisonResult`, and typed mismatch classes
- `DiagnosticPublicationPlan` and `DiagnosticDedupKey`
- `DiagnosticPromotionRequest`, `DiagnosticPromotionReceipt`, and `DiagnosticRollbackReceipt`

## Exact predecessor contracts

- **NCK4:** implemented ledger row for “Diagnostic-family manifest, hermetic oracle, certification, and node generator”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **NCK5:** implemented ledger row for “Framework diagnostic contribution ingress and profile isolation”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **H2:** implemented ledger row for “Project-scoped ProviderHub bindings”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **H3:** implemented ledger row for “Atomic readiness and stale-safe publication”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **COX0:** implemented ledger row for “Per-profile editor participation and coexistence”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **PUB0:** implemented ledger row for “Versioned public request/result and capability truth”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

### Binding architecture

- Authority is keyed by exact project profile, diagnostic family, and semantic feature slice; one global checker/provider boolean is forbidden.
- ObserveNative computes and compares native output but never contributes user-visible diagnostics, fixes, actions, counts, or success status.
- CertifiedNative becomes visible only in the same atomic state transition that suppresses external publication for the exact key.
- Deduplication is by semantic identity and authority, never normalized message text or approximate source range.
- Provider epoch, native implementation receipt, certification receipt, configuration epoch, and authored basis are all explicit transition inputs.
- Rollback names a prior accepted authority snapshot; implicit fallback to whichever provider is available is forbidden.
- A mixed-epoch, stale, cancelled, partial, or NeedInputs producer cannot publish as complete.

### Internal subblocks

#### NCK6-SB1 - Immutable authority registry and transition validator

**Independently testable outcome:** Every diagnostic authority key has one exact state and only legal receipt-backed transitions are admitted.

**Architecture:**

- Implement immutable project-scoped authority snapshots with structural keys.
- Define legal transitions and required receipts for External to ObserveNative to CertifiedNative, disablement, and rollback.
- Make configuration/profile changes produce a new authority epoch rather than mutate state in place.

**Expected changes:**

- Replace global provider/native booleans and scattered suppression flags at the authority seam.
- Generate transition tables and static guards from NCK0 authority catalogs.

**Discriminating proof:**

- Illegal transitions, missing receipts, cross-profile reuse, and stale snapshot publication fail closed.
- Incremental reconstruction byte-equals a fresh snapshot for the same inputs.

#### NCK6-SB2 - Non-publishing shadow observation

**Independently testable outcome:** ObserveNative produces structured comparison evidence without changing user-visible behavior.

**Architecture:**

- Run native and external owners on the same exact input basis and canonicalize their diagnostic identities.
- Classify missing, extra, wrong-code, wrong-anchor, wrong-related-location, wrong-fix-intent, and completeness mismatches.
- Keep observation results bounded and non-admitted to ordinary diagnostic publication caches.

**Expected changes:**

- Add an observation scheduler lane with cancellation and budgets.
- Persist only bounded certification evidence or aggregate counters explicitly required by NCK4.

**Discriminating proof:**

- Observation on/off produces byte-identical user-visible diagnostics and actions.
- A planted native mismatch is detected while the external result remains the sole published result.

#### NCK6-SB3 - Semantic deduplication and composed publication plan

**Independently testable outcome:** The publication plan contains exactly one authoritative diagnostic per semantic identity and preserves distinct legitimate diagnostics.

**Architecture:**

- Construct semantic dedup keys from origin/family/rule/subject/authored anchor/profile/basis.
- Compose parser, semantic, framework, lint, project/configuration, and external classes under their own authority rules.
- Preserve separately owned diagnostics even when wording and ranges coincide.

**Expected changes:**

- Move deduplication out of consumer-specific merge code into the shared diagnostic authority layer.
- Emit a deterministic publication plan with provenance and completeness.

**Discriminating proof:**

- Message wording mutations do not change dedup identity.
- Two different rules at the same anchor survive; duplicate authorities for one key fail.

#### NCK6-SB4 - Provider/native epoch coordination

**Independently testable outcome:** A publication plan never combines provider and native results from incompatible bases or epochs.

**Architecture:**

- Join exact source revision, project profile, provider epoch, native authority epoch, and configuration epoch.
- Cancel or discard superseded comparison/publication work on any epoch transition.
- Require exact latest-basis settlement from H3 before publication.

**Expected changes:**

- Thread authority snapshot IDs through shared diagnostic production and publication receipts.
- Remove best-effort merge behavior that accepts whichever batch arrives first.

**Discriminating proof:**

- Race tests with provider restart, edit, config change, and promotion publish only the newest coherent basis.
- No mixed-epoch batch can serialize as complete.

#### NCK6-SB5 - Promotion and rollback execution

**Independently testable outcome:** Promotion and rollback are atomic, auditable, and leave neither duplicate nor missing authority.

**Architecture:**

- Validate certification, implementation, profile, provider, and source receipts immediately before transition.
- Publish the new authority snapshot and invalidate displaced result routes atomically.
- Rollback only to an explicitly named accepted snapshot with compatible inputs.

**Expected changes:**

- Implement transition receipts and negative guards against implicit fallback.
- Expose truthful capability/maturity status through PUB0/COX0.

**Discriminating proof:**

- Crash/failure injection at every transition point results in either old or new complete authority, never half-transition.
- Promotion immediately drives external diagnostic work for the certified key to zero.

#### NCK6-SB6 - Authority observability and bounded counters

**Independently testable outcome:** Operators and tests can prove which authority ran and how much equivalent work it performed without leaking provider internals into semantic APIs.

**Architecture:**

- Count native/external requests by family/slice/state, comparisons, discarded stale batches, promotions, rollbacks, and dedup decisions.
- Keep counters keyed by stable IDs and bounded cardinality.
- Separate certification/test telemetry from production result identity.

**Expected changes:**

- Add audit events and PER0-compatible work counters.
- Remove consumer-local diagnostic count heuristics used as authority evidence.

**Discriminating proof:**

- Certified warm requests show zero provider diagnostic work for that key.
- Counter reset/restart does not affect semantic or publication identity.

### Identity, invalidation, and publication

- Authority snapshots are immutable and project/profile scoped; no process-global mutable map is semantic truth.
- Observation results never enter public caches or consumer responses.
- Promotion invalidates displaced producer routes by exact key, not by broad provider shutdown.
- Publication ordering is deterministic after authority selection and semantic deduplication.
- Uncertified families remain externally owned and are reported honestly.

### Migration and cutover

- Introduce the registry in External state for every existing family, proving behavior identity before observation.
- Enable ObserveNative only for accepted NCF slices and compare without publication.
- Promote one canary slice, validate zero duplicates/gaps, then expand only through accepted receipts.
- Leave consumer adapters on the shared publication plan seam for NCK7 migration.

### Consumers and unlocks

- Unlocks NCK7 shared consumer integration.
- Supplies the exact diagnostic authority snapshot consumed by language-service conformance when NCK is opened.
- Provides truthful family maturity to COX0 and PUB0.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **NCK6-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or reproduced defect that existing evidence does not discriminate.
- **NCK6-AC2 — positive contract:** the named API/data boundary preserves exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **NCK6-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm; otherwise bind a terse not-applicable rationale.
- **NCK6-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, provider, filesystem, network, allocation, copy, or retained-candidate work; otherwise bind a terse not-applicable rationale.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; do not add implementation mirrors, duplicate permutations, or universal test quotas.
- Test homes: `crates/verter_session/tests/cases`, `crates/verter_protocol/tests`, `packages/typescript-plugin/src`, and the exact generated vertical fixture selected by this node.


### Pack-specific proof obligations

- **NCK6-AC-STATE:** exhaustive state-machine mutations reject illegal, stale, cross-profile, and receipt-less transitions.
- **NCK6-AC-SHADOW:** observation is user-invisible and detects planted semantic mismatches.
- **NCK6-AC-ATOMIC:** failure injection proves old-or-new atomic authority with no duplicate or missing publication.
- **NCK6-AC-ZERO-PROVIDER:** certified warm slices perform zero external diagnostic work.
- **NCK6-AC-DEDUP:** semantic dedup preserves distinct owners and removes only exact duplicate authority.
- **NCK6-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **NCK6-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **NCK6-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **NCK6-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Deletions and forbidden designs

- Delete global checker/provider diagnostic booleans displaced by the exact authority registry.
- Delete message-text and approximate-range deduplication used as an authority substitute.
- Delete provider/native first-arrival merge arbitration for migrated diagnostic classes.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

- Publishing ObserveNative results or fixes.
- Promoting an entire provider/project when only bounded families are certified.
- Implicit rollback to any available provider or stale authority snapshot.
- Consumer-specific authority decisions after the shared publication plan exists.
- Counting diagnostic equality as certification without identity/provenance/completeness comparison.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

- Delete or structurally reject every compatibility path that would preserve a second owner after cutover.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 800 production LOC, 8 production files, 2 related crates/packages.
- Mandatory rescope above 1500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, identity aliasing, or unauthorized executable work.
- Performance budget: when preflight identifies touched authority or a hot path, use the ratified replacement SLO and equivalent-work counters below; otherwise performance evidence is not applicable and no soak is invented solely to populate evidence.

- External-only state adds no native semantic work; Disabled adds no producer work; ObserveNative cost is explicit and budgeted.
- Authority lookup is allocation-free after snapshot construction and does not scan all families for a leaf request.
- Target ceiling: 800 production LOC, 8 production files, and 2 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Abort conditions

- Abort if a producer cannot name exact family/slice/profile/basis identity.
- Abort if promotion cannot atomically suppress the displaced authority.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. `node roadmap/0.1.0-tama/tools/validate-program-dag.mjs --strict`
2. Run every final command in the bound `targeted-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

### Pack-specific verification inventory

1. Authority-state, epoch-race, observation-invisibility, semantic-dedup, promotion/rollback, and zero-provider-work suites.
1. Provider restart and concurrent edit failure injection under H2/H3 publication semantics.
1. Architecture guard proving consumer adapters cannot independently choose diagnostic authority.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN evidence when applicable, the configured independent review profile, and the owning final gate on the squashed review candidate.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch adds one `[[implemented]]` row to `authority/state/implemented.toml` with the node ID, planned squash commit message, approximate date with timezone, and optional pull-request number. Row presence is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.
