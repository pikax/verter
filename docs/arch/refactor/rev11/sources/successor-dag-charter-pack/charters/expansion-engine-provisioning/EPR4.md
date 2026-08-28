<!-- unified-charter-v2
id=EPR4
name=Exact authorized engine candidate resolution and selection
phase=expansion
train=expansion.engine-provisioning
product=engine_provisioning
kind=implementation
semantic_role=delivery
class=successor
predecessors=EPR1,H2
conditional_predecessors=EPR2:when-opened,EPR3:when-opened
owner=expansion.engine-provisioning:explicit policy-controlled engine acquisition, resolution, and activation authority
conflict_domains=provider_lifecycle,performance_evidence,source_lineage
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
charter=charters/expansion-engine-provisioning/EPR4.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# EPR4 - Exact authorized engine candidate resolution and selection

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

The internal subblocks below are binding decomposition and review checkpoints. They do not receive independent dispatch, leases, receipts, or deletion ownership unless the pre-scope architect proves that one subblock is independently acceptable or the block crosses a mandatory rescope trigger. In that case, amend the DAG before production mutation rather than treating a train-sized subblock as an implementation checklist.

## Independently acceptable outcome

Implement the exact authorized engine candidate resolver and deterministic selection plan. It enumerates only EPR0-authorized source adapters, converts every found locator into an EPR1-validated descriptor, records every rejection, ranks compatible candidates by explicit policy, and returns one selection or a typed no-selection report. It does not spawn or activate the engine.

The current owner is **tier-ordered path enumeration, source-specific validation/fallback, mixed discovery and spawn logic, cache scans, and incomplete status reporting**. The final and sole owner is **one EngineResolver with authorized source adapters, normalized validated candidates, deterministic comparator, complete rejection evidence, and warm bounded/zero-network behavior**.

## Architectural role and end state

EPR4 separates “which executable should be used” from acquisition and activation. It keeps source policy, validation, selection, and operational health honest and independently testable.

## Expected production surfaces

- `crates/verter_tsgo_api::toolchain` source adapters and resolution coordinator
- `crates/verter_type_runtime`/session ProviderHub request boundary
- `crates/verter_identity` resolver/candidate/plan identities
- `crates/verter_protocol` status/selection report projections
- performance/audit counters and hermetic filesystem fixtures

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.

## Named APIs and data boundaries

- `EngineResolutionRequest`, `EngineRequirement`, and `EngineResolutionBasis`
- `EngineSourceAdapter` closed registration/capability contract
- `EngineCandidate`, `ValidatedEngineCandidate`, and `EngineCandidateRejection`
- `EngineSelectionPolicy`, `EngineCandidateComparator`, and `EngineSelectionPlan`
- `EngineResolutionReport { selected, considered, rejected, outcome, basis }`
- `EngineResolverSnapshot`, `EngineResolutionEpoch`, and complete-only cache admission

## Exact predecessor contracts

- **EPR1:** consume exact candidate artifact/compatibility/origin/integrity/cache validation.
- **H2:** consume project-scoped ProviderHub requirements and provider binding lifecycle.
- **EPR2:when-opened:** when opened, enumerate only already installed verified managed entries; resolution itself performs no network.
- **EPR3:when-opened:** when opened, enumerate exact authorized bundled relative-path candidates; when unopened prove zero bundle work.

External custody: none beyond the package activation boundary.

## Binding architecture

- Resolution enumerates only source classes authorized by the captured EPR0 policy for the request context.
- Source adapters discover locators and source evidence; EPR1 validates and normalizes before comparison.
- Every candidate rejection is retained with typed reason; integrity/trust/revocation failure is not silently downgraded to absence.
- Selection comparator is explicit, deterministic, versioned, and independent of filesystem enumeration order.
- No source adapter spawns, executes, downloads, updates, or mutates a project/system/editor artifact.
- Managed/download and bundled channels are optional inputs only when opened and authorized; normal resolution never triggers EPR2 network work.
- Warm resolution uses exact source/policy/filesystem/provider requirement facts and performs zero repeated broad scans/hashes/network.
- A no-selection outcome distinguishes forbidden, offline, not-found, rejected, incompatible, and cancelled states and provides exact remediation.

