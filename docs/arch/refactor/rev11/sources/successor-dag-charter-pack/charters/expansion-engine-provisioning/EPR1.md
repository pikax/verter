<!-- unified-charter-v2
id=EPR1
name=Engine artifact identity, compatibility, integrity, and cache contract
phase=expansion
train=expansion.engine-provisioning
product=engine_provisioning
kind=contract
semantic_role=delivery
class=successor
predecessors=EPR0,VID0
conditional_predecessors=
owner=expansion.engine-provisioning:explicit policy-controlled engine acquisition, resolution, and activation authority
conflict_domains=provider_lifecycle,source_lineage,program_authority
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
charter=charters/expansion-engine-provisioning/EPR1.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# EPR1 - Engine artifact identity, compatibility, integrity, and cache contract

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

The internal subblocks below are binding decomposition and review checkpoints. They do not receive independent dispatch, leases, receipts, or deletion ownership unless the pre-scope architect proves that one subblock is independently acceptable or the block crosses a mandatory rescope trigger. In that case, amend the DAG before production mutation rather than treating a train-sized subblock as an implementation checklist.

## Independently acceptable outcome

Define the exact identity, compatibility, origin, integrity, installation, cache, revocation, and validation contract for any executable engine candidate. This block is contract-only and applies uniformly to project/system/editor/download/bundled sources.

The current owner is **path/version probes, source-specific validation, bundle manifests, consume-only cache checks, package metadata, and ad hoc compatibility rules**. The final and sole owner is **one EngineArtifactDescriptor/ValidationReceipt law and one trusted cache/install layout consumed by every source adapter before selection or execution**.

## Architectural role and end state

EPR1 makes “found an engine path” insufficient. Every candidate must become an immutable artifact identity with origin and validation evidence, and every cache/install must be safe under concurrent writers, corruption, tampering, and revocation.

## Expected production surfaces

- `crates/verter_identity` for artifact/platform/origin/digest IDs
- `crates/verter_tsgo_api::toolchain` and `crates/verter_type_runtime` validation contracts
- `crates/verter_protocol` public status/provenance projections
- cache/install manifest schemas and release artifact metadata
- security/audit tests and revocation catalogs

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.

## Named APIs and data boundaries

- `EngineArtifactId`, `EnginePlatform`, `EngineFlavor`, and `EngineVersion`
- `EngineOrigin`, `EngineOriginReceipt`, and `EngineArtifactDescriptor`
- `EngineCompatibilityRequirement` and `EngineCompatibilityVerdict`
- `EngineIntegrityEvidence`, `EngineSignatureEvidence`, and `EngineValidationReceipt`
- `EngineInstallLayout`, `EngineReadyMarker`, `EngineCacheKey`, and `EngineCacheEntry`
- `EngineRejection`, `EngineRevocation`, and exact rejection reason codes

## Exact predecessor contracts

- **EPR0:** consume explicit source/trust/update/offline policy.
- **VID0:** consume orthogonal identities and exact-release law.

External custody: none beyond the package activation boundary.

## Binding architecture

- Artifact identity includes exact engine/version/flavor/platform/build/origin/content digest and policy-compatible metadata.
- Path is a locator, never identity; replacing bytes at one path invalidates validation.
- Compatibility is checked before execution and includes protocol/API/feature constraints, not version string alone.
- Integrity/signature/origin evidence is source-specific but normalized to one validation receipt.
- Cache/install entries are private, non-symlink/reparse, ownership/permission checked, immutable after READY, and atomically installed.
- READY is written last only after validation; incomplete/corrupt entries are never candidates.
- Revocation and policy epoch invalidate candidate/validation caches immediately.

## Internal subblocks

### EPR1-SB1 - Artifact and platform identity

**Independently testable outcome:** Every candidate has one collision-resistant structural identity independent of path.

**Architecture:**

- Define engine flavor/version/build/platform/ABI/protocol/content/origin components.
- Use full structural fields or content digest where digest is the artifact itself, not lossy replacement for semantic axes.
- Canonicalize platform triples and executable layout.

**Expected changes:**

- Add identity types and serialization/catalog schemas.
- Migrate source-specific version/path tuples.

**Discriminating proof:**

- Different bytes/build/origin/platform never alias.
- Same verified artifact reached through two locators canonicalizes appropriately while origin receipts remain distinct.

### EPR1-SB2 - Compatibility and feature contract

**Independently testable outcome:** An engine is selectable only when its exact API/protocol/features satisfy the requester.

**Architecture:**

- Define version ranges, protocol versions, command/API capabilities, project/toolchain compatibility.
- Separate compatible, unsupported, too-old/new, wrong-platform/flavor, and unknown.
- Bind compatibility policy version.

**Expected changes:**

- Centralize compatibility evaluator.
- Remove source adapter/version-string-only decisions.

**Discriminating proof:**

- Boundary/mutation matrix detects each incompatibility.
- Compatibility changes invalidate selection without reusing stale receipt.

### EPR1-SB3 - Origin, integrity, signature, and provenance evidence

**Independently testable outcome:** Validation proves what bytes were obtained, from which authorized channel, under which trust root.

**Architecture:**

- Normalize registry integrity, release checksum/signature/attestation, bundle manifest, manual/project/system evidence.
- Require digest over executed artifact and critical sidecar files.
- Record SBOM/license/provenance references where policy demands.

**Expected changes:**

- Implement receipt schemas and source adapter obligations.
- Ban self-asserted “trusted” booleans.

**Discriminating proof:**

- Byte mutation, origin substitution, signature/trust-root mismatch, and manifest omission fail.
- Receipt is deterministic and redacts secrets/local absolute roots where required.

### EPR1-SB4 - Safe cache/install layout and concurrent writers

