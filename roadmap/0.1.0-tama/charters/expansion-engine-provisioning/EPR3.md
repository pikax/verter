<!-- unified-charter-v2
id=EPR3
name=Bundled sidecar shipping and distribution channel
predecessors=EPR1
phase=expansion
train=expansion.engine-provisioning
product=engine_provisioning
kind=implementation
semantic_role=delivery
class=successor
owner=expansion.engine-provisioning:explicit policy-controlled engine acquisition, resolution, and activation authority
conflict_domains=provider_lifecycle,program_authority,source_lineage,cli_application
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
external_requirements=maintainer_bundled_engine_shipping
charter=charters/expansion-engine-provisioning/EPR3.md
size=M
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# EPR3 — Bundled sidecar shipping and distribution channel

Readiness is derived only from implemented-ledger rows for the node ancestors. Commit message, approximate timezone-bearing date, and optional PR are loose locator hints; the CLI performs no Git, GitHub, SHA, tree, ancestry, receipt, lease, or digest validation.

## Independently acceptable outcome

Optionally implement a bundled engine sidecar distribution channel owned by explicit release artifacts. Build/release stages acquire a pinned verified engine, stage it into authorized platform packages, emit manifest/SBOM/license/provenance evidence, validate installed layout end to end, and make the immutable bundled candidate visible to EPR4. The block remains closed unless `maintainer_bundled_engine_shipping` is present.

The current owner is **a complete runtime bundle-location/manifest reader, release packages that ship the server, and build guards explicitly forbidding tsgo-shaped artifacts**. The final and sole owner is **one authorized per-platform bundled engine artifact family with exact release provenance, installed validation, size/update/rollback policy, and no unauthorized package inclusion**.

## Concrete surfaces and APIs

- Production surfaces: `packages/verter-lsp`, `crates/verter_tsgo_api`.
- Pack production inventory:
- release workflow/build matrices and platform package staging scripts
- `packages/verter-lsp` or another explicitly named artifact family
- VSIX/editor packages only if separately authorized by EPR0 policy
- `crates/verter_tsgo_api::toolchain::bundle` validation contract
- SBOM/license/provenance manifests and installed-package E2E tests

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.
- Named API/data boundaries:
- `BundledEngineReleaseSpec`, `BundledEngineInputReceipt`, and platform target matrix
- `BundledEngineManifest`, file digest table, and install-relative layout
- `BundledEngineProvenance`, `BundledEngineSbomRef`, and license notice set
- `BundledPackageReceipt` and `InstalledBundleValidationReceipt`
- `BundleShippingPolicy` and authorized package IDs

## Exact predecessor contracts

- **EPR1:** implemented ledger row for “Engine artifact identity, compatibility, integrity, and cache contract”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirement maintainer_bundled_engine_shipping:** agents obtain the maintainer decision; tooling does not validate it.

## Source-specific scope

### Binding architecture

- Bundling is authorized per package family/platform/version; a general “bundling enabled” flag is insufficient.
- Release inputs are pinned and verified independently; build machines do not fetch latest unpinned artifacts.
- The shipped bytes and manifest are validated after packaging/extraction, not only before staging.
- Package whitelists/guards are amended deliberately to allow only exact authorized paths/digests/layouts.
- SBOM, license, provenance, size, update cadence, rollback, and security response are release acceptance inputs.
- Runtime sees the bundled candidate through the same EPR1 descriptor/validation as other sources.
- Unavailable platform rows remain explicit; no package claims a bundle that it does not contain.
- Bundled presence never overrides EPR0 source policy or EPR4 selection rules implicitly.

### Internal subblocks

#### EPR3-SB1 - Shipping owner and artifact-family decision

**Independently testable outcome:** One exact release artifact family owns the bundle, and all other packages reject it.

**Architecture:**

- Select lsp platform package, dedicated engine package, VSIX, or another explicit channel.
- Define consumers, install-relative layout, duplication policy, and platform coverage.
- Reconcile existing whitelist/never-package guards.

**Expected changes:**

- Amend release authority and package guard tests.
- Fail dispatch without owner authorization.

