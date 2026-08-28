<!-- unified-charter-v2
id=EPR0
name=External engine provisioning policy and trust constitution
predecessors=UAK1,CFG0,H2,PUB0,TCM4
conditional_predecessors=
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
source_refs=source:successor-dag-amendment.md:L1,source:legacy-arch-reconciliation.md:L1
external_requirements=
activation_gate=ORC0
charter=charters/expansion-engine-provisioning/EPR0.md
size=M
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# EPR0 — External engine provisioning policy and trust constitution

Authority state is derived at dispatch. The canonical CLI validates the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

## Independently acceptable outcome

Ratify an explicit external-engine provisioning and trust constitution. The policy may authorize project-local, system, editor-shared, managed-download, bundled-sidecar, or no automatic acquisition. Network and bundled channels remain closed until separately authorized; missing engines produce typed outcomes rather than hidden fallback.

The current owner is **partially implemented discovery tiers, blocked future documents, release-package invariants, environment overrides, editor sharing, and implicit product assumptions**. The final and sole owner is **one captured EngineProvisioningPolicy with explicit source authorization, trust/update/offline/enterprise law, typed outcomes, and separate acquisition/resolution/activation authorities**.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_identity`, `crates/verter_protocol`, `crates/verter_session`, `crates/verter_tsgo_api`, `crates/verter_type_runtime`.
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

- **UAK1:** exact current receipt ID and digest for “Universal-tooling constitution and program split”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **CFG0:** exact current receipt ID and digest for “Declarative Verter and captured ecosystem configuration”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **H2:** exact current receipt ID and digest for “Project-scoped ProviderHub bindings”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **PUB0:** exact current receipt ID and digest for “Versioned public request/result and capability truth”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **TCM4:** exact current receipt ID and digest for “Atomic activation and deletion”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

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

- **EPR0-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or reproduced defect that existing evidence does not discriminate.
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

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

- Silent network download, update, telemetry, or registry probe.
- Executing an unverified candidate because it exists on PATH/project/editor/cache.
- Treating bundle/download authorization as implied by prior tier numbering.
- Collapsing integrity/trust failure to not-found fallback.
- Secrets in logs, digests, receipts, or public DTOs.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

- Delete or structurally reject every compatibility path that would preserve a second owner after cutover.
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

1. `node docs/arch/refactor/rev11/tools/validate-program-dag.mjs --strict`
2. Run every final command in the bound `docs-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

### Pack-specific verification inventory

1. Policy/source/external-requirement/source-coverage validation.
1. Negative no-network/no-bundle/no-unverified-execution architecture tests.
1. Typed outcome/capability/configuration schema and secret-redaction tests.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Review and lower-severity findings

Apply `security-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `supply-chain-platform`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the exact candidate tree, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact frozen worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired round handle; the complete gate command list; 3 fresh distinct harness review tasks for exactly `adversarial`, `conformance`, `supply-chain-platform`, deterministic low|medium|high effort and exact author/task/agent/provider/model bindings; immutable-review-worktree and cleanup policy; and the required terse report-back schema. These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:successor-dag-amendment.md:L1`
- `source:legacy-arch-reconciliation.md:L1`

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-LEGACY-EPR-POLICY-001

- Kind: `requirement`
- Source: `legacy-arch-reconciliation.md:145-150`
- Applicability: `EPR0`
- Exact text SHA-256: `205a061f7959820aa50934387fb03bf4975026688264a97f25f0e89cca310d93`

~~~~markdown
### EPR-POLICY-001 — Acquisition and bundling are explicit policy

- Automatic network acquisition and bundled distribution are security/product decisions.
- A valid end state may forbid both.
- Source classes, order, authorization, trust, update, rollback, offline, proxy, enterprise, and privacy behavior are captured policy.
- Targets: `EPR0`.
~~~~

### SRC-LEGACY-EPR-BUNDLE-001

- Kind: `requirement`
- Source: `legacy-arch-reconciliation.md:166-171`
- Applicability: `EPR0`, `EPR3`
- Exact text SHA-256: `4053dc931f7487d7d6a804dac7636b9d3d0ff69e9d73d7ab89d21fbdd66c4d03`

~~~~markdown
### EPR-BUNDLE-001 — Explicit shipping owner

- Bundled engine bytes must belong to one named package/platform matrix with pinned input, manifest, SBOM, license, provenance, installed-package validation, size/update/rollback policy, and negative rejection elsewhere.
- Existing “never package” guards are changed only through explicit authorization.
- Targets: `EPR0`, optional `EPR3`.
- Source: `docs/arch/future/engine-provisioning-bundled-sidecar-and-shipping-channel.md`, blob `8fdd6d881db77615e09b31f51318fc0254bb27dd`.
~~~~

### SRC-LEGACY-EPR-OFFLINE-001

- Kind: `requirement`
- Source: `legacy-arch-reconciliation.md:188-193`
- Applicability: `EPR0`, `EPR2`, `EPR6`
- Exact text SHA-256: `12a3f963d95695f053129b2ce845b2ca4c3cd78a8e69653fd73923ce716fbb05`

~~~~markdown
### EPR-OFFLINE-001 — No hidden network and truthful status

- Forbidden/offline/air-gapped policy makes zero DNS/socket attempts.
- Proxy/custom CA inputs are explicit and secrets are not persisted/logged.
- No engine produces typed NeedInputs/unavailable capability rather than hidden fallback.
- Targets: `EPR0`, `EPR2`, `EPR6`.
~~~~

### SRC-LEGACY-TRANSFER-8FDD6D881DB7

- Kind: `requirement`
- Source: `legacy-architecture-transfers.md:68-73`
- Applicability: `EPR0`, `EPR1`, `EPR3`, `EPR4`, `EPR6`
- Exact text SHA-256: `7a95fcce3e5b3b258d1207ddd262c50b579888081ca6831479bf3f685fc38290`

~~~~markdown
### LEGACY-TRANSFER-8FDD6D881DB7

- Original path: `docs/arch/future/engine-provisioning-bundled-sidecar-and-shipping-channel.md`; Git blob: `8fdd6d881db77615e09b31f51318fc0254bb27dd`; exact source SHA-256: `e9c51872088b2637bc69a0e1c45a49f907dede39c3322acfb1857771be8a42d9`.
- Exact retained source: `sources/legacy-architecture-transfers/future/engine-provisioning-bundled-sidecar-and-shipping-channel.md`.
- Applicable authority: `EPR0`, `EPR1`, `EPR3`, `EPR4`, `EPR6`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-CD6618EFB8E1

- Kind: `requirement`
- Source: `legacy-architecture-transfers.md:75-80`
- Applicability: `EPR0`, `EPR1`, `EPR2`, `EPR4`, `EPR6`
- Exact text SHA-256: `23d0a0a1d5c5e65e7e981f082e836dbd34814e659995a2b5b9aebd7fb8ca37f8`

~~~~markdown
### LEGACY-TRANSFER-CD6618EFB8E1

- Original path: `docs/arch/future/engine-provisioning-download-tier.md`; Git blob: `cd6618efb8e1a586caa6842874a1ce5b128469af`; exact source SHA-256: `3c932ef124f15e4f45d66833b7548bf2dd7809c732416b69b1beb106acba41ab`.
- Exact retained source: `sources/legacy-architecture-transfers/future/engine-provisioning-download-tier.md`.
- Applicable authority: `EPR0`, `EPR1`, `EPR2`, `EPR4`, `EPR6`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~
