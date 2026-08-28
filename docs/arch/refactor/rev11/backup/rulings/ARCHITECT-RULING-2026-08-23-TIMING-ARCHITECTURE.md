---
ruling_id: "TIMING-ARCHITECTURE-2026-08-23"
type: "architecture-ruling"
date: "2026-08-23"
date_source: "in-document (**Date:** 2026-08-23)"
binds: ["program-wide timing architecture (production and tests)"]
source_file: "ARCHITECT-RULING-2026-08-23-TIMING-ARCHITECTURE.md"
summary: "The correct timing architecture is causal progress + semantic virtual time + an independent real watchdog, selected per surface — not causal assertion versus virtual clock. Standardize Tokio-owned async code on tokio::time::Instant and Tokio timers with paused time in tests; a custom clock trait is only for a synchronous domain with genuine semantic time that cannot use Tokio (the scheduler does not need one). A deterministic executor is not the primary mechanism because it cannot control parking_lot threads, Rayon pools, Cargo, tsserver, tsgo, or OS scheduling. An event log is an observer, never a second state machine; durable receipts and generations remain the production authority. Timing values belong to their semantic domain; the composition root must not flatten them into one global duration table. Keep block/deterministic-tests; apply four corrections before extending the conversion pattern, including serializing every real-process shared-provider test. The shared-overlay 20-second fallback bound needs its own architecture ruling because it changes serving behaviour."
supersedes: []
superseded_by: []
contradicts: []
notes: "Transcribes the 2026-08-23 architecture consult (codex architect, xhigh, read-only) under this program's delegated authority. The consult answered ~/.claude/briefs/rev11/verify/time-usage-consult.md; its conclusions are the decision. Maintainer direction recorded 2026-08-23 belongs in this ruling's context: time-sensitive checks get a proper refactor; where behaviour must depend on time, tests fake the clock rather than letting real time pass. Claim 3's exact cause of the three reported composite_* timeouts is UNKNOWN from source alone. The shared-overlay 20-second fallback is flagged as needing a separate architecture ruling; this document does not settle that product policy."
---

# Architect ruling — timing architecture: causal progress, semantic virtual time, independent watchdog

**Date:** 2026-08-23
**Authority:** architecture consult (codex architect), acting under the delegated
amendment-ratification authority recorded for this program.
**Supersedes:** none. Keep `block/deterministic-tests`; do not rebase or discard
it. This ruling names four corrections required before that conversion pattern
is applied more broadly.

## Context

Verter's test suite has a recurring failure class: tests that pass on an idle
machine and time out under load. The prompting case was three TIMEOUTs in
`cases::shared_provider_live::composite_*` (`verter_lsp`, real tsserver
subprocess) on a branch that touched no provider/tsserver/tsgo files. An
in-flight block (`block/deterministic-tests`) was replacing timing windows with
causal assertions — waiting on an observable event instead of a duration.

The consult was asked whether that approach is the right architecture, or
whether production and tests over-use durations, sleeps, debounces, poll
intervals and fixed deadlines where a structurally different mechanism would
be correct.

**Maintainer direction this ruling serves** (recorded 2026-08-23):

> Time-sensitive checks get a proper refactor and the best possible solution.
> Where behaviour must depend on time, tests **fake the clock** rather than
> letting real time pass. Reasons, in the maintainer's order: (1) time-based
> events are flaky by nature; (2) real elapsed time in tests is wasted
> wall-clock; (3) a time-based event should exist only where it is the ONLY
> possible solution.

**Evidence.** The consult's conclusions, citing production and test files on
`program/architecture-lock` and the in-flight `block/deterministic-tests`
worktree. File:line citations below are the architect's. The exact cause of
the three reported `composite_*` timeouts is not settled by source alone.

## Decision 1 — the core answer

The “causal assertion” direction is sound, but it is not a universal
replacement for time. The question is mis-framed as “causal assertion versus
virtual clock.” The architecture is **causal progress + semantic virtual time
+ an independent real watchdog**, selected per surface.