**Discriminating proof:**

- Bundle in unauthorized package/path fails.
- Authorized package absence/presence matches platform matrix exactly.

#### EPR3-SB2 - Pinned verified release input

**Independently testable outcome:** The release build consumes an exact engine version/platform artifact with independent origin/integrity receipt.

**Architecture:**

- Pin source version/digest/signature/provenance.
- Disallow latest/unversioned/nightly unless policy explicitly names it.
- Separate release-input acquisition from runtime managed acquisition.

**Expected changes:**

- Implement deterministic staging input process and cached release receipt.
- Reuse EPR1 validation before staging.

**Discriminating proof:**

- Input substitution/version drift fails reproducible build gate.
- No package scripts/unverified code execute during staging.

#### EPR3-SB3 - Platform staging and package manifest

**Independently testable outcome:** Each package contains only expected engine files at the exact runtime-relative layout.

**Architecture:**

- Stage executable/support files and bundle integrity manifest.
- Normalize permissions/executable bit/platform naming.
- Keep server and engine identities distinct.

**Expected changes:**

- Update per-platform build matrix and package manifests.
- Retain strict whitelist for every unrelated entry.

**Discriminating proof:**

- Installed archive listing equals declared manifest.
- Wrong platform/name/permission/layout fails package tests.

#### EPR3-SB4 - SBOM, license, provenance, and security response

**Independently testable outcome:** The shipped engine is legally and operationally traceable and revocable.

**Architecture:**

- Generate/include SBOM and license notices.
- Record upstream source/build/release provenance and security contact/update SLA.
- Define revocation/withdrawal/emergency release process.

**Expected changes:**

- Integrate artifact attestations and release metadata.
- Bind records into package receipt.

**Discriminating proof:**

- Missing/stale notices/provenance block release.
- Revoked input cannot be promoted or selected.

#### EPR3-SB5 - Installed-package end-to-end validation

**Independently testable outcome:** The actual consumer-installed package exposes a candidate that passes EPR1 and can handshake under EPR5 canaries.

**Architecture:**

- Install/extract each platform package in a clean sandbox.
- Locate via runtime relative path and validate manifest/digests/compatibility.
- Run bounded version/protocol handshake without project semantics.

**Expected changes:**

- Add CI per-platform package E2E.
- Test VSIX/editor packaging separately where authorized.

**Discriminating proof:**

- Pre-stage success cannot mask post-package corruption/omission.
- Installed layout matches runtime discovery exactly.

#### EPR3-SB6 - Size, update, rollback, and channel coexistence

**Independently testable outcome:** Bundle cost and lifecycle are explicit and do not create duplicate/conflicting engine copies accidentally.

**Architecture:**

- Measure compressed/uncompressed/install size and platform package impact.
- Define update synchronization with server/plugin compatibility.
- Retain/restore prior known-good package release via package manager, not mutable in-place bundle.

**Expected changes:**

- Add release thresholds and compatibility matrix.
- Define source precedence with project/editor/system/managed candidates in EPR4.

**Discriminating proof:**

- Size/compatibility thresholds and rollback canary pass.
- Duplicate package engines have explicit identities and selection law.

#### EPR3-SB7 - Unauthorized absence and zero-work proof

**Independently testable outcome:** When EPR3 is unopened or policy forbids bundle use, no build/runtime bundle path is silently active.

**Architecture:**

- Keep guards rejecting bundle-shaped artifacts in unauthorized channels.
- Generate opened/unopened release matrix.
- Prove runtime does not scan/hash bundle paths when source class disabled.

**Expected changes:**

- Add negative package/runtime tests.
- Remove stale tier claims when permanently unopened.

**Discriminating proof:**

- Unauthorized package injection fails.
- Disabled bundle source performs zero bundle filesystem work.

### Identity, invalidation, and publication

