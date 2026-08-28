<!-- unified-charter-v2
id=EPR5
name=Engine activation epochs, health, and truthful capability publication
phase=expansion
train=expansion.engine-provisioning
product=engine_provisioning
kind=convergence
semantic_role=delivery
class=successor
predecessors=EPR4,H3,PUB0,COX0
conditional_predecessors=
owner=expansion.engine-provisioning:explicit policy-controlled engine acquisition, resolution, and activation authority
conflict_domains=provider_lifecycle,lsp_publication,public_protocol,capability_catalog
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
charter=charters/expansion-engine-provisioning/EPR5.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# EPR5 - Engine activation epochs, health, and truthful capability publication

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation and release gates, external authorizations, source atom digests, conflict-domain admission, and the landing-frozen candidate before mutation.

The internal subblocks below are binding decomposition and review checkpoints. They do not receive independent dispatch, leases, receipts, or deletion ownership unless the pre-scope architect proves that one subblock is independently acceptable or the block crosses a mandatory rescope trigger. In that case, amend the DAG before production mutation rather than treating a train-sized subblock as an implementation checklist.

## Independently acceptable outcome

Implement engine activation as a separate atomic lifecycle: revalidate the selected artifact handoff, spawn under bounded owner-affine control, perform version/protocol/capability handshake and health checks, bind a new ProviderEpoch into ProviderHub atomically, publish truthful capabilities only after success, and handle swap/restart/crash/rollback without stale mixed service.

The current owner is **source-specific spawn helpers, provider constructors, LSP initialize capability assumptions, partial shared/editor attach logic, and mixed discovery/activation failures**. The final and sole owner is **one EngineActivator and project-scoped activation state machine with exact receipts, deadlines, health, ProviderEpoch, atomic ProviderHub binding, and capability publication**.

## Architectural role and end state

EPR5 ensures that a selected executable is not treated as available until it has successfully handshaken and become the exact active project binding. It owns operational lifecycle, not artifact selection or semantic provider implementation.

## Expected production surfaces

- `crates/verter_session` ProviderHub/project service graph
- `crates/verter_type_runtime` provider process/transport adapters
- `crates/verter_tsgo_api` actor/process lifecycle where applicable
- `crates/verter_lsp` capability/status publication through shared host
- `crates/verter_protocol` activation/status/receipt schemas

These are expected ownership surfaces, not permission to touch all listed paths. The dispatch packet must bind exact path and symbol sets after reconciling the live tree. A newly discovered owner or unrelated package requires an amendment or rescope.

## Named APIs and data boundaries

- `EngineActivationRequest`, `EngineActivationBasis`, and `EngineActivationPlan`
- `EngineActivator`, `EngineProcessHandle`, and owner-affine lifecycle commands
- `EngineHandshake`, `EngineCapabilitySet`, and `EngineHealthState`
- `EngineActivationReceipt`, `ProviderEpoch`, and `ProviderBinding`
- `EngineSwapPlan`, `EngineRollbackPlan`, and `EngineDeactivationReceipt`
- `EngineActivationFailure::{StaleSelection, Spawn, Handshake, Protocol, Capability, Health, Timeout, Crash, Cancelled}`

## Exact predecessor contracts

- **EPR4:** consume deterministic selection plan and exact validated artifact receipt.
- **H3:** consume stale-safe publication/supersession semantics.
- **PUB0:** consume typed public outcomes/capability truth.
- **COX0:** consume dynamic profile capability registration/withdrawal and zero-work modes.

External custody: none beyond the package activation boundary.

## Binding architecture

- Activation revalidates the exact artifact/path facts needed to close TOCTOU before spawn.
- Spawn occurs under bounded process/resource/deadline/cancellation policy and uses owner-affine lifecycle commands.
- Availability requires successful protocol/version/capability handshake; process existence alone is insufficient.
- ProviderHub binding and ProviderEpoch publication are atomic; requests never see a half-initialized or mixed old/new provider.
- Capabilities are derived from the active handshake plus profile/coexistence policy and published only after binding success.
- Swap/restart/rollback keeps old binding until new one is healthy, or withdraws truthfully when no valid binding remains.
- Crash/hang/deadline failures cancel affected requests, invalidate epoch-bound handles, and never reuse stale results.
- Shared/editor-attached topology follows the same applied-snapshot/epoch/health receipt law as child processes.

## Internal subblocks

### EPR5-SB1 - Activation request and stale-selection revalidation

**Independently testable outcome:** Activation binds exact selected artifact, policy, project/profile, and current source/provider requirement basis.

**Architecture:**

- Define request/plan identity and revalidate EPR1/EPR4 receipt/path facts.
- Reject stale selection/policy/revocation/project requirement changes.
- Separate activate, attach, swap, restart, deactivate.

**Expected changes:**