Distinguish three different things:

1. **Progress/order** — owned by state transitions, receipts, channels, and
   protocol responses.
2. **Semantic time** — debounce, backoff, fairness, cooldown.
3. **Liveness bounds** — independent watchdogs that turn a missing transition
   into a local failure.

The `composite_*` evidence points more strongly to a test-harness /
resource-composition problem than to a missing clock abstraction.

### Per-surface table

| Surface | Progress authority | Time authority | Primary test mechanism |
|---|---|---|---|
| LSP debounce | edit generation + quiet-window deadline | One domain-owned LSP quiet-window policy | Paused Tokio time plus boundary assertions |
| Import publication | delivered receipt / in-flight `watch` | Same quiet-window policy for edit-triggered enqueue | Advance time, await receipt, assert user result once |
| Scheduler / condvar | mutex-protected predicate, completion channel | No semantic clock | Barriers / channels; outer watchdog; model tests only for small state machines |
| tsgo cancellation | atomic cancellation state + `Notify` | Request / lifecycle watchdog only | Deterministic channel handshakes |
| Real tsgo / tsserver | protocol request / response and readiness witness | Real monotonic watchdog | Serialized / resource-isolated subprocess test |
| Performance contracts | completed operation | Real monotonic time | Warmups / repetitions / distribution, not single tight ceilings |

## Decision 2 — mechanism

Standardize async Tokio-owned code on `tokio::time::Instant` and Tokio
timers, with paused time in tests.

A custom clock trait is introduced only for a synchronous domain that
genuinely has semantic time and cannot use Tokio. **The scheduler does not
currently need one.** A universal mock-clock trait is not required.

There is no repo-owned injectable `Clock` interface. The closest façade is a
platform-dependent re-export of `std::time::Instant` or `web_time::Instant`,
not dependency injection (`crates/verter_session/src/instant.rs:1-5`). That
does not mean tests cannot use virtual time without a production change:
`verter_type_runtime` already enables `tokio/test-util`, and its
restart/backoff tests run with `start_paused = true`
(`crates/verter_type_runtime/Cargo.toml:39-49`,
`crates/verter_type_runtime/src/resilient_tests.rs:1096-1115`), exercising a
real production `tokio::time::sleep` backoff
(`crates/verter_type_runtime/src/resilient.rs:1026-1069`).

A production change is needed only for paths using a different clock. The
important example is `SyncCoordinator`: it timestamps with
`std::time::Instant`, converts that deadline into a Tokio sleep, then checks
readiness again using `std::time::Instant`
(`crates/verter_lsp/src/sync_coordinator.rs:307-329`,
`crates/verter_lsp/src/sync_coordinator.rs:448-464`). Pausing Tokio time
cannot reliably drive that mixed-clock algorithm. It must use
`tokio::time::Instant` consistently.

**A deterministic executor is not the primary mechanism.** It cannot control
`parking_lot` threads, Rayon pools, Cargo, tsserver, tsgo, or OS scheduling.

An event log is useful for complex multi-stage ordering, but durable
receipts/generations remain the production authority. **An event log is an
observer, never a second state machine.**

Causal waits use this construction:

- A durable state predicate or receipt is authoritative.
- `Notify`, `watch`, or a channel is only the wake mechanism.
- Interest is registered before rechecking the state.
- An independent watchdog bounds the whole test.

An unbounded causal wait can degrade a precise assertion into a suite-level
timeout. That is poorer diagnostics, not a masked pass: nextest still fails
it.

Clock control cannot make an OS child execute deterministically. Protocol
ordering can nevertheless be deterministic. For real tsserver/tsgo processes
the right combination is protocol readiness, resource isolation, and a
generous real-time watchdog. Their latency is nondeterministic; readiness is
observable:

