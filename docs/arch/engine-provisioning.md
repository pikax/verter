# External engine provisioning and trust

This constitution captures the maintainer ruling **epr0-network-bundling**,
answered 2026-11-01: automatic acquisition requires explicit user/admin opt-in
for the project and an explicit acquisition request. Ordinary resolution never
initiates network work. Enterprise denial and offline policy override opt-in.
Opt-in grants neither background updates nor execution. Bundled distribution
remains unauthorized, including in the VSIX.

This is a zero-production decision: it changes no discovery, process, package,
configuration parser or public DTO. It describes `feat(session): reserve module
homes for the lint, css, a11y, formatter, (#1059)`, 2026-11-01. That title and
date are context only. The machine-readable contract is
`tests/engine-provisioning/EPR0/products/engine-policy.v1.json`; its case table
is `tests/engine-provisioning/EPR0/products/engine-cases.v1.json`. Neither is a
runtime implementation or a claim that today's routes enforce the final policy.

## Authority and captured inputs

One immutable **EngineProvisioningPolicy** owns source authorization for one
configured project, profile and execution host. It consumes the captured
[configuration](kernel/configuration.md) inputs (including CF-H03); it is not
another base configuration layer. User/admin host inputs select engines and
opt in. Workspace content, an environment variable, a package script or a
discovered executable cannot grant permissions. Enterprise restrictions
intersect user permissions; a project/profile may narrow but never widen them.
Absent or invalid opt-in means no automatic acquisition. Unknown policy schema,
source or required field fails closed, without falling back to defaults.
`verter.lspBinaryPath` locates the Verter host; it grants no external-engine
trust or execution permission.

The effective snapshot contains schema version, policy epoch, project/profile/
host identity, authorizations and their provenance, ordered source lists per
requirement, update/offline/proxy/rollback policy, trust-root identities,
revocation generation and compatibility policy identity. Secret values and
credential-bearing URLs are excluded. EPR1 specifies structural artifact and
receipt identities; EPR4 implements the common validator and selector; EPR5
owns activation and publication. The protocol/session adapters serialize those
facts under the [public envelope](kernel/public-envelope.md), without another
policy decision. EPR6 checks the complete opened-channel matrix.

Four authorities have distinct receipts:

| Authority                  | Receipt establishes                                                                       | Does not establish                                |
| -------------------------- | ----------------------------------------------------------------------------------------- | ------------------------------------------------- |
| EPR2 acquisition           | Exact requested artifact bytes, approved origin and verified installation evidence        | Selection, permission to execute, or availability |
| EPR4 resolution/validation | Authorized candidates with current origin, integrity, compatibility and trust evidence    | A running provider                                |
| EPR4 selection             | One exact candidate and complete rejection provenance under the captured order/basis      | Successful activation                             |
| EPR5 activation            | Revalidated selection, successful capability/protocol handshake and current ProviderEpoch | Authority for another project or epoch            |

## Sources and deterministic selection

Discovery visibility, selection authorization, acquisition permission and
execution permission are separate. An unauthorized source receives no adapter
invocation, filesystem scan, metadata probe or executable work. An environment
override naming a forbidden cache/bundle location does not launder its origin.
Canonical aliases retain every source constraint; the most restrictive applies.
Project package-manager links may be resolved only within the trusted install
closure; managed roots/installations never follow links or reparse points.

The following table is rendered from the catalog's `orders` rows. These are
explicit defaults for the later selector, preserving current usable local
ordering by requirement. They do not certify current implementations. A policy
may reorder permitted sources; that produces a new epoch and resolution basis.
An explicit engine choice never silently changes engine family.
An exact manual/CLI engine path restricts selection to that candidate; a
rejection is a hard typed error, with no lower-tier fallback.

<!-- source-order:start -->

| Requirement            | Ordered sources                                              | Closed sources                                     |
| ---------------------- | ------------------------------------------------------------ | -------------------------------------------------- |
| tsgo LSP/API           | EnvironmentManual → SystemPath → ProjectLocal → ManagedCache | ManagedDownload, Bundled                           |
| managed tsserver       | ProjectLocal → EnvironmentManual → SystemPath                | ManagedCache, ManagedDownload, Bundled             |
| editor tsgo rendezvous | EnvironmentManual → ProjectLocal                             | ManagedCache, ManagedDownload, Bundled, SystemPath |
| editor-owned provider  | EditorShared                                                 | All filesystem/acquisition sources                 |
| extension TypeScript   | ProjectLocal                                                 | All other sources                                  |