- Add activation coordinator input and TOCTOU checks.
- Remove path-only spawn entry points.

**Discriminating proof:**

- Replacing/revoking artifact between selection/spawn fails before execution.
- Same exact plan is deterministic and singleflight.

### EPR5-SB2 - Bounded process/transport startup

**Independently testable outcome:** Engine startup is cancellable/deadline-bounded, owner-affine, and leaks no orphan process/transport.

**Architecture:**

- Spawn with exact executable/args/env/workdir/sandbox policy.
- Bound stdout/stderr/message sizes and startup resources.
- Support child and approved shared/editor transport adapters.

**Expected changes:**

- Centralize startup/cleanup under ProviderHub lifecycle.
- Remove source-specific unmanaged spawn helpers.

**Discriminating proof:**

- Timeout/cancel/spawn failure leaves no active binding/orphan process.
- Argument/env/path injection and secret logging tests fail closed.

### EPR5-SB3 - Handshake, compatibility, and capability verification

**Independently testable outcome:** The running engine proves exact identity/protocol/features before it can serve requests.

**Architecture:**

- Query version/build/protocol/capabilities and compare with selected descriptor/requirements.
- Detect wrong binary/wrapper/protocol downgrade.
- Capture handshake evidence in activation receipt.

**Expected changes:**

- Implement provider-neutral handshake result and adapter mappings.
- Refuse capability lies/unknown required features.

**Discriminating proof:**

- Wrong-version/protocol/capability mutation kills candidate before binding.
- Handshake receipt matches selected artifact identity.

### EPR5-SB4 - Atomic ProviderHub binding and epoch publication

**Independently testable outcome:** A healthy engine becomes visible in one atomic project-scoped state transition.

**Architecture:**

- Create new ProviderEpoch and immutable binding after handshake.
- Swap binding pointer/service graph atomically and invalidate old epoch handles.
- Coordinate in-flight request settlement/cancellation.

**Expected changes:**

- Integrate with H2 ProviderHub and H3 publication.
- Delete global mutable provider/current-engine fields.

**Discriminating proof:**

- Failure injection yields old or new complete binding, never half state.
- Requests/results/resolve keys from old epoch fail closed after swap.

### EPR5-SB5 - Health, crash, hang, restart, and rollback

**Independently testable outcome:** Operational degradation is detected and handled without stale publication or retry storms.

**Architecture:**

- Define Starting/Healthy/Degraded/Failed/Stopping states and heartbeat/request-deadline signals.
- Bound restart/backoff under explicit policy; no infinite/sleep-poll correctness loop.
- Rollback to prior validated selection/binding only under policy.

**Expected changes:**

- Implement health supervisor/audit and deterministic transition table.
- Cancel affected flights and withdraw capabilities when unavailable.

**Discriminating proof:**

- Crash/hang/restart/rollback race matrix publishes no stale result.
- Repeated failure is bounded and capability status remains truthful.

### EPR5-SB6 - Truthful capability/status publication

**Independently testable outcome:** LSP/CLI/public surfaces advertise only capabilities actually available from active engine plus profile/coexistence policy.

**Architecture:**

- Compose handshake capabilities, certified native replacements, profile masks, and client participation.
- Register/unregister dynamically and clear owned stale diagnostics/results.
- Expose exact source/version/status/rejection/remediation safely.

**Expected changes:**

- Route capability generation through PUB0/COX0.
- Remove initialize-time assumptions based only on configured provider mode.

**Discriminating proof:**

- No active engine means provider capabilities false/NeedInputs, not dishonest true.
- Engine/native family transitions withdraw only displaced capabilities.

### EPR5-SB7 - Activation cache/work and lifecycle memory proof

**Independently testable outcome:** Repeated activation/healthy use avoids redundant validation/spawn while teardown fully releases resources.

**Architecture:**

- Singleflight same activation plan; reuse only healthy exact binding.
- Count spawn/handshake/restart/swap/requests/orphans/retained handles.
- Release process, transport, snapshots, resolve keys, caches on project close/policy change.

**Expected changes:**

- Add PER0 lifecycle receipts and soak tests.
- Ensure resolution does not rerun inside every request.

**Discriminating proof:**

- Warm healthy requests perform zero resolution/activation work.
- Long churn/project teardown leaves no orphan processes or retained growth.

## Data, identity, invalidation, and publication laws

- Engine acquisition, resolution, selection, and activation are distinct authorities with distinct receipts; no stage infers success from a later stage.
- No executable is run before exact origin, compatibility, integrity, and trust validation succeeds.
- Unavailable, unauthorized, incompatible, corrupt, offline, cancelled, and operationally failed outcomes remain distinct and capability-visible.
- All caches/installations are content/version/platform scoped, race-safe, and fail closed on symlink/reparse/permission/ownership violations.
- ProviderEpoch is the identity of an applied healthy binding, not a configured mode or discovered path.
- Activation receipts bind selected artifact/validation/handshake/policy/project/profile facts.
- Request/result/resolve caches and handles are epoch-scoped and invalid after swap/deactivation.