**Independently testable outcome:** Install/cache entries cannot expose partial, mutable, symlinked, or attacker-controlled executables.

**Architecture:**

- Use private root, temp sibling, no-follow creation, ownership/permission checks, bounded extraction, atomic rename, READY-last.
- Define cross-process lock/loser cleanup and immutable versioned entries.
- Reject group/world-writable or reparse/symlink components.

**Expected changes:**

- Ratify layout consumed by EPR2/EPR3 and existing cache readers.
- Add corruption/quarantine cleanup policy.

**Discriminating proof:**

- Crash at every install step never yields a selectable partial entry.
- Concurrent installers converge to one verified entry without overwrite races.

### EPR1-SB5 - Validation cache and exact invalidation

**Independently testable outcome:** Expensive validation reuses receipts only while every artifact/origin/policy/revocation fact matches.

**Architecture:**

- Key by artifact locator stat identity/content evidence/origin/policy/revocation epoch.
- Revalidate mutable/manual/system/project locators as policy requires.
- Keep immutable downloaded/bundled entries fast after READY.

**Expected changes:**

- Implement bounded validation receipt cache and counters.
- Do not cache rejected unknowns across facts that could change.

**Discriminating proof:**

- Replace bytes/metadata/policy/revocation forces validation.
- Warm immutable validation performs zero rehash/stat beyond the ratified trust boundary.

### EPR1-SB6 - Revocation, corruption, and quarantine

**Independently testable outcome:** Known-bad or newly revoked artifacts are never selected and are handled without silent fallback.

**Architecture:**

- Define revocation catalog/epoch and emergency policy.
- Distinguish integrity failure from revocation/incompatibility/operational failure.
- Quarantine/remove only entries owned by managed channels; never mutate user project/system installs.

**Expected changes:**

- Add rejection/status/audit paths.
- Define retry/update/rollback interaction.

**Discriminating proof:**

- Revocation race cancels activation and invalidates caches.
- Managed corruption is quarantined; manual corruption is reported without destructive mutation.

### EPR1-SB7 - Public validation status and secret/path hygiene

**Independently testable outcome:** Users/operators receive actionable source/version/status without leaking secrets or unstable machine roots into portable receipts.

**Architecture:**

- Define public summary versus private diagnostic detail.
- Normalize/redact paths, proxy credentials, tokens, and trust material.
- Provide stable reason/action codes.

**Expected changes:**

- Amend PUB0 status schema and logs/audit.
- Add portability guards.

**Discriminating proof:**

- Golden tests contain no secrets/machine roots.
- Every rejection has stable typed reason and remediation class.

## Data, identity, invalidation, and publication laws

- Engine acquisition, resolution, selection, and activation are distinct authorities with distinct receipts; no stage infers success from a later stage.
- No executable is run before exact origin, compatibility, integrity, and trust validation succeeds.
- Unavailable, unauthorized, incompatible, corrupt, offline, cancelled, and operationally failed outcomes remain distinct and capability-visible.
- All caches/installations are content/version/platform scoped, race-safe, and fail closed on symlink/reparse/permission/ownership violations.
- Validation receipts are immutable and bind exact policy/trust/revocation epochs.
- Managed cache writer owns only managed roots; project/system/editor artifacts are read-only.
- A rejected candidate cannot be reclassified as not-found to continue fallback silently.

## Migration and cutover

- Characterize current source validators and bundle/cache manifests.
- Introduce normalized descriptor/receipt while preserving current source order under EPR0.
- Migrate all source adapters before EPR4 selection.

## Deletions

- Delete path/version-only candidate identity and duplicated compatibility/integrity decisions.
- Delete READY/manifest trust that does not bind executed bytes.
- Delete unsafe mutable/symlink-permissive cache paths.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

## Forbidden designs

- Path as artifact identity.
- Executing to discover compatibility before validation.
- Checksum/signature verification after installation/execution.
- Following symlinks/reparse points in managed install roots.
- Silent fallback after integrity/trust/revocation failure.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

## Acceptance IDs and discriminating proof

- **EPR1-AC-IDENTITY:** artifact identity collision/substitution matrix is exact.
- **EPR1-AC-COMPAT:** compatibility boundaries and feature requirements are mutation-tested.
- **EPR1-AC-INTEGRITY:** byte/origin/signature/manifest mutations fail before execution.
- **EPR1-AC-INSTALL:** crash/concurrency/symlink/permission tests never expose partial/untrusted entries.
- **EPR1-AC-REVOCATION:** revocation invalidates validation/selection/activation immediately.
- **EPR1-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **EPR1-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **EPR1-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **EPR1-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Performance and bounded work

- Immutable READY entries may use bounded receipt validation; mutable locators revalidate according to explicit policy without repeated full scans.
- Target ceiling: 0 production LOC, 0 production files, and 0 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Mandatory rescope and abort conditions

- Abort if any source cannot produce evidence sufficient for its authorized trust class.
- Abort if a managed install cannot be created with no-follow/atomic/private semantics on a supported platform.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. Artifact identity/compatibility/integrity/signature/origin mutation matrix.
1. Cross-platform cache layout, permission, symlink/reparse, crash, concurrency, quarantine, revocation tests.
1. Public status redaction/portability and warm validation work counters.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Consumers and unlocks

- Unlocks optional EPR2/EPR3 and required EPR4.
- Provides uniform validation receipts to selection/activation.
- Owns safe managed cache/install contract.

## Source reconciliation

- Existing toolchain discovery/bundle/cache validation code and future provisioning docs.
- VID0 exact-release and portability/security guards.

Durable clauses are transferred as digest-bound requirement atoms. Historical path archaeology, obsolete branches, and implementation journals are not copied into the charter. Git history remains the archive.