<!-- source-order:end -->

Editor rendezvous uses the explicit SDK sequence `js/ts.tsdk.path`,
`typescript.tsdk`, `typescript.native-preview.tsdk`, then the workspace platform
package, after `VERTER_TSGO_BIN`. The editor owns the resulting child; it must
attest the exact engine, project and endpoint. A shared route is attempted only
when the product explicitly requests that topology. It cannot upload source or
switch to a remote host implicitly. Failed attachment is recorded before any
separately authorized next topology is attempted.

Within ProjectLocal, nearest owning-project ancestor wins; preserve the tsgo
flat package, sorted pnpm store, nested package, then shim order. Within a store
or managed cache, greatest supported exact version wins, with canonical
artifact identity as a deterministic tie-breaker. SystemPath preserves captured
PATH entry order and platform-manifest executable order; tsserver's global
package lookup is its explicit SystemPath adapter. Duplicate identities are
validated once, retaining aliases and rejection reasons. Enumeration permutation
must not change selection. No ambient CWD or sibling project's engine is used.
Integrity/trust failures terminate selection, never degrade to NotFound or a
successful lower-trust fallback. Nonsecurity incompatibility may advance only
where the captured policy explicitly permits it, preserving the rejected input
and the toolchain substitution in the result.

## Network, trust and enterprise law

**EngineAcquisitionPermission** is `Forbidden`, `ManualOnly` or `Allowed`.
Forbidden grants no acquisition; ManualOnly permits the user/admin to supply
local verified artifacts but performs no automatic network work; Allowed
requires a recorded project opt-in and a separate explicit install/update
request. It is still overridden by enterprise denial, cancellation or offline
state. ManagedCache is consume-only and does not imply ManagedDownload.

The authorized initial managed-origin set is HTTPS `registry.npmjs.org`, scoped
to the exact `typescript` and `@typescript/typescript-{darwin,linux,win32}-{arm64,x64}`
packages represented by the existing platform manifest. No arbitrary registry,
mirror, URL, GitHub release or package name is authorized by that pattern.
The six concrete platform names are enumerated in the catalog. Enterprise
mirrors need an explicit admin-approved TrustedEngineOrigin entry with package
scope and trust-root identity; they are never inferred from npm configuration.

TLS hostname/certificate validation is mandatory. System CA roots and explicit
admin-approved enterprise CA roots have separate EngineTrustRootIds and policy
epochs. An unapproved root, insecure TLS switch, plain HTTP, or redirect outside
the exact authorized origin/package scope is TrustFailure before artifact use.
Registry-authenticated metadata must bind an exact version/platform tarball
with SHA-512 integrity. Verify it before extraction and bind installed-file
digests to it; missing integrity is IntegrityFailure. If a source policy
requires signatures/attestations, their accepted signer/root set is explicit
and missing or revoked evidence fails closed. EPR2 reviews network/archive
dependencies, TLS, redirect and extraction bounds before implementing the
channel. No package lifecycle script executes during acquisition.

Manual/project/system candidates need an admin/user-approved local trust
receipt binding origin, exact bytes and their entire execution closure
(including Node, scripts and required libraries), compatibility and platform;
mere existence, execute bits, package name or a successful `--version` is not
trust evidence. No candidate code, even a version probe, runs before the
nonexecuting checks pass. EPR4 validates the evidence; EPR5 revalidates against
replacement between selection and spawn. Editor attachment requires a trusted
endpoint/owner receipt as well as engine and capability evidence.

**EngineOfflinePolicy** is OnlineAllowed, Offline or DenyNetwork. Offline uses
only already verified permitted local artifacts; missing ones return Offline
for acquisition and NotFound/NeedInputs for local resolution as appropriate.
DenyNetwork is an administrative prohibition, reported Forbidden with its
policy reason. Neither may cause DNS, sockets, registry queries, telemetry or
install writes. Proxy/no-proxy and custom CA configuration are explicit
**EngineProxyPolicy** inputs; credential references are runtime-only. Auth,
proxy or TLS errors never trigger direct-network or alternate-origin fallback.
Public reports use origin IDs and redacted diagnostics, never credentials,
tokens, raw proxy URLs, private source or secret-derived hashes. No acquisition
telemetry or unrelated registry ping is permitted.

