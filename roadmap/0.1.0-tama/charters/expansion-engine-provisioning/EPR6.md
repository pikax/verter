<!-- unified-charter-v2
id=EPR6
name=Offline, enterprise, and supply-chain conformance terminal
predecessors=EPR5,VIM1,PER0,BR0
phase=expansion
train=expansion.engine-provisioning
product=engine_provisioning
kind=terminal
semantic_role=delivery
class=successor
owner=expansion.engine-provisioning:explicit policy-controlled engine acquisition, resolution, and activation authority
conflict_domains=provider_lifecycle,performance_evidence,program_authority
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
optional=false
release_gating=product
external_requirements=
charter=charters/expansion-engine-provisioning/EPR6.md
size=S
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# EPR6 — Offline, enterprise, and supply-chain conformance terminal

Readiness is derived only from implemented-ledger rows for the node ancestors. Commit message, approximate timezone-bearing date, and optional PR are loose locator hints; the CLI performs no Git, GitHub, SHA, tree, ancestry, receipt, lease, or digest validation.

## Independently acceptable outcome

Close and promote engine provisioning only after policy, artifact validation, every opened source channel, deterministic resolution, atomic activation, offline/enterprise behavior, supply-chain evidence, capability truth, performance, teardown, and legacy deletion are proven on exact platform/topology rows. EPR6 adds no new acquisition or lifecycle behavior.

