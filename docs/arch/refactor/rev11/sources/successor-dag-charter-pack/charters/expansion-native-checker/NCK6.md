<!-- unified-charter-v2
id=NCK6
name=Family-scoped diagnostic authority arbitration and atomic publication
phase=expansion
train=expansion.native-checker
product=native_checker
kind=cutover
semantic_role=delivery
class=successor
predecessors=NCK4,NCK5,H2,H3,COX0,PUB0
conditional_predecessors=
owner=expansion.native-checker:sole native semantic diagnostic authority and certified-family cutover
conflict_domains=diagnostic_action_service,provider_lifecycle,lsp_publication,public_protocol
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
charter=charters/expansion-native-checker/NCK6.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCK6 - Family-scoped diagnostic authority arbitration and atomic publication

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

The internal subblocks below are binding decomposition and review checkpoints. They do not receive independent dispatch, leases, receipts, or deletion ownership unless the pre-scope architect proves that one subblock is independently acceptable or the block crosses a mandatory rescope trigger. In that case, amend the DAG before production mutation rather than treating a train-sized subblock as an implementation checklist.

## Independently acceptable outcome

Implement the sole family-scoped diagnostic authority registry and atomic publication decision layer: exact External/ObserveNative/CertifiedNative/Disabled state, non-publishing shadow comparison, deterministic deduplication, provider/native epoch coordination, and rollback to an explicit prior certified receipt. This block does not integrate individual consumer surfaces.

The current owner is **provider-specific LSP merge branches, ad hoc suppression rules, global provider-enabled flags, and diagnostic message-text deduplication**. The final and sole owner is **one immutable DiagnosticAuthoritySnapshot and one atomic diagnostic publication decision for every project profile, family, and semantic feature slice**.

## Architectural role and end state

NCK6 is the authority cutover block. It prevents a green native implementation from becoming user-visible before certification and prevents external and native producers from publishing the same semantic family. It deliberately stops before LSP/CLI/MCP/NAPI/WASM adapters, which are owned by NCK7.

## Expected production surfaces

- `crates/verter_diagnostics` for authority registry, comparison, deduplication, and publication plans
- `crates/verter_session` for project-scoped immutable authority snapshots and exact basis selection
- `crates/verter_type_runtime` for external observation inputs and provider epoch identity only
- `crates/verter_lsp` publication coordinator only at the shared publication-plan seam, not feature adapters
- `crates/verter_protocol` for authority/certification status exposed under PUB0

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.

## Named APIs and data boundaries

- `DiagnosticAuthorityKey { project_profile, family, feature_slice }`
- `DiagnosticAuthorityState::{External, ObserveNative, CertifiedNative, Disabled}`
- `DiagnosticAuthoritySnapshot`, `DiagnosticAuthorityEpoch`, and immutable transition receipts
- `DiagnosticObservationBatch`, `DiagnosticComparisonResult`, and typed mismatch classes
- `DiagnosticPublicationPlan` and `DiagnosticDedupKey`
- `DiagnosticPromotionRequest`, `DiagnosticPromotionReceipt`, and `DiagnosticRollbackReceipt`

## Exact predecessor contracts

- **NCK4:** consume generated family manifests, exact certification receipts, and canonical oracle comparison.
- **NCK5:** consume validated framework contribution/profile isolation so authority keys never alias across profiles.
- **H2:** consume project-scoped provider bindings and exact provider epochs.
- **H3:** consume latest-basis stale-safe publication and supersession behavior.
- **COX0:** consume per-profile capability participation and dynamic withdrawal.
- **PUB0:** consume typed public outcomes, capability truth, and schema epochs.

External custody: none beyond the package activation boundary.

## Binding architecture

- Authority is keyed by exact project profile, diagnostic family, and semantic feature slice; one global checker/provider boolean is forbidden.
- ObserveNative computes and compares native output but never contributes user-visible diagnostics, fixes, actions, counts, or success status.
- CertifiedNative becomes visible only in the same atomic state transition that suppresses external publication for the exact key.
- Deduplication is by semantic identity and authority, never normalized message text or approximate source range.
- Provider epoch, native implementation receipt, certification receipt, configuration epoch, and authored basis are all explicit transition inputs.
- Rollback names a prior accepted authority snapshot; implicit fallback to whichever provider is available is forbidden.
- A mixed-epoch, stale, cancelled, partial, or NeedInputs producer cannot publish as complete.