## Version, installation and lifecycle

**EngineUpdatePolicy** defaults to ExplicitOnly: no timer, first-use download,
background update or registry freshness check. Exact requested pins take
precedence over a compatible channel; revocation and compatibility reject even
a pin. An unpinned explicit request selects the greatest supported exact
version. Current tsgo support comes from `toolchain/policy.rs` (workspace pin,
inclusive floor, next-minor ceiling, permitted beta/rc channel); tsserver's
serving tiers remain separately identified in `verter_type_runtime::discovery`.
Policy authorizes no new version range and changes no real dependency pin.

Policy schema 1 has no implicit migration: a future unknown schema is
Unsupported until an explicit migration produces a new immutable snapshot.
Any source-order, permission, trust, origin, revocation, compatibility or
platform change invalidates affected receipts and supersedes in-flight work.
Changing credentials alone supplies new runtime inputs, never a secret digest.
Publication checks the current project/host/policy/provider epochs. Cancelled,
stale, partial and NeedInputs results never warm-admit as complete. An unchanged
basis may reuse a complete receipt only after its recorded facts revalidate;
incremental selection must equal fresh selection.

EPR1 defines content/version/platform-scoped immutable entries and receipts.
EPR2 installs under a private managed root, bounded extraction, no-follow
locks, private temporary output, atomic rename and READY-last publication.
Partial, corrupt, wrongly owned or permission-unsafe entries are not candidates.
Concurrent losers validate the winner or discard only their own temporary
files. A project package-manager symlink is not permission to follow links in
the managed root. Resolution never installs, repairs, quarantines user files
or acquires missing bytes.

**EngineRollbackPolicy** is PreviousVerified or Disabled, explicitly captured.
PreviousVerified retains one prior known-good receipt, rechecks compatibility,
trust, revocation and current permission, then asks EPR5 for an atomic provider
swap. No valid retained artifact yields NeedInputs; it does not fetch an old
version. Disabled requires explicit user repair. Emergency admin revocation
bumps policy/revocation epochs, cancels pending work and prevents publication
or reuse of the revoked engine; EPR5 withdraws its capabilities and tears down
owned processes/transports. Failed replacement cannot publish mixed epochs.

## Typed results and capability truth

**EngineNeed** identifies the project/profile, required engine family and
capability, authored request/snapshot and execution host. **EngineRequirement**
adds platform/ABI, exact pin or compatibility policy, required protocol/features,
source order and deadline/cancellation context. **EngineSourcePolicy** binds
EngineSourceKind to discovery/select/execute permissions, acquisition enum,
origin/root IDs and refusal/fallback law. All are captured data; policy
evaluation itself performs no I/O.

**EngineProvisioningOutcome** carries phase (acquire, resolve, select,
activate), typed state/reason, need/requirement and policy identities,
completeness, ordered candidate rejections, redacted provenance, applicable
receipts and actionable user/admin remediation. Each stage reports its own
success: Acquired, Resolved or Selected carries that stage's receipt, and only
activation reports Available. The remaining states are NeedInputs, Forbidden,
Offline, NotFound, Incompatible, IntegrityFailure, TrustFailure, Cancelled and
OperationalFailure; Unsupported explicitly covers unknown schema, host or
capability. Missing input, exhaustive absence, candidate rejection and
activation failure remain distinct. No missing provider is reported as a
successful provider-off result.

Stage success is not capability availability. **EngineCapabilityState** stays
Unavailable through Acquired, Resolved and Selected, and becomes Available only
with a current selection AND activation receipt proving the requested
handshake. An acquisition receipt, installed path, provisional resolution,
shared-process PID or stale provider is insufficient. Unaffected native
inspection capabilities keep their own authority; they do not replace an absent
TypeScript semantic answer. PUB0/COX0 transport and eligibility rules consume
this truth, preserving per-operation unsupported/partial/refusal distinctions
and provenance.

