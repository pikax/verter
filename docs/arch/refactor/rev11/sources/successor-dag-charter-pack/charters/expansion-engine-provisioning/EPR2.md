<!-- unified-charter-v2
id=EPR2
name=Managed download and verified atomic installation channel
phase=expansion
train=expansion.engine-provisioning
product=engine_provisioning
kind=implementation
semantic_role=delivery
class=successor
predecessors=EPR1,G5
conditional_predecessors=
owner=expansion.engine-provisioning:explicit policy-controlled engine acquisition, resolution, and activation authority
conflict_domains=provider_lifecycle,scheduler_admission,source_lineage
resource_class=rust-mixed
review_profile=security-3
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
optional=true
release_gating=non_release
source_refs=live:docs/arch/refactor/rev11/sources/legacy-arch-reconciliation.md
external_requirements=maintainer_managed_engine_acquisition
activation_gate=ORC0
charter=charters/expansion-engine-provisioning/EPR2.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# EPR2 - Managed download and verified atomic installation channel

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

The internal subblocks below are binding decomposition and review checkpoints. They do not receive independent dispatch, leases, receipts, or deletion ownership unless the pre-scope architect proves that one subblock is independently acceptable or the block crosses a mandatory rescope trigger. In that case, amend the DAG before production mutation rather than treating a train-sized subblock as an implementation checklist.

## Independently acceptable outcome

Optionally implement managed network acquisition of an authorized engine artifact: resolve a policy-compatible release, download through approved HTTP/TLS/proxy infrastructure, verify integrity/signature before exposure, safely extract/install under EPR1, and publish an immutable acquisition receipt. The block remains closed unless `maintainer_managed_engine_acquisition` is present.

The current owner is **a consume-only cache reader, no HTTP/TLS writer, blocked download-tier prose, and implicit npm registry assumptions**. The final and sole owner is **one policy-gated EngineAcquirer and verified atomic installer with no hidden network behavior and exact origin/integrity receipts**.

## Architectural role and end state

EPR2 is deliberately optional because it adds network and executable supply-chain dependencies. It owns acquisition only; candidate ranking belongs to EPR4 and activation/execution belongs to EPR5.

## Expected production surfaces

- `crates/verter_tsgo_api::toolchain::acquire` or a narrower dedicated acquisition crate
- approved HTTP/TLS/archive dependencies and dependency-policy records
- managed cache/install root from EPR1
- registry/release metadata adapters
- audit/security/proxy/offline test harnesses

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.

## Named APIs and data boundaries

- `EngineAcquisitionRequest`, `EngineReleaseRequirement`, and `EngineAcquisitionPlan`
- `EngineReleaseIndex`, `EngineReleaseDescriptor`, and exact origin metadata
- `EngineDownload`, `EngineDownloadProgress`, and cancellation/deadline controls
- `VerifiedArchive`, `SafeExtractionPlan`, and `EngineAcquisitionReceipt`
- `AcquisitionFailure::{Forbidden, Offline, Proxy, TLS, Origin, Integrity, Archive, Install, Cancelled}`

## Exact predecessor contracts

- **EPR1:** consume exact artifact/origin/integrity/cache/install/validation contract.
- **G5:** consume bounded I/O/CPU execution pools, cancellation, and owner-affine commands.

External custody: maintainer_managed_engine_acquisition. Dispatch fails until the canonical authorization receipt exists.

## Binding architecture

- No acquisition code is reachable unless the captured EPR0 policy and external authorization allow managed download.
- Release metadata and artifacts come only from declared authorized origins; redirects are bounded and revalidated.
- TLS verification is never disabled; custom enterprise roots/proxy configuration are explicit runtime inputs.
- Integrity/signature is verified on downloaded bytes before extraction and again against installed executable evidence as required.
- Archive extraction is path-safe, bounded, no-follow, and restricted to the temporary private root.
- Installation follows EPR1 temp/lock/atomic rename/READY-last semantics.
- Acquisition receipt does not imply selection or activation; EPR4/EPR5 revalidate required facts.
- No telemetry, registry ping, or auto-update occurs outside an explicit acquisition request.