## Internal subblocks

### NCK6-SB1 - Immutable authority registry and transition validator

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

### NCK6-SB2 - Non-publishing shadow observation

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

### NCK6-SB3 - Semantic deduplication and composed publication plan

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

### NCK6-SB4 - Provider/native epoch coordination

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

### NCK6-SB5 - Promotion and rollback execution

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

### NCK6-SB6 - Authority observability and bounded counters

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

## Data, identity, invalidation, and publication laws

- Authority snapshots are immutable and project/profile scoped; no process-global mutable map is semantic truth.
- Observation results never enter public caches or consumer responses.
- Promotion invalidates displaced producer routes by exact key, not by broad provider shutdown.
- Publication ordering is deterministic after authority selection and semantic deduplication.
- Uncertified families remain externally owned and are reported honestly.

## Migration and cutover

- Introduce the registry in External state for every existing family, proving behavior identity before observation.
- Enable ObserveNative only for accepted NCF slices and compare without publication.
- Promote one canary slice, validate zero duplicates/gaps, then expand only through accepted receipts.
- Leave consumer adapters on the shared publication plan seam for NCK7 migration.

## Deletions

- Delete global checker/provider diagnostic booleans displaced by the exact authority registry.
- Delete message-text and approximate-range deduplication used as an authority substitute.
- Delete provider/native first-arrival merge arbitration for migrated diagnostic classes.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

## Forbidden designs

- Publishing ObserveNative results or fixes.
- Promoting an entire provider/project when only bounded families are certified.
- Implicit rollback to any available provider or stale authority snapshot.
- Consumer-specific authority decisions after the shared publication plan exists.
- Counting diagnostic equality as certification without identity/provenance/completeness comparison.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

## Acceptance IDs and discriminating proof

- **NCK6-AC-STATE:** exhaustive state-machine mutations reject illegal, stale, cross-profile, and receipt-less transitions.
- **NCK6-AC-SHADOW:** observation is user-invisible and detects planted semantic mismatches.
- **NCK6-AC-ATOMIC:** failure injection proves old-or-new atomic authority with no duplicate or missing publication.
- **NCK6-AC-ZERO-PROVIDER:** certified warm slices perform zero external diagnostic work.
- **NCK6-AC-DEDUP:** semantic dedup preserves distinct owners and removes only exact duplicate authority.
- **NCK6-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **NCK6-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **NCK6-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **NCK6-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Performance and bounded work

- External-only state adds no native semantic work; Disabled adds no producer work; ObserveNative cost is explicit and budgeted.
- Authority lookup is allocation-free after snapshot construction and does not scan all families for a leaf request.
- Target ceiling: 800 production LOC, 8 production files, and 2 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Mandatory rescope and abort conditions

- Abort if a producer cannot name exact family/slice/profile/basis identity.
- Abort if promotion cannot atomically suppress the displaced authority.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. Authority-state, epoch-race, observation-invisibility, semantic-dedup, promotion/rollback, and zero-provider-work suites.
1. Provider restart and concurrent edit failure injection under H2/H3 publication semantics.
1. Architecture guard proving consumer adapters cannot independently choose diagnostic authority.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Consumers and unlocks

- Unlocks NCK7 shared consumer integration.
- Supplies the exact diagnostic authority snapshot consumed by language-service conformance when NCK is opened.
- Provides truthful family maturity to COX0 and PUB0.

## Source reconciliation

- `docs/arch/native-checker.md` authority/cutover clauses.
- `docs/arch/provider-*` and diagnostic merge designs containing provider/native arbitration behavior.

Durable clauses are transferred as digest-bound requirement atoms. Historical path archaeology, obsolete branches, and implementation journals are not copied into the charter. Git history remains the archive.
