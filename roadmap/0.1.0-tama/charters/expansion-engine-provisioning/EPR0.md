<!-- unified-charter-v2
id=EPR0
name=External engine provisioning policy and trust constitution
predecessors=UAK1,CFG0,H2,PUB0,TCM4
phase=expansion
train=expansion.engine-provisioning
product=engine_provisioning
kind=constitution
semantic_role=delivery
class=successor
owner=expansion.engine-provisioning:explicit policy-controlled engine acquisition, resolution, and activation authority
conflict_domains=provider_lifecycle,public_protocol,program_authority
resource_class=docs-light
gate_profile=docs-domain
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
optional=false
release_gating=none
external_requirements=
charter=charters/expansion-engine-provisioning/EPR0.md
size=M
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# EPR0 — External engine provisioning policy and trust constitution

Readiness is derived only from implemented-ledger rows for the node ancestors. Commit message, approximate timezone-bearing date, and optional PR are loose locator hints; the CLI performs no Git, GitHub, SHA, tree, ancestry, receipt, lease, or digest validation.

## Independently acceptable outcome

Ratify an explicit external-engine provisioning and trust constitution. The policy may authorize project-local, system, editor-shared, managed-download, bundled-sidecar, or no automatic acquisition. Network and bundled channels remain closed until separately authorized; missing engines produce typed outcomes rather than hidden fallback.

The current owner is **partially implemented discovery tiers, blocked future documents, release-package invariants, environment overrides, editor sharing, and implicit product assumptions**. The final and sole owner is **one captured EngineProvisioningPolicy with explicit source authorization, trust/update/offline/enterprise law, typed outcomes, and separate acquisition/resolution/activation authorities**.

## Concrete surfaces and APIs

- Future production-owner inventory (read-only in this contract): `crates/verter_identity`, `crates/verter_protocol`, `crates/verter_session`, `crates/verter_tsgo_api`, `crates/verter_type_runtime`.
- Pack production inventory:
- Rev11 authority/catalogs and declarative configuration under CFG0
- `crates/verter_identity` and `crates/verter_protocol` for policy/source/outcome identities
- `crates/verter_session`/ProviderHub future policy consumption
- `crates/verter_tsgo_api`/`crates/verter_type_runtime` as implementation consumers only
- release/packaging/editor products as conditionally authorized channels

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.
- Named API/data boundaries:
- `EngineProvisioningPolicy`, `EngineSourcePolicy`, and `EngineSourceKind`
- `EngineAcquisitionPermission::{Forbidden, ManualOnly, Allowed}`
- `EngineUpdatePolicy`, `EngineOfflinePolicy`, `EngineProxyPolicy`, and `EngineRollbackPolicy`
- `EngineNeed`, `EngineRequirement`, and `EngineProvisioningOutcome`
- `TrustedEngineOrigin`, `EngineTrustRootId`, and policy epoch
- `EngineCapabilityState` and truthful source/availability reporting

## Exact predecessor contracts

- **UAK1:** implemented ledger row for “Universal-tooling constitution and program split”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **CFG0:** implemented ledger row for “Declarative Verter and captured ecosystem configuration”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **H2:** implemented ledger row for “Project-scoped ProviderHub bindings”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **PUB0:** implemented ledger row for “Versioned public request/result and capability truth”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **TCM4:** implemented ledger row for “Atomic activation and deletion”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

### Binding architecture

- Automatic download and bundled distribution are product/security choices, not implementation defaults.
- A valid policy may forbid both EPR2 and EPR3 while still requiring deterministic manual/project/system/editor discovery.
- Source order and authorization are explicit data; environment overrides cannot silently bypass forbidden source classes.
- Network behavior is opt-in, observable, proxy/enterprise compatible, cancellable, and absent from ordinary resolution when disallowed.
- Bundled artifacts require explicit shipping ownership, license/provenance/size/update policy, and platform coverage.
- Missing engine yields NeedInputs/Unsupported/Unavailable according to context and never a fake “provider off but success” result.
- Native checker/language-service certified families may reduce engine demand, but do not change provisioning policy implicitly.

### Internal subblocks

#### EPR0-SB1 - Engine source taxonomy and authorization matrix

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

#### EPR0-SB2 - Network and managed acquisition policy

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

#### EPR0-SB3 - Bundled distribution policy

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

#### EPR0-SB4 - Offline, enterprise, and privacy behavior

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

#### EPR0-SB5 - Version/update/rollback constitution

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

#### EPR0-SB6 - Typed outcomes and capability truth

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

#### EPR0-SB7 - Legacy decision/source reconciliation

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

### Identity, invalidation, and publication