Browser-local artifacts can execute only browser-compatible code with explicit
artifact trust/permissions. A native EngineRequirement there is Unsupported;
there is no native spawn or automatic companion fallback. Local-native
companions and remote workspace hosts use the same policy/receipts but distinct
host/platform identities and explicit execution/source-transfer permission.
Opting into acquisition never authorizes remote execution or source upload.
Exposed features need the DX1 descriptor and executable/replay evidence before
promotion; shared-core proof cannot certify a browser/JetBrains/Lapce UI.

## Bundling and legacy disposition

No package family or platform is authorized to ship an external engine. The
Verter relay shim is a Verter artifact, not a bundled tsgo permission. Preserve
the existing package/staging whitelist and no-tsgo tests. Optional future
bundling requires a separate maintainer authorization naming the release
owner, exact package/platform/version/paths, license notices, SBOM, provenance,
integrity/signature, measured size, update/security-response and package-manager
rollback policy. EPR3 must validate installed package bytes as well as staging;
its present disposition is unopened on all platforms, with zero bundle work.

The catalog inventories current routes and their exact later deletion/receiving
acceptances. EPR4 replaces independent candidate selectors, cache/bundle probes
and SDK/environment bypasses; EPR5 replaces independent attach/spawn/status
decisions, including the CLI/API path's provisional start and raw-path connect
and the relay shim's direct execution of a supplied engine path, while those
routes keep their transport and process ownership.
Existing support-policy and platform tables remain shared inputs.
Existing local routes remain usable until their production owners cut over;
this constitution neither silently deletes nor certifies them.

There are no orphan blocked download/bundle documents in the captured tracked
tree. The surviving tier claims are in `toolchain/discovery.rs`, `bundle.rs`,
`platform.rs` and `policy.rs`; their source atoms are transferred in the catalog,
with production prose corrections owned by EPR4. No packaging prohibition is
deleted. The repository has no roadmap/charter authority to duplicate here.

## Proof boundaries

AC1/POLICY/LEGACY/CONTRACT are covered by the ownership/source catalog and
decision cases. The table records the required negative controls and exact
behavioral owners rather than claiming spec data executes a runtime guard.
AC2/OUTCOMES specify deterministic identities, serialization and redaction;
existing discovery and packaging tests characterize only the listed local
invariants. They do not prove the future policy consumer exists.

AC3/INCREMENTAL and AC4/WORK have no changed runtime cache, publication, hot path
or allocation behavior here. Their contract vectors are retained: EPR4 owns
fresh/warm equivalence, disabled-source zero work and resolution counters;
EPR2 owns denied/offline zero DNS/socket/install work and acquisition counters;
EPR5 owns stale/cancelled activation publication and retained-epoch cleanup;
EPR6 checks cross-channel closure. SOLE's planted duplicate route, unverified
execution, bundle and secret mutations are specified in the case table for
those production-capable owners. No runtime pass, soak or performance claim is
made by this decision. The future warm 100-request control checks work/growth,
with timing/allocation/RSS advisory under the measurement rule.

Contract verification uses `pnpm --filter docs check`, changed-file
`pnpm exec oxfmt --check`, and
`pnpm exec vitest run packages/vue-vscode/src/package.spec.ts`.
Bounded catalog inspection checks JSON parsing, unique source/boundary/case
identities, source-order table agreement, case-to-rule references, complete
acceptance coverage, current source paths and exact receiving-owner acceptance
bindings. Relative documentation links must resolve. These checks establish
contract consistency and the existing packaging prohibition; they do not
execute future acquisition, selection or activation behavior.
Tama owns DAG validation; there is no repository roadmap validator to run.

## Engine artifact and validation contract

This section specifies the one descriptor, receipt and install/cache law every
source adapter consumes before selection or execution. Like the policy above it
adds no production type, evaluator, cache, adapter or status path. Machine forms
live in `tests/engine-provisioning/EPR1/`: named schemas in
`schemas/engine-artifact.schema.json`; decision tables, vectors, limits and the
obligation population in `products/engine-artifact-contract.v1.json`; and owners
in `catalogs/engine-validation-ownership.toml`. A vector proves the specification
rejects a mutation. The owner named in the catalog proves a running system does,
and none of that runtime proof has executed yet.

### Artifact identity