## Internal subblocks

### EPR4-SB1 - Resolution request and requirement model

**Independently testable outcome:** Every resolution names exact project/profile/engine flavor/features/platform/policy and cancellation/budget basis.

**Architecture:**

- Define requirement axes and request identity.
- Separate mandatory/optional engine needs and feature capability requirements.
- Bind captured policy/provider/project epochs.

**Expected changes:**

- Add resolver request and result schemas.
- Replace global/default discovery calls with project-scoped requests.

**Discriminating proof:**

- Different project/profile/feature/policy requirements never warm-hit each other.
- Missing ambiguous project context yields NeedInputs.

### EPR4-SB2 - Authorized source adapter registry

**Independently testable outcome:** Only declared source adapters execute, in policy-defined selection groups with exact zero-work for disabled sources.

**Architecture:**

- Define adapters for explicit override/manual, project-local, editor-shared, system/PATH, managed cache, and bundle.
- Expose bounded enumeration and source snapshot/read sets.
- Keep adapter output as locators/evidence, not trusted candidates.

**Expected changes:**

- Centralize registration and remove hard-coded tier chain.
- Generate source/capability matrix from EPR0.

**Discriminating proof:**

- Disabled/unopened adapters are never called.
- Planting an unregistered adapter or hidden source path fails.

### EPR4-SB3 - Candidate discovery and validation composition

**Independently testable outcome:** Every discovered locator is normalized and validated under EPR1 before selection.

**Architecture:**

- Canonicalize paths/layout without following forbidden links.
- Capture mutable source stat/manifest/read-set basis.
- Run compatibility/integrity/origin/revocation validation and retain rejection.

**Expected changes:**

- Implement concurrent/bounded validation under scheduler pools where beneficial.
- Delete source-specific trust shortcuts.

**Discriminating proof:**

- Malicious/invalid first candidate cannot suppress reporting or become not-found fallback.
- Validation order permutations produce same candidate/rejection set.

### EPR4-SB4 - Deterministic selection comparator

**Independently testable outcome:** One exact comparator selects the best compatible authorized candidate without hidden source or latest bias.

**Architecture:**

- Define source preference, explicit override law, project pin/locality, editor sharing, version stability, compatibility, policy update channel, and tie-breakers.
- Version comparator and emit explanation trace.
- Preserve explicit override failures rather than silently ignoring them when policy says strict.

**Expected changes:**

- Implement pure comparator and property/mutation matrix.
- Remove first-found/tier-return behavior.

**Discriminating proof:**

- Enumeration/order permutation yields same selection.
- Every comparator dimension has discriminating positive/negative fixture.

### EPR4-SB5 - Resolution report and truthful remediation

**Independently testable outcome:** Selected and rejected candidates are fully explainable and no-selection outcomes are actionable without leaking secrets.

**Architecture:**

- Return selected descriptor/receipt ref and ordered considered/rejected summaries.
- Distinguish Forbidden, Offline, NeedInputs, NotFound, Incompatible, Trust/Integrity failure, Revoked, Cancelled.
- Expose safe public status versus private audit detail.

**Expected changes:**

- Add PUB0/status adapters and logs.
- Delete vague “no provider” fallback reports.

**Discriminating proof:**

- Outcome/status mutation tests preserve exact reason/remediation.
- Explicit corrupt override is reported loudly according to policy.

### EPR4-SB6 - Resolution cache, invalidation, and zero-work

**Independently testable outcome:** Repeated resolution reuses exact validated source snapshots while changes invalidate only affected adapters/candidates.

**Architecture:**

- Cache by full request/policy/source snapshot/revocation/compatibility basis.
- Use source-specific watchers/epochs or bounded stat facts.
- Never cache incomplete/budget/cancelled negative as complete.

**Expected changes:**

- Implement project-scoped resolver snapshots/singleflight and counters.
- Release caches on project/policy/provider teardown.

**Discriminating proof:**