- Engine acquisition, resolution, selection, and activation are distinct authorities with distinct receipts; no stage infers success from a later stage.
- No executable is run before exact origin, compatibility, integrity, and trust validation succeeds.
- Unavailable, unauthorized, incompatible, corrupt, offline, cancelled, and operationally failed outcomes remain distinct and capability-visible.
- All caches/installations are content/version/platform scoped, race-safe, and fail closed on symlink/reparse/permission/ownership violations.
- Policy snapshots are immutable, captured, and part of resolution identity.
- Secrets/credentials are runtime inputs and never stored in policy receipts or logs.
- A policy change cancels/supersedes in-flight acquisition/resolution/activation work.

### Migration and cutover

- Land policy with current usable sources represented exactly and network/bundle forbidden unless explicitly authorized.
- Do not implement EPR2/EPR3 in this block.
- Replace prose tier assumptions with generated source policy/status tables.

### Consumers and unlocks

- Unlocks EPR1 and optional EPR2/EPR3.
- Provides exact policy consumed by EPR4 resolution and EPR5 activation.
- Allows a valid no-download/no-bundle end state.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **EPR0-AC1 — sole-owner outcome:** the contract artifacts establish the named ownership rules and inventory every displaced production route with its later production-capable deletion owner; this zero-production node performs no runtime migration. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or reproduced defect that existing evidence does not discriminate.
- **EPR0-AC2 — positive contract:** the named API/data boundary preserves exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **EPR0-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm; otherwise bind a terse not-applicable rationale.
- **EPR0-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, provider, filesystem, network, allocation, copy, or retained-candidate work; otherwise bind a terse not-applicable rationale.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; do not add implementation mirrors, duplicate permutations, or universal test quotas.
- Test homes: `crates/verter_session/tests/cases`, `crates/verter_protocol/tests`, `packages/typescript-plugin/src`, and the exact generated vertical fixture selected by this node.


### Pack-specific proof obligations

- **EPR0-AC-POLICY:** every source/network/bundle/update/offline state has exact authorization and owner.
- **EPR0-AC-ZERO-NETWORK:** forbidden/manual-only policy proves zero automatic network attempts.
- **EPR0-AC-OUTCOMES:** all absence/rejection/failure states remain typed and capability-truthful.
- **EPR0-AC-LEGACY:** download/bundle docs and packaging invariants have complete source disposition.
- **EPR0-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **EPR0-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **EPR0-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **EPR0-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Deletions and forbidden designs

- Delete orphan blocked download/bundle architecture docs after source transfer.
- Delete undocumented tier ordering and implicit automatic acquisition claims.
- Delete conflicting packaging invariants only through explicit EPR0/EPR3 authorization, never incidentally.

This contract may remove superseded authority prose after preserving its durable clauses. Every production deletion listed here is an inventory obligation: bind it to an exact later production-capable owner and acceptance criterion before completing this contract. It authorizes no runtime route deletion.

- Silent network download, update, telemetry, or registry probe.
- Executing an unverified candidate because it exists on PATH/project/editor/cache.
- Treating bundle/download authorization as implied by prior tier numbering.
- Collapsing integrity/trust failure to not-found fallback.
- Secrets in logs, digests, receipts, or public DTOs.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

- Inventory and assign a later deletion owner for every compatibility path that would preserve a second production owner after cutover.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 0 production LOC, 0 production files, 0 related crates/packages.
- Mandatory rescope above 1500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, identity aliasing, or unauthorized executable work.
- Performance budget: when preflight identifies touched authority or a hot path, use the ratified replacement SLO and equivalent-work counters below; otherwise performance evidence is not applicable and no soak is invented solely to populate evidence.

- Policy evaluation and source filtering are allocation-free/bounded after snapshot construction and perform no filesystem/network work themselves.
- Target ceiling: 0 production LOC, 0 production files, and 0 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Abort conditions

- Abort if the maintainer decision on network or bundling is inferred rather than explicit.
- Abort if enterprise/offline behavior or executable trust root cannot be specified.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. `node roadmap/0.1.0-tama/tools/validate-program-dag.mjs --strict`
2. Run every final command in the bound `docs-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

### Pack-specific verification inventory

1. Policy/source/external-requirement/source-coverage validation.
1. Negative no-network/no-bundle/no-unverified-execution architecture tests.
1. Typed outcome/capability/configuration schema and secret-redaction tests.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN evidence when applicable, the configured independent review profile, and the owning final gate on the squashed review candidate.

## Review and lower-severity findings

Apply `security-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `supply-chain-platform`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's predeclared row in `authority/state/implemented.toml` from `status = "pending"` to `status = "implemented"` with the planned squash commit message, approximate date with timezone, and optional pull-request number. The transitioned row is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.