- Managed tsgo sends `initialize`, awaits the response, sends `initialized`,
  and only then returns the provider
  (`crates/verter_type_runtime/src/tsgo/ipc.rs:2257-2324`).
- Managed tsserver awaits a response-bearing `configure` request before
  construction completes
  (`crates/verter_type_runtime/src/tsserver/ipc.rs:1903-1951`,
  `crates/verter_type_runtime/src/tsserver/ipc.rs:2282-2303`).
- Shared tsgo already captures an in-band `InitializedWitness`
  (`crates/verter_tsgo_api/src/relay.rs:583-626`). `verter/waitInitialized`
  races that witness against relay death and a lifecycle timeout
  (`crates/verter_tsgo_api/src/control/server.rs:386-423`). The LSP shared
  provider consumes that protocol fence before establishing the API session
  (`crates/verter_lsp/src/tsgo/shared.rs:432-498`).

LSP debounce and edit-triggered import publication are semantically timed.
The coordinator promises one sync after 300 ms of silence
(`crates/verter_lsp/src/sync_coordinator.rs:1-6`,
`crates/verter_lsp/src/sync_coordinator.rs:276-307`). Edit-triggered import
publication separately sleeps for another hard-coded 300 ms and abandons
superseded epochs
(`crates/verter_lsp/src/server/import_publication.rs:35-37`,
`crates/verter_lsp/src/server/import_publication.rs:154-176`). That duration
cannot disappear without changing product behaviour. It is tested with
virtual time:

- Prove nothing publishes at `quiet_window - ε`.
- Prove a later edit resets the window.
- Advance to the boundary.
- Then await the publication receipt or user-visible result.

Publication completion itself is causal. `ImportSyncMemo` already records
durable receipts and supplies a `watch` receiver resolving when an in-flight
publication settles
(`crates/verter_lsp/src/server/import_sync_state.rs:12-24`,
`crates/verter_lsp/src/server/import_sync_state.rs:83-104`). Semantic delay
remains; real wall-clock waiting in tests does not.

## Decision 3 — ownership

Timing values belong to their semantic domain, not to one global clock
object:

- `verter_lsp`: quiet windows and scanner fairness.
- `verter_type_runtime`: provider lifecycle, recovery, silence health, and
  request-hop margins.
- `verter_tsgo_api`: relay/attach/process lifecycle bounds.
- `verter_workspace`: external-tool execution policy.

The composition root may assemble those policies. It must **not** flatten
them into one undifferentiated duration table. A single global “time scale”
is the wrong answer: these durations have different semantics and ordering
relationships.

Production timing values are presently scattered, with no single control
point. Examples:

- Two independent 300 ms LSP constants
  (`crates/verter_lsp/src/sync_coordinator.rs:276-307`,
  `crates/verter_lsp/src/server/import_publication.rs:35-37`).
- Shared attach/overlay/close bounds of 15/20/2 seconds
  (`crates/verter_lsp/src/tsgo/composite.rs:56-84`).
- Scanner fairness values of 1 second and 500 ms
  (`crates/verter_lsp/src/workspace_scanner.rs:352-360`).
- Vite subprocess timeout/poll values of 10 seconds and 50 ms
  (`crates/verter_workspace/src/vite_config.rs:829-840`).
- Relay readiness and carrier-barrier bounds in separate modules
  (`crates/verter_tsgo_api/src/control/server.rs:63-68`,
  `crates/verter_tsgo_api/src/relay.rs:157-162`).

There is partial centralization for per-LSP-method request/audit budgets
(`crates/verter_session/src/types.rs:986-1048`). Production request deadlines
default to disabled (`crates/verter_session/src/types.rs:1051-1114`). That
does not centralize debounce, lifecycle, process, scanner, or recovery
policies.

## Decision 4 — durations that remain

Legitimate production durations include:

- Debounce/quiet windows.
- Retry backoff, activation cooldown, and rate limiting.
- Scanner fairness and starvation caps.
- Heartbeats and silence-health detection.
- Initialization, attach, writer-stall, shutdown, process-reap, and
  best-effort close bounds.