## Migration and cutover

- Wrap current provider startup behind activation state machine while keeping current source selections.
- Migrate one topology at a time: child process, project-local/system, editor-shared, managed/bundled when opened.
- Move capability publication after atomic binding and delete old constructors/flags.

## Deletions

- Delete mixed discovery-spawn helpers, global provider state, initialize-time capability guesses, and unbounded restart/poll loops.
- Delete epoch-less resolve/request handles and stale-result reuse.
- Delete source-specific activation semantics after adapter migration.

Deletion ownership is exact. This block may delete only the routes and artifacts named above after their replacement is proven on the same candidate. Neighboring legacy deletion remains with its owning node.

## Forbidden designs

- Publishing capability before successful handshake/binding.
- Spawning from path without current validation/selection receipt.
- Half-swapped provider binding or mixing old/new epoch results.
- Infinite retry, sleep/poll readiness as correctness, orphan process/transport.
- Treating process alive as healthy/compatible.

The program-wide prohibitions also apply: no dual-running semantic authority, compatibility fallback, string or regex semantic recovery, test-only production bypass, sleep or polling readiness, resource-capacity predecessor, unqualified cache identity, or hidden whole-workspace eager work.

## Acceptance IDs and discriminating proof

- **EPR5-AC-REVALIDATE:** path/artifact/policy/revocation drift between selection/spawn is rejected.
- **EPR5-AC-HANDSHAKE:** wrong version/protocol/capability never binds.
- **EPR5-AC-ATOMIC:** swap/failure injection proves old-or-new ProviderEpoch only.
- **EPR5-AC-HEALTH:** crash/hang/restart/rollback is bounded and stale-safe.
- **EPR5-AC-CAPABILITY:** public capability/status exactly reflects active handshake plus native/profile/coexistence authority.
- **EPR5-AC-TEARDOWN:** no orphan processes/transports or retained epoch handles after churn/close.
- **EPR5-AC-SOLE:** a planted displaced authority or duplicate route is rejected by a static or runtime guard.
- **EPR5-AC-CONTRACT:** the named APIs, identities, outcomes, and provenance fields are exact, deterministic, and complete for this block.
- **EPR5-AC-INCREMENTAL:** incremental execution equals fresh execution on the same basis; cancelled, stale, partial, or NeedInputs outcomes are never warm-admitted as complete.
- **EPR5-AC-WORK:** equivalent-work counters prove no hidden parse, resolve, index walk, provider call, allocation, copy, or retained candidate beyond the declared demand.

## Performance and bounded work

- Healthy active requests perform zero resolution/activation work; lifecycle overhead is bounded to explicit transitions and monitored under PER0.
- Target ceiling: 800 production LOC, 8 production files, and 2 related packages.
- No wall-time claim is accepted without equivalent-work counters and allocation/RSS evidence for the same semantic work.
- After warmup, 100 identical requests must show no unbounded retained-byte growth and no repeated provider or filesystem work unless the request explicitly demands it.

## Mandatory rescope and abort conditions

- Abort if a provider topology cannot expose exact handshake/epoch/applied-snapshot evidence.
- Abort if atomic swap/withdrawal cannot be guaranteed for a supported public surface.
- Rescope before mutation above 1500 production LOC, 12 files, or 3 unrelated packages.
- Rescope when a public/wire change, concurrency/lifetime change, and semantic algorithm change would otherwise land in one review context.
- Abort on any wrong-complete result, stale publication, provenance loss, identity aliasing, silent fallback, or inability to name the sole final owner.

## Targeted verification

1. TOCTOU/revalidation/spawn/argument/environment/timeout/cancel tests.
1. Handshake/version/protocol/capability and atomic swap/epoch invalidation matrix.
1. Crash/hang/restart/rollback/capability/publication/project teardown/soak/performance tests.

The canonical gate profile remains authoritative. Targeted success is iteration evidence only. Final acceptance requires fresh RED/GREEN mutation evidence, the exact gate receipt, and the configured independent review profile on the landing-frozen tree.

## Consumers and unlocks

- Unlocks EPR6 terminal conformance.
- Provides exact active engine binding/status to TypeScript observation, language service, CLI, and diagnostics.
- Supports native replacement by truthful capability composition rather than all-or-nothing provider shutdown.

## Source reconciliation

- H2/H3/TCM/provider lifecycle contracts and current engine spawn/attach behavior.
- Legacy editor/shared provider and provisioning documents.

Durable clauses are transferred as digest-bound requirement atoms. Historical path archaeology, obsolete branches, and implementation journals are not copied into the charter. Git history remains the archive.
