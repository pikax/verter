<!-- unified-charter-v2
id=EPR2
name=Managed download and verified atomic installation channel
predecessors=EPR1,G5
phase=expansion
train=expansion.engine-provisioning
product=engine_provisioning
kind=implementation
semantic_role=delivery
class=successor
owner=expansion.engine-provisioning:explicit policy-controlled engine acquisition, resolution, and activation authority
conflict_domains=provider_lifecycle,scheduler_admission,source_lineage
resource_class=rust-mixed
gate_profile=targeted-domain
review_profile=security-3
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=high
verification_effort_default=high
confirmation_effort_min=high
confirmation_effort_default=high
dispatchable=true
optional=true
release_gating=non_release
external_requirements=maintainer_managed_engine_acquisition
charter=charters/expansion-engine-provisioning/EPR2.md
size=M
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# EPR2 — Managed download and verified atomic installation channel

Readiness is derived only from implemented-ledger rows for the node ancestors. Commit message, approximate timezone-bearing date, and optional PR are loose locator hints; the CLI performs no Git, GitHub, SHA, tree, ancestry, receipt, lease, or digest validation.

## Independently acceptable outcome

Optionally implement managed network acquisition of an authorized engine artifact: resolve a policy-compatible release, download through approved HTTP/TLS/proxy infrastructure, verify integrity/signature before exposure, safely extract/install under EPR1, and publish an immutable acquisition receipt. The block remains closed unless `maintainer_managed_engine_acquisition` is present.