- The shared-overlay 20-second fallback bound, if the intended product
  policy is genuinely “slow editor route admits managed fallback.” It
  changes serving behaviour and therefore needs an explicit architecture
  ruling and exact timer tests; it is not merely a watchdog.
- Outer test-process watchdogs.
- Real wall-clock performance measurements.

Polling intervals are legitimate when observing an external thing with no
push facility. They are not legitimate for an in-process state transition
that Verter itself owns.

The shared-overlay 20-second fallback bound is **not** settled here. It
needs its own explicit architecture ruling because it changes serving
behaviour.

## Decision 5 — claims answered

The consult was asked to mark six claims TRUE / FALSE / PARTLY. Those
verdicts are part of this ruling.

### 1. Production has no injectable clock, so tests cannot use virtual time without production changes — PARTLY

No repo-owned injectable `Clock` interface was found. Tokio time can already
be virtualized without changing production code on paths that already use
Tokio timers. A production change is needed only for mixed-clock paths such
as `SyncCoordinator`. A universal mock-clock trait is not required.

### 2. LSP debounce/publication is semantically timed, so causal assertions cannot replace all durations — PARTLY

The 300 ms quiet window is product behaviour and cannot disappear without
changing it. It is tested with virtual time plus boundary assertions.
Publication completion is causal (`ImportSyncMemo` receipts / `watch`).
Semantic delay remains; real wall-clock waiting in tests does not.

### 3. `composite_*` timeouts come from a fixed deadline with no completion signal available — FALSE as written; actual cause UNKNOWN

The feature calls already await observable completion. `get_diagnostics` is
awaited inside the 45-second watchdog
(`crates/verter_lsp/tests/cases/shared_provider_live.rs:1542-1548`). The
JSON-RPC transport registers a response `oneshot` before sending and awaits
that receiver (`crates/verter_type_runtime/src/tsgo/ipc.rs:762-829`). The
timeout surrounds the completion signal; it is not substituting for one.

Initialization is also observable through its JSON-RPC response, although
the test harness currently discovers that response by polling a frame vector
every 25 ms
(`crates/verter_lsp/tests/cases/shared_provider_live.rs:314-390`,
`crates/verter_lsp/tests/cases/shared_provider_live.rs:1450-1458`). That
harness can use a pending-request `oneshot` or frame `Notify`.

Stronger load-sensitive structural problems exist (see Decision 6,
correction 1). They plausibly explain load sensitivity; they do not prove
which one caused the three observed timeouts. Test artifacts and isolated
reruns are required by the project's own timeout policy
(`.claude/skills/testing/SKILL.md:371-373`).

### 4. Causal waits risk masking races by hanging when the event never arrives — PARTLY

An unbounded causal wait is poorer diagnostics, not a masked pass. The
correct construction is the durable-predicate + wake-mechanism + independent
watchdog pattern in Decision 2. The in-flight cancellation change on
`block/deterministic-tests` follows that pattern
(`crates/verter_tsgo_api/src/actor/mod.rs:72-104`,
`crates/verter_tsgo_api/src/actor/mod.rs:205-224` on that branch). Its actor
test observes the actor's raw reply channel so client-side cancellation
cannot mask an incorrect actor result
(`crates/verter_tsgo_api/src/actor/tests.rs:303-331` on that branch). The
scheduler branch still polls `waiter_count` with 1 ms sleeps and fixed
deadlines (`crates/verter_scheduler/src/cpu_concurrency.rs:204-250` on that
branch). A test-only channel emitted immediately before `Condvar::wait`
would provide the same discrimination without polling.

### 5. Real tsserver/tsgo processes are inherently unobservable at startup — FALSE

Latency is nondeterministic; readiness is observable. See Decision 2.

### 6. Production timing values are scattered, with no single control point — TRUE