- Warm unchanged request performs zero network, zero broad scan, and ratified minimal stat/hash work.
- Adding/removing/replacing candidates or policy/revocation change invalidates exactly.

### EPR4-SB7 - Resolver security and adversarial filesystem proof

**Independently testable outcome:** Resolution remains safe under symlink/reparse/permission/path races and hostile project/cache layouts.

**Architecture:**

- Use no-follow/open-by-handle patterns where required by EPR1.
- Bound directory/file enumeration and path lengths.
- Detect TOCTOU between validation receipt and selection handoff.

**Expected changes:**

- Add adversarial cross-platform fixture harness.
- Pass immutable descriptor/validation receipt to EPR5, not only path.

**Discriminating proof:**

- Path substitution/race/symlink/permission mutations cannot produce a selected executable.
- Large hostile trees remain bounded and cancellable.

## Data, identity, invalidation, and publication laws

- Engine acquisition, resolution, selection, and activation are distinct authorities with distinct receipts; no stage infers success from a later stage.
- No executable is run before exact origin, compatibility, integrity, and trust validation succeeds.
- Unavailable, unauthorized, incompatible, corrupt, offline, cancelled, and operationally failed outcomes remain distinct and capability-visible.
- All caches/installations are content/version/platform scoped, race-safe, and fail closed on symlink/reparse/permission/ownership violations.
- Selection plan is pure data and does not imply process health or activation.
- An explicit strict override rejection may block fallback according to policy; fallback behavior is never hard-coded in adapters.
- Resolution cache admission requires complete enumeration of every policy-applicable source group needed for the decision.

## Migration and cutover

- Wrap current source discovery as adapters under the new resolver in current policy order.
- Characterize selections/rejections and make intentional trust/correctness changes explicit.
- Add optional managed/bundle adapters only when opened; delete tier chain after parity.

## Deletions

- Delete hard-coded numeric tier/first-return discovery and mixed discovery-spawn logic.
- Delete source-specific silent fallback after integrity/trust failure.
- Delete broad recursive cache/project scans and process-global selection caches.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

## Forbidden designs

- Download/update/spawn inside resolution.
- First-found or filesystem-order selection.
- Path/version-only unvalidated candidates.
- Collapsing rejected candidate to not-found.
- Warm cache without exact source/policy/revocation basis.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

## Acceptance IDs and discriminating proof

- **EPR4-AC-SOURCES:** only authorized/opened adapters execute and disabled sources prove zero work.
- **EPR4-AC-CANDIDATES:** every selected candidate has current EPR1 validation and every rejection remains typed.
- **EPR4-AC-COMPARATOR:** enumeration permutations and dimension mutation matrix yield exact deterministic selection.
- **EPR4-AC-NO-NETWORK:** resolution never performs network acquisition/update.
- **EPR4-AC-CACHE:** incremental/warm resolution equals fresh and invalidates exactly under source/policy/revocation changes.
- **EPR4-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **EPR4-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **EPR4-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **EPR4-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Performance and bounded work

- Resolution work is bounded per authorized adapter; warm unchanged resolution performs zero broad scans/hashes/network and plateaus in memory.
- Target ceiling: 800 production LOC, 8 production files, and 2 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Mandatory rescope and abort conditions

- Abort if a source adapter cannot provide complete bounded enumeration/read-set facts required for a selection claim.
- Abort if path substitution between validation and activation cannot be prevented/detected.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. Source authorization/zero-work and adapter registry guards.
1. Comparator permutation/mutation and complete rejection/outcome matrix.
1. Adversarial filesystem/TOCTOU/symlink/reparse/permission/cancel/cache/invalidation/performance tests.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Consumers and unlocks

- Supplies exact selection plans/validated artifacts to EPR5.
- Feeds public engine status and CLI diagnostics without spawning.
- Makes optional acquisition/bundle sources composable without changing activation.

## Source reconciliation

- Current toolchain discovery policy/code and EPR0-EPR3 contracts.
- ProviderHub/TCM engine requirement identities.

Durable clauses are transferred as digest-bound requirement atoms. Historical path archaeology, obsolete branches, and implementation journals are not copied into the charter. Git history remains the archive.