The current owner is **a consume-only cache reader, no HTTP/TLS writer, blocked download-tier prose, and implicit npm registry assumptions**. The final and sole owner is **one policy-gated EngineAcquirer and verified atomic installer with no hidden network behavior and exact origin/integrity receipts**.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_tsgo_api`.
- Pack production inventory:
- `crates/verter_tsgo_api::toolchain::acquire` or a narrower dedicated acquisition crate
- approved HTTP/TLS/archive dependencies and dependency-policy records
- managed cache/install root from EPR1
- registry/release metadata adapters
- audit/security/proxy/offline test harnesses

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.
- Named API/data boundaries:
- `EngineAcquisitionRequest`, `EngineReleaseRequirement`, and `EngineAcquisitionPlan`
- `EngineReleaseIndex`, `EngineReleaseDescriptor`, and exact origin metadata
- `EngineDownload`, `EngineDownloadProgress`, and cancellation/deadline controls
- `VerifiedArchive`, `SafeExtractionPlan`, and `EngineAcquisitionReceipt`
- `AcquisitionFailure::{Forbidden, Offline, Proxy, TLS, Origin, Integrity, Archive, Install, Cancelled}`

## Exact predecessor contracts

- **EPR1:** implemented ledger row for “Engine artifact identity, compatibility, integrity, and cache contract”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **G5:** implemented ledger row for “Scheduler pool host runtime convergence”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirement maintainer_managed_engine_acquisition:** agents obtain the maintainer decision; tooling does not validate it.

## Source-specific scope

### Binding architecture

- No acquisition code is reachable unless the captured EPR0 policy and external authorization allow managed download.
- Release metadata and artifacts come only from declared authorized origins; redirects are bounded and revalidated.
- TLS verification is never disabled; custom enterprise roots/proxy configuration are explicit runtime inputs.
- Integrity/signature is verified on downloaded bytes before extraction and again against installed executable evidence as required.
- Archive extraction is path-safe, bounded, no-follow, and restricted to the temporary private root.
- Installation follows EPR1 temp/lock/atomic rename/READY-last semantics.
- Acquisition receipt does not imply selection or activation; EPR4/EPR5 revalidate required facts.
- No telemetry, registry ping, or auto-update occurs outside an explicit acquisition request.

### Internal subblocks

#### EPR2-SB1 - Dependency, threat, and policy review

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

#### EPR2-SB2 - Release metadata and version resolution

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

#### EPR2-SB3 - Private cancellable download

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

#### EPR2-SB4 - Integrity, signature, and origin verification

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

#### EPR2-SB5 - Safe extraction and atomic installation

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

#### EPR2-SB6 - Enterprise proxy/offline/update behavior

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

#### EPR2-SB7 - Acquisition observability and zero-work proof

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

### Identity, invalidation, and publication

- Engine acquisition, resolution, selection, and activation are distinct authorities with distinct receipts; no stage infers success from a later stage.
- No executable is run before exact origin, compatibility, integrity, and trust validation succeeds.
- Unavailable, unauthorized, incompatible, corrupt, offline, cancelled, and operationally failed outcomes remain distinct and capability-visible.
- All caches/installations are content/version/platform scoped, race-safe, and fail closed on symlink/reparse/permission/ownership violations.
- Acquisition requests are explicit side effects and never run inside a semantic/query hot path.
- Downloaded metadata/artifacts are untrusted until EPR1 validation receipts exist.
- Managed acquisition may mutate only its private managed root.

### Migration and cutover

- Create module/dependencies only after external authorization.
- Implement against hermetic local test server before any real registry canary.
- Keep existing manual/project/system/editor sources unchanged; add managed source only to EPR4 after acceptance.

### Consumers and unlocks

- When opened, supplies managed verified candidates to EPR4.
- Does not by itself advertise engine availability or activate a provider.
- Can remain permanently unopened under a no-download policy.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **EPR2-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or reproduced defect that existing evidence does not discriminate.
- **EPR2-AC2 — positive contract:** the named API/data boundary preserves exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **EPR2-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm; otherwise bind a terse not-applicable rationale.
- **EPR2-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, provider, filesystem, network, allocation, copy, or retained-candidate work; otherwise bind a terse not-applicable rationale.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; do not add implementation mirrors, duplicate permutations, or universal test quotas.
- Test homes: `crates/verter_session/tests/cases`, `crates/verter_protocol/tests`, `packages/typescript-plugin/src`, and the exact generated vertical fixture selected by this node.


### Pack-specific proof obligations

- **EPR2-AC-AUTHORIZED:** channel is unreachable without exact policy/external authorization.
- **EPR2-AC-ZERO-HIDDEN-NET:** normal resolution and forbidden/offline policies make zero network attempts.
- **EPR2-AC-VERIFY-FIRST:** byte/origin/signature mutations fail before extraction/execution.
- **EPR2-AC-SAFE-EXTRACT:** traversal/bomb/link/crash/concurrency matrix is contained and atomic.
- **EPR2-AC-ENTERPRISE:** proxy/custom CA/offline/secret-redaction behavior is exact.
- **EPR2-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **EPR2-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **EPR2-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **EPR2-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Deletions and forbidden designs

- Delete blocked consume-only download-tier prose after this charter/source atoms land.
- Delete any ad hoc shell/npm/npx download execution path.
- Delete temporary insecure dependency/config experiments.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

- Background/implicit download or update.
- TLS verification disable, arbitrary URL, unbounded redirect/response/archive.
- Executing package scripts or downloaded code during install.
- Extracting before integrity verification or outside private temp root.
- Treating integrity/trust failure as a reason to try another origin silently.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

- Delete or structurally reject every compatibility path that would preserve a second owner after cutover.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 800 production LOC, 8 production files, 2 related crates/packages.
- Mandatory rescope above 1500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, identity aliasing, or unauthorized executable work.
- Performance budget: when preflight identifies touched authority or a hot path, use the ratified replacement SLO and equivalent-work counters below; otherwise performance evidence is not applicable and no soak is invented solely to populate evidence.

- Metadata/download/extraction have explicit byte/file/time/CPU limits; normal warm resolution performs no EPR2 work.
- Target ceiling: 800 production LOC, 8 production files, and 2 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Abort conditions

- Abort if dependency/security review is not accepted.
- Abort if a platform/archive cannot satisfy safe no-follow/atomic install semantics.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. `node roadmap/0.1.0-tama/tools/validate-program-dag.mjs --strict`
2. Run every final command in the bound `targeted-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

### Pack-specific verification inventory

1. Hermetic HTTP/TLS/proxy/redirect/metadata/download failure matrix.
1. Integrity/signature/origin and safe archive/extraction/crash/concurrency tests.
1. Policy/authorization/no-network/secret-redaction/cleanup/equivalent-work receipts.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN evidence when applicable, the configured independent review profile, and the owning final gate on the squashed review candidate.

## Review and lower-severity findings

Apply `security-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `supply-chain-platform`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch adds one `[[implemented]]` row to `authority/state/implemented.toml` with the node ID, planned squash commit message, approximate date with timezone, and optional pull-request number. Row presence is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.