See Decision 3. Scattering is a fact; a single global time scale is still
the wrong repair.

## Decision 6 — keep `block/deterministic-tests`; four corrections first

Keep the work. Do not rebase or discard it wholesale. Its strongest changes
are architecturally correct:

- Paused-time tests for registration signals.
- Atomic-state-plus-`Notify` cancellation.
- Oneshot handshakes that force exact cancellation ordering.
- Replacing “sleep and hope the second thread has blocked” with observation
  of the blocking predicate.

Before extending that conversion pattern more broadly, four corrections are
required.

### Correction 1 — fix the live-process harness first

Remove the nested Cargo build, provide the shim as a prebuilt test artifact
or move the integration surface to the shim's defining package, **serialize
every real-process shared-provider test**, and turn `FakeEditor` into a
request-id-to-oneshot JSON-RPC harness.

Cited evidence:

- Each test process may run `cargo build -p verter_relay_shim` from inside
  the test
  (`crates/verter_lsp/tests/cases/shared_provider_live.rs:410-443`). That
  contradicts the ratified rule that tests do not build CLIs or Rust
  projects
  (`docs/arch/refactor/rev11/rulings/MAINTAINER-RULING-NO-BUILD-INVOKING-TESTS.md:24-29`).
- The nextest group serializes only two `composite_*` tests
  (`.config/nextest.toml:20-34`). The two real-process template variants at
  `crates/verter_lsp/tests/cases/shared_provider_live.rs:2100-2122` are
  absent from that group.
- A template composite test has a 40-second initialization guard followed by
  three sequential 45-second feature guards
  (`crates/verter_lsp/tests/cases/shared_provider_live.rs:1450-1458`,
  `crates/verter_lsp/tests/cases/shared_provider_live.rs:2023-2070`). Those
  declared maxima already total 175 seconds before teardown, against
  nextest's 180-second process watchdog (`.config/nextest.toml:4-12`).
- `FakeEditor` discovers the initialize response by polling a frame vector
  every 25 ms
  (`crates/verter_lsp/tests/cases/shared_provider_live.rs:314-390`,
  `crates/verter_lsp/tests/cases/shared_provider_live.rs:1450-1458`).

### Correction 2 — rebase the LSP debounce slice on consistent Tokio time

Replace `std::time::Instant` in `SyncCoordinator`
(`crates/verter_lsp/src/sync_coordinator.rs:307-329`,
`crates/verter_lsp/src/sync_coordinator.rs:448-464`), give both 300 ms paths
one quiet-window owner
(`crates/verter_lsp/src/sync_coordinator.rs:276-307`,
`crates/verter_lsp/src/server/import_publication.rs:35-37`), and test the
boundary under paused time.

### Correction 3 — do not treat suppression flags as timing architecture

The block adds public `LspConfig` switches that turn off the two production
background entrances (`crates/verter_lsp/src/lib.rs:489-515` on
`block/deterministic-tests`):

- `suppress_edit_debounced_import_publication`
- `suppress_sync_coordinator_signal`

Those switches are **not** timing architecture. They are useful temporarily
for mechanism-isolation tests, but they create a non-production topology.
Prefer paused time, explicit actor control in a test harness, or a domain
timing policy.

### Correction 4 — finish replacing polling with synchronization

The latest production-debounce test has a better user-visible oracle, but it
still polls `references_at` every 10 ms under a 10-second real deadline
(`crates/verter_lsp/src/server/workspace_symbol_frontier_tests.rs:480-536`
on `block/deterministic-tests`). Await the publication/semantic receipt,
then issue the user-visible request once. The scheduler branch's 1 ms
`waiter_count` poll
(`crates/verter_scheduler/src/cpu_concurrency.rs:204-250` on that branch) is
the same class.

The current block has causal progress largely right. The debounce and
subprocess surfaces need semantic virtual time and the independent real
watchdog before the same conversion pattern is applied more broadly.
