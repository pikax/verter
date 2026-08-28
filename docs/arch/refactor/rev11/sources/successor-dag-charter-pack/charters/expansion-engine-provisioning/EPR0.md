<!-- unified-charter-v2
id=EPR0
name=External engine provisioning policy and trust constitution
phase=expansion
train=expansion.engine-provisioning
product=engine_provisioning
kind=constitution
semantic_role=delivery
class=successor
predecessors=UAK1,CFG0,H2,PUB0,TCM4
conditional_predecessors=
owner=expansion.engine-provisioning:explicit policy-controlled engine acquisition, resolution, and activation authority
conflict_domains=provider_lifecycle,public_protocol,program_authority
resource_class=docs-light
review_profile=security-3
gate_profile=docs-domain
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
charter=charters/expansion-engine-provisioning/EPR0.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# EPR0 - External engine provisioning policy and trust constitution

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

The internal subblocks below are binding decomposition and review checkpoints. They do not receive independent dispatch, leases, receipts, or deletion ownership unless the pre-scope architect proves that one subblock is independently acceptable or the block crosses a mandatory rescope trigger. In that case, amend the DAG before production mutation rather than treating a train-sized subblock as an implementation checklist.

## Independently acceptable outcome

Ratify an explicit external-engine provisioning and trust constitution. The policy may authorize project-local, system, editor-shared, managed-download, bundled-sidecar, or no automatic acquisition. Network and bundled channels remain closed until separately authorized; missing engines produce typed outcomes rather than hidden fallback.

The current owner is **partially implemented discovery tiers, blocked future documents, release-package invariants, environment overrides, editor sharing, and implicit product assumptions**. The final and sole owner is **one captured EngineProvisioningPolicy with explicit source authorization, trust/update/offline/enterprise law, typed outcomes, and separate acquisition/resolution/activation authorities**.

## Architectural role and end state

EPR0 prevents implementation convenience from deciding security and product policy. It records whether Verter may download or bundle executable engines, which origins are trusted, how enterprise/offline environments behave, and what honest capability means when no engine is available.

## Expected production surfaces

- Rev11 authority/catalogs and declarative configuration under CFG0
- `crates/verter_identity` and `crates/verter_protocol` for policy/source/outcome identities
- `crates/verter_session`/ProviderHub future policy consumption
- `crates/verter_tsgo_api`/`crates/verter_type_runtime` as implementation consumers only
- release/packaging/editor products as conditionally authorized channels

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.

## Named APIs and data boundaries

- `EngineProvisioningPolicy`, `EngineSourcePolicy`, and `EngineSourceKind`
- `EngineAcquisitionPermission::{Forbidden, ManualOnly, Allowed}`
- `EngineUpdatePolicy`, `EngineOfflinePolicy`, `EngineProxyPolicy`, and `EngineRollbackPolicy`
- `EngineNeed`, `EngineRequirement`, and `EngineProvisioningOutcome`
- `TrustedEngineOrigin`, `EngineTrustRootId`, and policy epoch
- `EngineCapabilityState` and truthful source/availability reporting

## Exact predecessor contracts

- **UAK1:** consume universal-tooling constitution and product split.
- **CFG0:** consume declarative captured ecosystem configuration and project/profile selection.
- **H2:** consume ProviderHub project-scoped binding lifecycle.
- **PUB0:** consume typed public outcomes and truthful capability vocabulary.
- **TCM4:** consume certified TypeScript engine binding and observation identity.

External custody: none beyond the package activation boundary.

## Binding architecture

- Automatic download and bundled distribution are product/security choices, not implementation defaults.
- A valid policy may forbid both EPR2 and EPR3 while still requiring deterministic manual/project/system/editor discovery.
- Source order and authorization are explicit data; environment overrides cannot silently bypass forbidden source classes.
- Network behavior is opt-in, observable, proxy/enterprise compatible, cancellable, and absent from ordinary resolution when disallowed.
- Bundled artifacts require explicit shipping ownership, license/provenance/size/update policy, and platform coverage.
- Missing engine yields NeedInputs/Unsupported/Unavailable according to context and never a fake “provider off but success” result.
- Native checker/language-service certified families may reduce engine demand, but do not change provisioning policy implicitly.