- Engine acquisition, resolution, selection, and activation are distinct authorities with distinct receipts; no stage infers success from a later stage.
- No executable is run before exact origin, compatibility, integrity, and trust validation succeeds.
- Unavailable, unauthorized, incompatible, corrupt, offline, cancelled, and operationally failed outcomes remain distinct and capability-visible.
- All caches/installations are content/version/platform scoped, race-safe, and fail closed on symlink/reparse/permission/ownership violations.
- Bundled artifacts are immutable release contents; runtime never updates them in place.
- A package receipt binds exact staged and final packaged bytes.
- Package source/provenance does not grant runtime selection priority outside EPR4 policy.

### Migration and cutover

- Decide and authorize package owner before weakening any existing guard.
- Stage one canary platform, validate package/install/runtime layout, then expand platform matrix.
- Expose source to EPR4 only after all declared platform rows pass.

### Consumers and unlocks

- When opened, supplies bundled verified candidates to EPR4.
- May provide offline floor under explicit EPR0 policy.
- Can remain permanently unopened with tier removed from public policy.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **EPR3-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or reproduced defect that existing evidence does not discriminate.
- **EPR3-AC2 — positive contract:** the named API/data boundary preserves exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **EPR3-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm; otherwise bind a terse not-applicable rationale.
- **EPR3-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, provider, filesystem, network, allocation, copy, or retained-candidate work; otherwise bind a terse not-applicable rationale.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; do not add implementation mirrors, duplicate permutations, or universal test quotas.
- Test homes: `crates/verter_session/tests/cases`, `crates/verter_protocol/tests`, `packages/typescript-plugin/src`, and the exact generated vertical fixture selected by this node.


### Pack-specific proof obligations

- **EPR3-AC-OWNER:** exact authorized package/platform matrix and negative rejection elsewhere.
- **EPR3-AC-REPRO:** pinned input and final package bytes/manifests are reproducible and verified.
- **EPR3-AC-INSTALLED:** clean installed package passes runtime discovery/EPR1 validation/handshake.
- **EPR3-AC-SUPPLY:** SBOM/license/provenance/revocation evidence is complete.
- **EPR3-AC-COST:** size/update/compatibility/rollback thresholds are accepted.
- **EPR3-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **EPR3-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **EPR3-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **EPR3-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Deletions and forbidden designs

- Delete blocked bundled-sidecar prose after explicit decision/authority transfer.
- Delete obsolete “never packaged” guards only for exact authorized channels; keep/rewrite negative guards for all others.
- Delete ad hoc manual bundle staging scripts.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

- Quietly relaxing package whitelists.
- Fetching latest/unverified engine during release.
- Shipping without SBOM/license/provenance or installed-package validation.
- Runtime mutable update of bundled bytes.
- Claiming offline floor on platforms/packages without bundle rows.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

- Delete or structurally reject every compatibility path that would preserve a second owner after cutover.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 800 production LOC, 8 production files, 2 related crates/packages.
- Mandatory rescope above 1500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, identity aliasing, or unauthorized executable work.
- Performance budget: when preflight identifies touched authority or a hot path, use the ratified replacement SLO and equivalent-work counters below; otherwise performance evidence is not applicable and no soak is invented solely to populate evidence.

- Runtime bundle discovery is disabled-zero-work or bounded relative-path validation; no recursive package scanning.
- Target ceiling: 800 production LOC, 8 production files, and 2 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Abort conditions

- Abort if no release artifact owner accepts size/license/update/security obligations.
- Abort if supported platform packaging cannot reproduce the exact validated runtime layout.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. `node roadmap/0.1.0-tama/tools/validate-program-dag.mjs --strict`
2. Run every final command in the bound `targeted-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

### Pack-specific verification inventory

1. Release input reproducibility/integrity/provenance tests.
1. Per-platform package listing/permission/layout/install/runtime validation.
1. Unauthorized package injection, disabled zero-work, size, compatibility, rollback, and revocation tests.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN evidence when applicable, the configured independent review profile, and the owning final gate on the squashed review candidate.

## Review and lower-severity findings

Apply `security-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `supply-chain-platform`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's predeclared row in `authority/state/implemented.toml` from `status = "pending"` to `status = "implemented"` with the planned squash commit message, approximate date with timezone, and optional pull-request number. The transitioned row is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.