The current owner is **accepted EPR nodes plus residual tier logic, provider startup paths, package/discovery docs, manual status/capability claims, and unproven platform/offline topologies**. The final and sole owner is **one promoted engine-provisioning product receipt, exact source/platform/topology capability matrix, and structurally enforced acquisition-resolution-activation authority separation**.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_language/src`, `crates/verter_lsp`, `crates/verter_lsp/src`, `crates/verter_session`, `crates/verter_tsgo_api`, `crates/verter_tsgo_api/src/actor`, `crates/verter_type_runtime`, `crates/verter_type_runtime/src/tsserver`, `roadmap/0.1.0-tama`, `packages/typescript-plugin`, `crates/verter_audit/src`, `crates/verter_bench`, `crates/verter_compiler/src`, `crates/verter_napi/src`, `crates/verter_semantic/src`, `crates/verter_session/src`, `crates/verter_wasm/src`, `roadmap/0.1.0-tama/evidence`, `packages/benchmark`, `scripts`, `.claude/skills`, `AGENTS.md`, `crates/verter_identity`, `crates/verter_identity/src`, `crates/verter_protocol/src`.
- Pack production inventory:
- Rev11 EPR/VIM/PER0 authority and receipts
- toolchain resolver/activation/ProviderHub source and route inventories
- opened release packages/channels and installed-package evidence
- public engine status/capability/configuration docs
- legacy provisioning/editor architecture paths classified for deletion/relocation

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.
- Named API/data boundaries:
- `EngineProvisioningProductReceipt`
- `EngineProvisioningConformanceManifest` and stable platform/topology/source rows
- `EngineSourceCapabilitySnapshot` and `EngineResidualLedger`
- `EngineProvisioningRouteDeletionManifest`
- `SupplyChainClosureReceipt` and `LifecycleSoakReceipt`

## Exact predecessor contracts

- **EPR5:** implemented ledger row for “Engine activation epochs, health, and truthful capability publication”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **VIM1:** implemented ledger row for “Deterministic manifest compiler and conformance generator”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **PER0:** implemented ledger row for “Cache/backend identity, cancellation, budgets, and zero work”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **BR0:** implemented ledger row for “Post-L4 successor product promotion”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

### Binding architecture

- Terminal certifies exact policy-applicable sources/platforms/topologies; unopened optional channels must prove absence/zero-work, not fake success.
- Supply-chain closure is required for every opened executable channel.
- Resolution and activation route inventories must show no bypass or unvalidated path-only execution.
- Offline/deny-network/proxy/custom-CA/enterprise behavior is mandatory where policy exposes it.
- Capabilities/status are generated from exact conformance and active receipts, never manually promoted.
- Product receipt binds policy, trust roots, revocation, platform matrix, packages, implementation, evidence, review, deletion, and residual digests.
- Legacy docs are deleted/relocated; Git history is the archive.

### Internal subblocks

#### EPR6-SB1 - Conformance manifest and applicability closure

**Independently testable outcome:** Every policy-applicable source/platform/topology has an exact row; optional unopened channels have negative rows.

**Architecture:**

- Define rows for manual/project/system/editor/managed/bundle, OS/arch/libc, child/shared, online/offline/proxy, swap/crash/rollback.
- Mark required/opened/unopened/unsupported with owner.
- Bind exact toolchain/package fixtures.

**Expected changes:**

- Generate VIM rows/tests/receipts.
- Reject silent platform/topology omission.

**Discriminating proof:**

- Bijection/completeness guard passes.
- Public capability matrix equals applicable accepted rows.

#### EPR6-SB2 - Supply-chain and installed-artifact terminal

**Independently testable outcome:** Every opened executable channel has current origin/integrity/provenance/license/SBOM/revocation/installed validation.

**Architecture:**

- Validate EPR1 receipts, EPR2 acquisition or EPR3 package receipts.
- Check current trust roots/revocation and final installed bytes.
- Keep unopened sources absent.

**Expected changes:**

- Capture supply-chain closure receipt.
- Withdraw/reopen on stale/revoked input.

**Discriminating proof:**

- Byte/package/origin/revocation mutation invalidates terminal.
- No execution inventory entry lacks validation receipt.

#### EPR6-SB3 - Resolver/activation route deletion and no-bypass proof

**Independently testable outcome:** No code path selects/spawns/attaches an engine outside EPR4/EPR5.

**Architecture:**

- Generate call/path/symbol inventory for old tier/which/path/spawn/provider constructor routes.
- Delete routes/flags/helpers and add negative guards.
- Retain source/transport adapters only behind canonical authorities.

**Expected changes:**

- Perform bounded deletions in frozen candidate.
- Remove migration shims and stale tests/docs.

**Discriminating proof:**

- Planting path-only spawn/hidden source/first-found selection fails.
- Inventory has zero unexplained bypasses.

#### EPR6-SB4 - Offline/enterprise/security terminal

**Independently testable outcome:** Deny-network/offline/proxy/custom-CA/secret hygiene and failure remediation pass exact adversarial matrix.

**Architecture:**

- Run network attempt monitors, malicious origin/redirect/archive/path fixtures, secret logging scans.
- Validate policy behavior under corporate proxy/air-gap/read-only cache.
- Test integrity/trust/revocation loud failure.

**Expected changes:**

- Capture security review/receipt under security-3.
- Reopen EPR0-EPR5 for findings.

**Discriminating proof:**

- Forbidden/offline modes make zero network attempts.
- No secret/path leak or silent trust downgrade.

#### EPR6-SB5 - Lifecycle, capability, cancellation, and teardown terminal

**Independently testable outcome:** Selection/activation/swap/crash/restart/rollback/capabilities remain atomic and bounded across churn.

**Architecture:**

- Run concurrent project/provider/policy/edit transitions and long soak.
- Validate old epoch handle invalidation and stale-safe publication.
- Ensure teardown removes processes/transports/caches/resolve keys.

**Expected changes:**

- Capture lifecycle soak and capability receipts.
- Delete manual capability/status branches.

**Discriminating proof:**

- No stale/mixed epoch result or orphan process.
- Memory and resource counts plateau/release.

#### EPR6-SB6 - Performance and zero-work terminal

**Independently testable outcome:** Warm resolution/healthy operation and disabled/unopened sources meet exact work/allocation/latency/RSS thresholds.

**Architecture:**

- Measure source calls/stats/hashes/network/spawn/handshake/allocations/retained bytes.
- Compare fresh/incremental/warm/disabled/offline/project churn.
- Separate explicit acquisition cost from ordinary resolution.

**Expected changes:**

- Capture PER0 terminal receipt.
- Reopen owner for unexplained regression.

**Discriminating proof:**

- Warm healthy requests perform zero resolution/activation.
- Unopened/disabled sources perform zero filesystem/network/package work.

#### EPR6-SB7 - Legacy cleanup, product receipt, and promotion

**Independently testable outcome:** All provisioning facts are in Rev11/product docs, residual policy is honest, and promotion is immutable.

**Architecture:**

- Validate legacy disposition and relocate user/admin setup docs.
- Delete blocked future/tier/status architecture docs and obsolete tier numbering.
- Bind exact residual unsupported/platform rows.

**Expected changes:**

- Emit product receipt and permanent docs/route guards.
- Do not create archive directories.

**Discriminating proof:**

- No unclassified legacy path or live reference remains.
- Receipt invalidates on any authority/policy/trust/package/evidence change.

### Identity, invalidation, and publication

- Engine acquisition, resolution, selection, and activation are distinct authorities with distinct receipts; no stage infers success from a later stage.
- No executable is run before exact origin, compatibility, integrity, and trust validation succeeds.
- Unavailable, unauthorized, incompatible, corrupt, offline, cancelled, and operationally failed outcomes remain distinct and capability-visible.
- All caches/installations are content/version/platform scoped, race-safe, and fail closed on symlink/reparse/permission/ownership violations.
- EPR6 may not open an optional channel or add source/selection/activation behavior.
- Unopened EPR2/EPR3 are valid terminal states only when policy/capability/docs remove corresponding promises.
- Deletion/negative proof covers both source absence and structural inability to bypass validation/activation.

### Migration and cutover

- Run after EPR5 and every opened EPR2/EPR3 channel is accepted.
- Freeze candidate, generate complete conformance/route/source inventory, perform deletion/relocation, rerun security/performance/reviews.
- Reopen owning node on any defect.

### Consumers and unlocks

- Promotes exact engine provisioning/activation product.
- Provides stable truthful engine availability to CLI/LSP/language-service/diagnostics.
- Supports future engine flavors only through new policy/source/compatibility amendments.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **EPR6-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or reproduced defect that existing evidence does not discriminate.
- **EPR6-AC2 — positive contract:** the named API/data boundary preserves exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **EPR6-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm; otherwise bind a terse not-applicable rationale.
- **EPR6-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, provider, filesystem, network, allocation, copy, or retained-candidate work; otherwise bind a terse not-applicable rationale.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; do not add implementation mirrors, duplicate permutations, or universal test quotas.
- Test homes: `crates/verter_session/tests/cases`, `crates/verter_protocol/tests`, `packages/typescript-plugin/src`, and the exact generated vertical fixture selected by this node.


### Pack-specific proof obligations

- **EPR6-AC-MATRIX:** every policy-applicable source/platform/topology is accepted, unsupported, or unopened with exact evidence.
- **EPR6-AC-SUPPLY:** every opened executable channel has current installed-byte/origin/integrity/provenance/revocation closure.
- **EPR6-AC-NO-BYPASS:** route inventory proves all selection/activation flows use EPR4/EPR5.
- **EPR6-AC-SECURITY:** offline/proxy/air-gap/adversarial/secret/no-hidden-network matrix passes.
- **EPR6-AC-LIFECYCLE:** swap/crash/restart/rollback/capability/teardown soak is stale-safe and leak-free.
- **EPR6-AC-PERF:** warm/disabled/unopened equivalent-work and RSS thresholds pass.
- **EPR6-AC-HONEST:** public policy/capability/docs exactly match opened and certified scope.
- **EPR6-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **EPR6-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **EPR6-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **EPR6-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Deletions and forbidden designs

- Delete displaced tier/discovery/selection/spawn/provider constructor/capability/status routes and legacy docs named by manifests.
- Delete stale policy claims for unopened channels.
- Delete temporary migration/config compatibility shims.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

- Implementing missing channel/lifecycle behavior in terminal.
- Claiming offline/managed/bundled support without accepted applicable rows.
- Retaining path-only/first-found/unvalidated spawn fallback.
- Accepting stale supply-chain/security/performance evidence.
- Archiving legacy architecture in another folder.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

- Delete or structurally reject every compatibility path that would preserve a second owner after cutover.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 300 production LOC, 3 production files, 1 related crates/packages.
- Mandatory rescope above 1500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, identity aliasing, or unauthorized executable work.
- Performance budget: when preflight identifies touched authority or a hot path, use the ratified replacement SLO and equivalent-work counters below; otherwise performance evidence is not applicable and no soak is invented solely to populate evidence.

- Terminal thresholds distinguish explicit acquisition side effects from ordinary resolver/healthy-request hot paths and require zero hidden work for disabled/unopened sources.
- Target ceiling: 300 production LOC, 3 production files, and 1 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Abort conditions

- Abort if any executable path lacks current validation/supply-chain/activation receipt.
- Abort if optional unopened source remains promised by public policy/docs.
- Abort if route/source inventory is incomplete or security review is not clean.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. `node roadmap/0.1.0-tama/tools/validate-program-dag.mjs --strict`
2. Run every final command in the bound `targeted-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

### Pack-specific verification inventory

1. Full VIM source/platform/topology and installed artifact matrix.
1. No-bypass route inventory/mutation guards and complete security-3 review.
1. Offline/proxy/adversarial/lifecycle/capability/cancel/teardown/performance/RSS terminal suites and immutable product receipt validation.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN evidence when applicable, the configured independent review profile, and the owning final gate on the squashed review candidate.

## Review and lower-severity findings

Apply `security-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `supply-chain-platform`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's predeclared row in `authority/state/implemented.toml` from `status = "pending"` to `status = "implemented"` with the planned squash commit message, approximate date with timezone, and optional pull-request number. The transitioned row is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.