## Internal subblocks

### EPR0-SB1 - Engine source taxonomy and authorization matrix

**Independently testable outcome:** Every possible engine source has an exact authorization and trust owner.

**Architecture:**

- Define environment/manual, project-local, editor-shared, system/PATH, managed cache/download, and bundled sources.
- Separate discovery visibility from authorization to select/execute.
- Bind source policy by project/profile/enterprise context.

**Expected changes:**

- Add machine-readable source policy catalog and generated order table.
- Classify existing discovery tiers and release invariants.

**Discriminating proof:**

- Unclassified source or unauthorized source selection fails.
- Policy reorder changes exact epoch/selection expectations deterministically.

### EPR0-SB2 - Network and managed acquisition policy

**Independently testable outcome:** The constitution states whether network acquisition is forbidden, manual, or allowed and under what origin/TLS/proxy rules.

**Architecture:**

- Define allowed registries/origins/version channels/trust roots.
- Define proxy/custom CA/offline/air-gap/telemetry behavior.
- Require explicit first-use/update user/admin policy where applicable.

**Expected changes:**

- Register the external requirement consumed by optional EPR2.
- Record dependency and security review obligations.

**Discriminating proof:**

- With acquisition forbidden, no network code path is reachable and zero network work is proven.
- An unapproved origin or TLS bypass is structurally rejected.

### EPR0-SB3 - Bundled distribution policy

**Independently testable outcome:** Bundling is either explicitly forbidden or owned by named release artifacts with exact platform/license/update rules.

**Architecture:**

- Define which package/VSIX/platform artifact may contain an engine.
- Define size, SBOM, license notice, signature/provenance, update and rollback policy.
- Reconcile existing “tsgo never packaged” guards deliberately.

**Expected changes:**

- Register external requirement for EPR3 and release owner.
- Classify current packaging channels and invariants.

**Discriminating proof:**

- Unopened bundle channel remains absent and zero-work.
- A bundled artifact in an unauthorized package fails release validation.

### EPR0-SB4 - Offline, enterprise, and privacy behavior

**Independently testable outcome:** Air-gapped/corporate/proxy environments receive deterministic no-network behavior and actionable typed status.

**Architecture:**

- Define offline-first and deny-network modes.
- Define proxy/custom trust configuration without weakening verification.
- Prohibit telemetry or registry calls unrelated to explicit acquisition.

**Expected changes:**

- Add public configuration/status fields under CFG0/PUB0.
- Define support/escalation diagnostics without exposing secrets.

**Discriminating proof:**

- Offline/deny-network fixtures make no DNS/socket attempts.
- Proxy/auth secrets never enter logs, cache identity, or public result payloads.

### EPR0-SB5 - Version/update/rollback constitution

**Independently testable outcome:** Engine compatibility, update, pinning, downgrade, and rollback are explicit and reproducible.

**Architecture:**

- Define supported ranges and channel/pin precedence.
- Separate policy update from automatic artifact replacement.
- Require retained prior known-good receipt or explicit no-rollback policy.

**Expected changes:**

- Add policy schema/version and migration law.
- Define revocation/emergency disable path.

**Discriminating proof:**

- Policy changes invalidate resolution/activation exactly.
- Rollback never selects an unverified or incompatible artifact.

### EPR0-SB6 - Typed outcomes and capability truth

**Independently testable outcome:** Every provisioning state reports exact reason/source/actionability without collapsing failures.

**Architecture:**

- Define Available, NeedInputs, Forbidden, Offline, NotFound, Incompatible, IntegrityFailure, TrustFailure, Cancelled, and OperationalFailure.
- Separate no candidate from candidate rejected and candidate activation failed.
- Map states to honest capabilities and user/admin remediation.

**Expected changes:**

- Amend PUB0/COX0/provider status contracts.
- Delete vague “auto/off/no provider” ambiguity from authority.

**Discriminating proof:**