`EngineArtifactId` is the structural tuple engine, exact `EngineVersion`,
`EngineFlavor`, canonical `EnginePlatform`, ABI, protocol, build, origin and
content digest. A digest does not replace a semantic axis, and a path never
enters identity: replacing bytes at one path is a different artifact, and an
`EngineArtifactDescriptor` carries opaque locator IDs only. Two locators of one
verified artifact share one key while keeping two `EngineOriginReceipt`s.
Different origins never alias, even over identical bytes.

Canonical bytes are RFC 8785 member order over safe integers and NFC strings. A
key, closure manifest or receipt digest is SHA-256 over a domain string, one NUL
byte and those bytes; the domains are in the product, and the goldens let a Rust
implementation prove byte agreement. `EngineVersion` is strict SemVer and keeps
build metadata as an identity axis, unlike the precedence comparison in
`toolchain/policy.rs`. `EnginePlatform` uses the Rust target triples of the shared
platform manifest plus `wasm32-browser` and `js-browser`. An npm-style alias is
normalized only through the manifest, and an unlisted triple such as musl is
refused rather than guessed.

### Compatibility contract

`EngineCompatibilityRequirement` binds requirement and compatibility-policy IDs,
engine, allowed flavors, platform, version window, optional exact pin, protocol,
API, commands, features, toolchain family and host features. The verdict compares
it with declarations and never executes a candidate; the bounded handshake stays
with activation. Checks run platform, flavor, host, protocol, API, command,
feature, toolchain, version window, then pin, so a structural mismatch is never
reported as a version problem and a pin cannot override the window.

`EngineCompatibilityVerdict` is `Compatible`, `TooOld`, `TooNew`, `Unsupported`,
`WrongPlatform`, `WrongFlavor`, `UnsupportedProtocol`, `UnsupportedApi`,
`MissingCommand`, `MissingFeature`, `ToolchainIncompatible`, `HostUnsupported` or
`UnknownCompatibility`. Unknown is ineligible, including an unreadable version.
Version boundaries restate the current tsgo line law without moving a pin or floor;
tsserver serving tiers remain a separately identified requirement input. A policy
or requirement change alters the cache key, so a stale receipt cannot answer it.

### Origin, integrity and signature evidence

`EngineIntegrityEvidence` lists every file in the execution closure: executable,
interpreter, scripts, libraries, worker code and critical sidecars. The closure
digest binds that sorted list, so an archive digest or package metadata alone
proves nothing about installed bytes. Registry SHA-512 additionally binds an
archive. Paths must be relative, portable and free of traversal, reserved names,
alternate streams and case collisions. `EngineSignatureEvidence` and provenance
references bind the same digest; `EngineOriginReceipt` binds it to an authorized
channel and `EngineTrustRootId`. `TrustedEngineOrigin` and
`EngineProvisioningPolicy` keep the fields the policy above defines.

`EngineValidationReceipt` is immutable and exists only for a complete, compatible
result on one current basis, so its verdict and completion are constants. It binds
artifact, closure, origin, signature, provenance, requirement, project, profile,
host, policy, trust and revocation epochs, and it has no `trusted` field to assert.
Facts come from what the shared validator observed, never from a producer's claim.
Checks run in the order in the product's `validationOrder`, ahead of any candidate
code: a schema failure, unauthorized source, revocation, origin, closure, bytes,
root, signature, provenance, then artifact class. An unauthorized source receives
no validation work at all.

### Source evidence obligations

| Source            | Trust classes                     | Required evidence                                               | Mutation authority     |
| ----------------- | --------------------------------- | --------------------------------------------------------------- | ---------------------- |
| EnvironmentManual | local approval                    | Origin, closure, compatibility, explicit approval               | Read-only              |
| ProjectLocal      | package closure, local approval   | Origin, closure, compatibility, approval of the trusted closure | Read-only              |
| SystemPath        | local approval                    | Origin, closure, compatibility, explicit approval               | Read-only              |
| EditorShared      | editor attestation                | Origin, closure, compatibility, owner and endpoint attestation  | Read-only              |
| ManagedCache      | registry integrity                | Origin, closure, compatibility, archive integrity, READY marker | Managed installer only |
| ManagedDownload   | registry integrity                | Origin, closure, compatibility, archive integrity               | Managed installer only |
| Bundled           | bundle manifest                   | Origin, closure, compatibility, shipping manifest, provenance   | Shipping owner only    |
| BrowserArtifact   | published or development manifest | Origin, closure, compatibility, worker manifest, host limits    | Browser loader only    |