## Internal subblocks

### EPR2-SB1 - Dependency, threat, and policy review

**Independently testable outcome:** Network/TLS/archive dependencies and threat boundaries are explicitly accepted before code lands.

**Architecture:**

- Inventory dependency trees, platform support, proxy/custom CA behavior, archive formats, memory/CPU risks.
- Document SSRF/redirect/path traversal/decompression bomb/race/tamper threats.
- Bind approved origins/trust roots and dependency versions.

**Expected changes:**

- Add dependency policy amendment and security review receipt.
- Fail dispatch if authorization/review is missing.

**Discriminating proof:**

- Static dependency/license/advisory review passes.
- Negative configuration cannot disable TLS or authorize arbitrary origin.

### EPR2-SB2 - Release metadata and version resolution

**Independently testable outcome:** The acquirer selects a stable compatible release descriptor without executing or trusting unverified package code.

**Architecture:**

- Query approved metadata endpoint with bounded response/time/redirects.
- Parse exact version/platform/artifact URL/integrity/signature metadata.
- Apply EPR0 update/channel policy and EPR1 compatibility before download.

**Expected changes:**

- Implement source adapter and hermetic metadata fixtures.
- Cache metadata only under exact origin/policy/expiry rules.

**Discriminating proof:**

- Malicious/oversized/malformed/redirected metadata fails closed.
- Version ordering/prerelease/platform mutation matrix selects exactly.

### EPR2-SB3 - Private cancellable download

**Independently testable outcome:** Artifact bytes are written only to a private temporary file with bounded size/time and no partial cache visibility.

**Architecture:**

- Use bounded streaming, content-length/actual-size limits, cancellation/deadline, fsync policy.
- Never execute/source/import downloaded content.
- Handle proxy/auth without logging credentials.

**Expected changes:**

- Implement I/O-pool command and progress/status events.
- Clean temp files on failure/cancel.

**Discriminating proof:**

- Cancel/timeout/disk-full/network-drop leaves no candidate/READY entry.
- Secret-redaction and zero-network-when-forbidden tests pass.

### EPR2-SB4 - Integrity, signature, and origin verification

**Independently testable outcome:** Downloaded bytes are cryptographically tied to authorized metadata/trust before extraction.

**Architecture:**

- Verify registry integrity/checksum and signature/attestation when policy requires.
- Bind redirect final origin and metadata receipt.
- Reject missing/weak/downgraded evidence.

**Expected changes:**

- Produce EPR1 integrity/origin evidence.
- Quarantine/delete only managed temporary data on failure.

**Discriminating proof:**

- One-byte/signature/trust-root/origin substitution mutations fail before extraction.
- Failure remains Integrity/Trust, not NotFound fallback.

### EPR2-SB5 - Safe extraction and atomic installation

**Independently testable outcome:** Only expected platform artifact files reach a private immutable managed entry.

**Architecture:**

- Reject absolute/parent/symlink/hardlink/reparse/device entries.
- Bound file count, path length, uncompressed size, and permissions.
- Validate executable/manifest then atomic rename and READY-last under lock.

**Expected changes:**

- Implement safe extractor/installer over EPR1 APIs.
- Loser installers verify winner or discard temp.

**Discriminating proof:**

- Traversal/bomb/symlink/crash/concurrent install matrix never escapes root or exposes partial entry.
- Installed descriptor/digest equals acquisition receipt.

### EPR2-SB6 - Enterprise proxy/offline/update behavior

**Independently testable outcome:** Managed acquisition behaves predictably under proxy/custom CA/offline/air-gap/update/rollback policy.

**Architecture:**

- Support explicit proxy/no-proxy/custom root inputs without persisting secrets.
- Honor offline/deny-network immediately.
- Separate explicit install/update requests; no background update.

**Expected changes:**

- Add hermetic proxy/TLS simulators and configuration integration.
- Expose stable remediation status.

**Discriminating proof:**