- Outcome serialization/mutations preserve distinctions.
- Capabilities are unavailable until selection and activation receipts exist.

### EPR0-SB7 - Legacy decision/source reconciliation

**Independently testable outcome:** Blocked provisioning documents become exact policy choices and DAG nodes, then are deleted.

**Architecture:**

- Transfer download/bundle tier facts and unresolved decisions into EPR charters/source atoms.
- Bind existing discovery/packaging guards and source SHAs.
- Record rejected/deferred channels as policy, not orphan future docs.

**Expected changes:**

- Populate legacy disposition and external requirement catalogs.
- Name security guards for no unverified execution/no hidden network.

**Discriminating proof:**

- Legacy deletion fails until every durable clause/decision has a target.
- A hidden network or package bypass mutation fails architecture/release guards.

## Data, identity, invalidation, and publication laws

- Engine acquisition, resolution, selection, and activation are distinct authorities with distinct receipts; no stage infers success from a later stage.
- No executable is run before exact origin, compatibility, integrity, and trust validation succeeds.
- Unavailable, unauthorized, incompatible, corrupt, offline, cancelled, and operationally failed outcomes remain distinct and capability-visible.
- All caches/installations are content/version/platform scoped, race-safe, and fail closed on symlink/reparse/permission/ownership violations.
- Policy snapshots are immutable, captured, and part of resolution identity.
- Secrets/credentials are runtime inputs and never stored in policy receipts or logs.
- A policy change cancels/supersedes in-flight acquisition/resolution/activation work.

## Migration and cutover

- Land policy with current usable sources represented exactly and network/bundle forbidden unless explicitly authorized.
- Do not implement EPR2/EPR3 in this block.
- Replace prose tier assumptions with generated source policy/status tables.

## Deletions

- Delete orphan blocked download/bundle architecture docs after source transfer.
- Delete undocumented tier ordering and implicit automatic acquisition claims.
- Delete conflicting packaging invariants only through explicit EPR0/EPR3 authorization, never incidentally.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

## Forbidden designs

- Silent network download, update, telemetry, or registry probe.
- Executing an unverified candidate because it exists on PATH/project/editor/cache.
- Treating bundle/download authorization as implied by prior tier numbering.
- Collapsing integrity/trust failure to not-found fallback.
- Secrets in logs, digests, receipts, or public DTOs.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

## Acceptance IDs and discriminating proof

- **EPR0-AC-POLICY:** every source/network/bundle/update/offline state has exact authorization and owner.
- **EPR0-AC-ZERO-NETWORK:** forbidden/manual-only policy proves zero automatic network attempts.
- **EPR0-AC-OUTCOMES:** all absence/rejection/failure states remain typed and capability-truthful.
- **EPR0-AC-LEGACY:** download/bundle docs and packaging invariants have complete source disposition.
- **EPR0-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **EPR0-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **EPR0-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **EPR0-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Performance and bounded work

- Policy evaluation and source filtering are allocation-free/bounded after snapshot construction and perform no filesystem/network work themselves.
- Target ceiling: 0 production LOC, 0 production files, and 0 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Mandatory rescope and abort conditions

- Abort if the maintainer decision on network or bundling is inferred rather than explicit.
- Abort if enterprise/offline behavior or executable trust root cannot be specified.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. Policy/source/external-requirement/source-coverage validation.
1. Negative no-network/no-bundle/no-unverified-execution architecture tests.
1. Typed outcome/capability/configuration schema and secret-redaction tests.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Consumers and unlocks

- Unlocks EPR1 and optional EPR2/EPR3.
- Provides exact policy consumed by EPR4 resolution and EPR5 activation.
- Allows a valid no-download/no-bundle end state.

## Source reconciliation

- `docs/arch/future/engine-provisioning-download-tier.md`.
- `docs/arch/future/engine-provisioning-bundled-sidecar-and-shipping-channel.md`.
- Current toolchain discovery/packaging guards and ProviderHub/TCM contracts.

Durable clauses are transferred as digest-bound requirement atoms. Historical path archaeology, obsolete branches, and implementation journals are not copied into the charter. Git history remains the archive.