Sources are the policy's `EngineSourceKind` values plus the browser artifact class.
Optional managed and bundled channels stay inventoried without becoming mandatory
or authorized: the shipping prohibition above is unchanged. Missing evidence is a
typed rejection and never lowers a trust class. No source lacks evidence for its
authorized class, because each class above names evidence that source can produce.

### Browser worker artifacts

WASM and JS worker artifacts use the same descriptor as native sidecars, with
`wasm-single`, `wasm-threaded` or `js-worker` flavor. The manifest binds provenance,
integrity, ABI/schema, engine version and host constraints. A locally built artifact
has its own build identity and trust class, needs explicit development policy, and
cannot become a verified published artifact. Native flavors never run on a browser
platform, and no contract assumes tsgo has a browser artifact. Baseline operation
needs neither `SharedArrayBuffer` nor cross-origin isolation; a threaded artifact
without them is `HostUnsupported`.

### Installation layout and concurrent writers

`EngineInstallLayout` is `<private-root>/v1/<platform>/<artifact-key>/` holding the
manifest, closure, validation evidence and `READY.json`. The manifest repeats the
full structural ID and is compared on load. States run Absent, PrivateTemp,
Extracted, Verified, Renamed, Ready, with Quarantined reachable only by the managed
installer from Renamed or Ready. Only Ready is selectable, and only Renamed reaches
it. `EngineReadyMarker` binds artifact, manifest digest and install generation and
is published last, but it is not trust evidence: a reader still validates. A crash
before the rename leaves nothing to enumerate; a crash after it leaves
`IncompleteEntry`. A Ready entry is immutable, so an update is a different entry.

Components are opened no-follow with verified handles, not checked by path and
reopened. A symlink, junction, foreign owner, group- or world-writable mode or a
non-owner-only ACL rejects with its own reason. Writers take an artifact-scoped lock
and create a private exclusive sibling. A loser adopts a winner only if it has the
same identity and validates on the current basis, and it removes only its own
temporary output. Lock age never licenses breaking a live writer; recovery first
establishes owner death.

Each supported platform has the required primitives, so no install abort condition
applies. The `platformPrimitives` table names them: `openat` with `O_NOFOLLOW` on
Linux and macOS and `FILE_FLAG_OPEN_REPARSE_POINT` on Windows; exclusive private
creation; `RENAME_NOREPLACE`, `RENAME_EXCL` or `MoveFileExW` without replacement;
and the durable flush each one requires. A filesystem refusing no-replace fails
closed and never degrades to check-then-rename. Extraction stops at 16,384 entries,
512 MiB expanded, 200:1 compression or 64 path components, and refuses link, device
and traversal entries.

### Validation cache and invalidation

`EngineCacheKey` holds artifact key, closure digest, origin receipt, locator stat
identity, project, profile, host, requirement, compatibility policy, schema and
policy, trust and revocation epochs. Any field change validates fresh.
`EngineCacheEntry` retains one complete receipt. Only `Complete` with `Compatible`
is admitted; no negative result is cached, so a rejected or unknown answer cannot
outlive a fact that could change it. Cancelled, partial and stale results never warm.

An immutable READY entry inside a retained verified directory handle reuses with
zero stat, read, hash, process or network work. Restart, handle loss, directory
replacement, permission change or watcher overflow leaves that boundary and forces
fresh validation. A mutable locator compares per-file stat identity on each
resolution and rehashes only on change, so warm work is bounded without a repeated
scan; activation always rehashes it through verified handles. One validation runs
per full key, and supersession cancels its publication.

Per project and host, retention is at most 256 receipts and 16 MiB of evidence, with
least-recently-used eviction; an entry above the byte limit returns uncached. These
are local bounds on validation evidence, which holds no semantic artifact and sits
outside the semantic retention account. A closure holds at most 16,384 files and
1 GiB; a manifest holds at most 16 MiB. `EngineValidationWorkCounters` count
candidates, stats, reads, hashes, manifests, signature checks, hits, invalidations,
retained bytes and entries, network attempts and process spawns. The product lists
each zero-work mode and its owner, including the 100 identical warm requests with
zero added work and no retained growth. The measurement rule applies: work counts
decide acceptance, and this contract makes no runtime performance claim.