- Offline makes zero socket/DNS attempts.
- Proxy/auth failures are typed and do not fall back to unapproved origins.

### EPR2-SB7 - Acquisition observability and zero-work proof

**Independently testable outcome:** Acquisition work is explicit, auditable, bounded, and absent from normal resolution unless requested.

**Architecture:**

- Count metadata requests/download bytes/verification/extraction/files/install attempts/cleanup.
- Emit audit events without secrets.
- Prove EPR4 warm resolution never triggers network.

**Expected changes:**

- Add PER0/security receipts and long-cancel cleanup tests.
- Expose acquisition command/status through approved application surface only.

**Discriminating proof:**

- Policy-forbidden/unopened channel performs zero network/filesystem install work.
- Repeated explicit acquisition reuses valid immutable entry or performs exact declared update check.

## Data, identity, invalidation, and publication laws

- Engine acquisition, resolution, selection, and activation are distinct authorities with distinct receipts; no stage infers success from a later stage.
- No executable is run before exact origin, compatibility, integrity, and trust validation succeeds.
- Unavailable, unauthorized, incompatible, corrupt, offline, cancelled, and operationally failed outcomes remain distinct and capability-visible.
- All caches/installations are content/version/platform scoped, race-safe, and fail closed on symlink/reparse/permission/ownership violations.
- Acquisition requests are explicit side effects and never run inside a semantic/query hot path.
- Downloaded metadata/artifacts are untrusted until EPR1 validation receipts exist.
- Managed acquisition may mutate only its private managed root.

## Migration and cutover

- Create module/dependencies only after external authorization.
- Implement against hermetic local test server before any real registry canary.
- Keep existing manual/project/system/editor sources unchanged; add managed source only to EPR4 after acceptance.

## Deletions

- Delete blocked consume-only download-tier prose after this charter/source atoms land.
- Delete any ad hoc shell/npm/npx download execution path.
- Delete temporary insecure dependency/config experiments.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

## Forbidden designs

- Background/implicit download or update.
- TLS verification disable, arbitrary URL, unbounded redirect/response/archive.
- Executing package scripts or downloaded code during install.
- Extracting before integrity verification or outside private temp root.
- Treating integrity/trust failure as a reason to try another origin silently.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

## Acceptance IDs and discriminating proof

- **EPR2-AC-AUTHORIZED:** channel is unreachable without exact policy/external authorization.
- **EPR2-AC-ZERO-HIDDEN-NET:** normal resolution and forbidden/offline policies make zero network attempts.
- **EPR2-AC-VERIFY-FIRST:** byte/origin/signature mutations fail before extraction/execution.
- **EPR2-AC-SAFE-EXTRACT:** traversal/bomb/link/crash/concurrency matrix is contained and atomic.
- **EPR2-AC-ENTERPRISE:** proxy/custom CA/offline/secret-redaction behavior is exact.
- **EPR2-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **EPR2-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **EPR2-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **EPR2-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Performance and bounded work

- Metadata/download/extraction have explicit byte/file/time/CPU limits; normal warm resolution performs no EPR2 work.
- Target ceiling: 800 production LOC, 8 production files, and 2 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Mandatory rescope and abort conditions

- Abort if dependency/security review is not accepted.
- Abort if a platform/archive cannot satisfy safe no-follow/atomic install semantics.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. Hermetic HTTP/TLS/proxy/redirect/metadata/download failure matrix.
1. Integrity/signature/origin and safe archive/extraction/crash/concurrency tests.
1. Policy/authorization/no-network/secret-redaction/cleanup/equivalent-work receipts.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Consumers and unlocks

- When opened, supplies managed verified candidates to EPR4.
- Does not by itself advertise engine availability or activate a provider.
- Can remain permanently unopened under a no-download policy.

## Source reconciliation

- `docs/arch/future/engine-provisioning-download-tier.md`.
- EPR0/EPR1 policy and cache/install contracts.

Durable clauses are transferred as digest-bound requirement atoms. Historical path archaeology, obsolete branches, and implementation journals are not copied into the charter. Git history remains the archive.