### Revocation, corruption and quarantine

`EngineRevocation` binds an artifact, origin, signer or trust root to a reason,
issuer and rising epoch. A revocation catalog is verified before use, and a required
catalog without current freshness fails closed as `RevocationDataUnavailable`. The
product has one action for each stage and event: validation rejects; selection
publication cancels; pending activation cancels; an active provider withdraws its
capabilities. None reuses a receipt, falls back or reports not-found.

Integrity failure stays distinct from revocation, incompatibility and operational
failure. Managed corruption is rejected, then quarantined by the installer. Manual,
project, system and editor artifacts are reported and never mutated; bundled
withdrawal belongs to shipping, and a browser loader evicts only its own cache.
Update needs an explicit request. Rollback revalidates a retained artifact under
current facts and is never an automatic route around an integrity, trust or
revocation rejection.

### Public validation status

Every `EngineRejection` reason has one row in the product, with outcome state,
selection effect and remediation class. Integrity and trust reasons terminate
selection. Compatibility reasons may advance only where captured policy permits and
keep the rejection recorded; an exact manual path stays a hard error. No rejection
becomes `NotFound`, whose row means exhaustive absence with nothing rejected.

`EnginePublicStatus` is an allowlisted projection of source, version, state, reason,
remediation, artifact key, receipt digest and completion for the
[public envelope](kernel/public-envelope.md). Anything else stays private by
construction. Absolute roots, credential URLs, tokens and certificates cannot reach
it, and portable receipts carry only opaque IDs and digests.

### Runtime ownership transfer

The catalog binds each of the population's outcomes, consumers and displaced routes
to one production-capable owner, a dependency path from this contract, and the exact
receiving acceptance. The contract keeps only its own schemas, tables and validators.

| Obligation                                                                          | Owner                                      |
| ----------------------------------------------------------------------------------- | ------------------------------------------ |
| Shared types, evaluator, receipt construction, source adapters, READY validation    | EPR4-AC-CANDIDATES, EPR4-AC-SOURCES        |
| Receipt cache, invalidation, budgets, warm control                                  | EPR4-AC-CACHE, -INCREMENTAL, -WORK         |
| Rejections, status and redaction adapters                                           | EPR4-AC-CONTRACT                           |
| Displaced selectors, probes, READY and path shortcuts                               | EPR4-AC-CANDIDATES, EPR4-AC-SOLE           |
| Managed evidence, atomic install, concurrent writers, quarantine (optional channel) | EPR2-AC-VERIFY-FIRST, EPR2-AC-SAFE-EXTRACT |
| Shipping provenance, installed layout, withdrawal (optional channel)                | EPR3-AC-SUPPLY, EPR3-AC-INSTALLED          |
| Activation races, raw-path launches, status, lifecycle work                         | EPR5-AC-REVALIDATE, -CAPABILITY, -WORK     |
| Browser artifact loading; local companion consumption                               | BWH8-AC1, BWH8-AC-OWNER; DX4-AC-OWNER      |

A required candidate boundary never rests on an optional channel: the install layout
and READY marker types belong to the required resolver, and the installer that writes
them is optional. Displaced routes describe the tree named in the catalog; their
owners may delete those sources without invalidating the contract.

The validator also reads the policy catalog's own source, route and consumer rows
and applies the same refusals, adding an unclassified source and a route with no
source files or symbols. It restates none of them, so that catalog stays their one
authority.

Run from the repository root:

```
node --test tests/engine-provisioning/EPR1/engine-contract.test.mjs
node tests/engine-provisioning/EPR1/tools/validate-engine-validation-ownership.mjs --structural
node tests/engine-provisioning/EPR1/tools/validate-engine-validation-ownership.mjs --live
```

CI runs the first command, which includes the structural mode and every refusal
fixture over synthetic authority. `--live` resolves owners, edges, production
authority and acceptance IDs against the plan controller, which holds the DAG, and
fails if the controller is unavailable. It accepts pending owners: it verifies the
transfer, not their implementation. Neither mode reads a repository copy of the DAG.
