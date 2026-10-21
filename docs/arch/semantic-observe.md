# Optional semantic observations

This is the binding classification policy for semantic state and instrumentation.
Classify each field, store, hook, counter or trace by its actual consumer, not its
historical name. Split mixed mechanisms field by field. Every owner records its
classification in the inventory described below; the inventory is documentation,
never a runtime registry.

## Classification policy

**REQUIRED, always on:** compact FactResult status and machine causes needed for
safe behavior; adopted dependencies, exact read sets/strict roots/absence
evidence, validation tokens and publication fences; semantic/binder identities
and checker operation/reset/depth state; transaction journals,
active-recursion/SCC assumptions and minimal sound divergence certificates;
leases/retained-child edges, incarnation/cancellation/singleflight/wakeup state.
Existing ambient completeness taint stays required only until its replacement
lands atomically. Its history and discarded-probe narratives are optional. A
validity fact tracer is required; its chronological event stream is optional.

**REQUIRED-budget:** local operation counters, compact operation footprints and
sealed logical cost receipts, including failed prefixes. They make budget
outcomes cold/warm-deterministic. Charge exclusive identities once per connected
demand; share immediate-prerequisite DAGs/SCC seals, not per-step audit traces.
Minimal diagnostic arguments/provenance and elaboration needed for an actually
requested user diagnostic are product data. Human relation explanations and
why-loaded/why-instantiated histories are optional.

**REQUIRED-lifetime:** ownership/refcounts, cache/family bounds, admission
reservations and external-pin charge transfers. Production count accessors
remain available with capture off. Derive current counts from collection
lengths/occupancy under coherent snapshot locking; where necessary update
existing metadata at insert/remove or bulk transitions. Cover every resident
table keyed by node/content/view plus backing capacity. Do not add duplicate
event histories or global atomics per semantic operation. Compact cache/pin
policy charges are required. Active-byte histories, allocator attribution and
peak gauges are optional when no production decision consumes them; ownership
size/charge transfers remain exact.

**OPTIONAL, off by default:** audit envelopes/capture/stores, correlation-only
trace IDs, event tracers, taint histories, attribution/performance counters,
timers/contention/RSS samplers, explanation/proof trees, detailed cause
paths/debug dumps, footprint mining and chronological operation logs. Audit
footprints differ from required budget operation footprints. Test
cold-compute/visit/allocation counters and historical high-water metrics are
optional; actual production occupancy counts are required. An advisory
construction-byte rail is opt-in, high/configurable and bulk-only; remove it if
enabled normal-size CPU cost is measurable. It supplies no hard
computation-memory bound and cannot downgrade already complete output.

The consolidation sweep applies these rules to existing mixed mechanisms; its
rows, including the optional state still compiled into default builds, live
in [`crates/verter_audit/observe-inventory/SKR-OBS.md`](../../crates/verter_audit/observe-inventory/SKR-OBS.md).
Ambiguous classifications need a ruling from the policy owner; record the
item, competing readings and actual consumers rather than silently choosing a
new policy.

## Inventory format and ownership

The inventory is the glob `crates/*/observe-inventory/*.md`. Each owner task
writes one file at `crates/<owner's primary crate>/observe-inventory/<SLUG>.md`.
The slug identifies its owner task. There is no shared index file. Group the
rows into one table per owner crate within that file, including rows for other
crates that the same owner changes. Successors extend their own file, avoiding
concurrent edits to other owners' files or this policy page. The consolidation
owner may revise this policy and consolidate the inventory later.

Every table has exactly these columns:

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| Name the concrete item; split mixed fields | Name the decision/API consuming it, or `none` for measurement-only state | `REQUIRED`, `REQUIRED-budget`, `REQUIRED-lifetime`, or `OPTIONAL` | State who retains it and when it retires/resets | Crate-relative module owning it | `always`, `cfg(feature = "semantic-observe")`, a test gate, or `absent` |

Rows must name real consumers and lifetimes, not infer them from a telemetry or
cache-shaped name. A required count is a current occupancy or policy charge,
not a history of visits, allocations or peaks. A mixed snapshot can contain
both required occupancy and optional historical counters; classify those
fields separately. The seeded inventory lives at
[`crates/verter_audit/observe-inventory/SKR-OBS-D.md`](../../crates/verter_audit/observe-inventory/SKR-OBS-D.md).

OPTIONAL items use `cfg(feature = "semantic-observe")`. Optional state whose
only readers are tests may instead be gated to tests alone: `cfg(test)` when
only the owner's unit tests read it, `cfg(any(test, feature = "test-support"))`
when dependents' tests read it, and the combined
`cfg(any(test, feature = "test-support", feature = "semantic-observe"))` when a
measurement build also reads it. Every such gate is absent from production
builds. `absent` marks a deleted item. The former `attribution`,
`currency_probe` and `hotpath` gates are retired; no crate declares them.

## Feature and generator constraints

`semantic-observe` is the one coordinated, default-off measurement feature. The
audit crate declares it for the work-attribution table; the query boundary
declares it for the currency-probe rail; the parser, diagnostics, semantic,
compiler, scheduler, workspace, semantic source layer, type engine and
session declare it for their own optional state and `hotpath` profiler
instrumentation (it activates the optional `hotpath` dependency directly;
hotpath's profiler is enabled only on non-wasm32 targets, so an instrumented
wasm32 build compiles hotpath's no-op form), and
forward it through existing dependency edges without enabling it. The LSP,
NAPI, WASM, MCP and tsc roots, the FFI conversion crate and the benchmark
harness expose the combined opt-in;
the execution crate declares it for the request-context capture hooks. `hotpath-alloc`
layers hotpath's allocation profiling on top and implies `semantic-observe`.
There are no other measurement features.

`test-support` remains a separate capability. Only measurement harnesses
request optional capture; no production dependency edge, default feature or
dev-dependency edge may enable it, even through a root's dev-dependency
feature unification. The session crate's own test build is checked the same
way, so the canonical gate and the core CI shards never switch
`semantic-observe` on. They do build with the members' `test-support`
features, and the observation gate
`cfg(any(test, feature = "test-support", feature = "semantic-observe"))` also
compiles the bookkeeping tests read under `test-support`: origin edges, the
graph store's tallies, occupancy counters, VFS provenance and the
declaration-lowering counters. The shipped configuration of that state — no
`test`, no `test-support`, no `semantic-observe` — is exercised by the
shipped-cfg lane (`verter_shipped_cfg_contract` under the
`no-debug-assertions` profile), and the feature-closure guard keeps every
member's `test-support`, like `soak-support`, out of every shipped closure.
Suites that read a captured record, a footprint or
a measurement carry `#[cfg(feature = "semantic-observe")]` and run in the
instrumentation lane: `node scripts/observe-lane.mjs run` tests every
workspace member that declares `semantic-observe` with it switched on, under
the same provider-free core filter (CI job `rust-observe`). The member set is
read from `cargo metadata`, so a crate that declares the feature joins the
lane without a list edit. A default-build test is kept for each public audit
surface to prove it reports unavailable capture.

Guards whose only evidence is an optional counter run there too: the
lazy-lowering guards that count lowered declaration bodies, the fallthrough
reuse guard and the one-native-call compat guard read provenance counters that
a default build does not compile, so the instrumentation lane, a required CI
job, is their primary gate. Optional state read only by tests stays under a
test gate instead, and keeps running in the canonical gate.

When optional state is consolidated, default builds compile away optional
fields, hooks, argument/payload construction, TLS lookups, atomics, timers,
allocations and proof interning. Required state never depends on capture.
Generators must preserve that boundary:

- Semantic records and result constructors cannot require explanation/proof
  IDs or correlation trace IDs. Identity needed for validity, binding or
  ownership stays required; a human explanation does not become such an
  identity by sharing its storage.
- Generated optional fields, producers and payload construction use the same
  default-off feature as handwritten instrumentation. No generator may add a
  default-on counter, always-allocated explanation store or hidden feature
  activation to a production dependency.
- A generated reader cannot substitute zero or an empty metric set for capture
  that was compiled out. Required budgets, diagnostic data and ownership
  accounting remain accessible independently.

An instrumented binary captures per request. The audited entries (component
meta, type resolution, named-symbol and expression evaluation, graph queries,
framework surfaces, flow return, analyze, compile, workspace operations, LSP
handlers and MCP tools) select captured or uncaptured execution once at root
entry (`verter_session::capture::select_mode`, over
`ObserveMode::select`) from the host's `audit_enabled` setting and request
filter, and build their request context with
`verter_session::capture::root_entry_context`. Both modes install the same
REQUIRED context — canonical, the host's `projection_op_budget` and
cancellation — so budget, cancellation and partiality behave identically and
only the optional capture differs. The context carries the selected mode
(`RequestContext::observe_mode`), and every scheduler resumption and task
dispatcher reinstalls that same context, so background work runs in the mode
its submitting request selected. A captured context plants the audit observer
and the footprint accumulator; an uncaptured one clears the observer slot.
An LSP handler is an async window that may resume on any runtime worker, so
neither mode installs a governing request context there: the requests the
handler issues install their own exactly as they do uncaptured, and a captured
LSP session plants only the observer slot.

### Concrete execution modes

The selected mode becomes a type. `verter_audit::observe::ObserveExec` is a
sealed trait with exactly two implementors, `Captured` and `Uncaptured`, and
`verter_audit::specialise!(mode => M, { ... })` expands its body once per mode
with `M` naming that mode's type (only the `Uncaptured` expansion in a default
build). Code compiled for `M` emits through the typed forms
`observe!(M => |obs| ...)`, `observe_request!(M => |ctx| ...)`,
`component_meta_trace_structured!(M => ...)` and
`component_meta_trace_custom!(M => ...)`, each a branch on the constant
`M::CAPTURED`: the uncaptured expansion reads no observer, request-context,
accumulator or timing slot and builds no payload. Specialization is bounded
to these two modes, not every host/counter combination.

The mode rides the structures an execution unit already carries:

- The engine's capability family, `ResolverCapabilities::Observe`. A
  `ProjectSemanticDispatch<C>` is compiled for `C::Observe`; its evaluators
  (`ArenaOps`, `RelationCx`, `FlowCx`, `InferenceTxn`) carry the driver's mode.
- The semantic graph store as a unit reaches it, `GraphIn<'_, M>`
  (`ProjectSemanticDispatch::graph`). Interning (shard-lock timing), memo
  reads and publishes (per-request cache counters and cache events),
  dependency-signature interning and origin-edge recording are compiled for
  the unit's mode. `record_origin_edge!` builds an edge's arguments only when
  `GraphIn::records_origin_edges` is true: always in test and `test-support`
  builds, otherwise only for captured execution.
- The session's request contexts. `HostCapabilities<M>` is the host family
  for mode `M`; `HostRequestLifecycle<'a, M>` and
  `SessionRequestLifecycle<'a, M>` carry it (`RequestBoundLifecycle::Observe`),
  so every request port the dispatcher calls is compiled for the unit's mode
  and reaches the shared stores' typed lookups (`get_any_in`, `get_route_in`,
  `ensure_indexed_ready_serve_in`, ...). Session code between a root entry and
  a dispatcher is generic over `M`.

A unit adopts the carried mode once, where it starts: an audited root entry
from its own selection (`capture::entry_mode` when the body arrives as a
closure), any other unit — a context or dispatcher built inside a request, a
scheduler job, an upsert, an invalidation — from the installed request
(`capture::installed_mode`, over
`request_context::installed_observe_mode`). A shared store or host operation
called from outside a unit keeps a plain entry (`get_any`, `get_route`, ...)
that adopts the installed mode once and runs the typed `_in` form; units call
the typed form directly. A surface that installs a context only to arm the
projection budget, with no audit registration, selects its mode by the same
rule: with no request installed, captured exactly when an observer is planted. Once-per-request and once-per-phase emission — a
root entry's record, a compile phase's timing, a cold cache write, a
workspace maintenance pass — may use the untyped forms, which read the slot
once. A scheduler job carries its submitting request's context from
submission, and its dispatch publishes only when that carried context
reports `RequestContextLike::captured`; the worker never polls a slot to
decide. A `CodeTransform` counts its operations in a plain per-transform
tally and publishes the count once, when dropped.

In a debug instrumented build a dispatcher bound to a request context asserts
its compiled mode equals the mode the installed root entry selected, so a
misrouted unit fails in the instrumentation lane rather than dropping a
request's observations. The process-wide work-attribution table
(`verter_audit::attribute!`, `count_resolver_context_call!`) and the hosts'
provenance counters are selected by the instrumented build as a whole, not per
request, and are outside this specialisation.

A default build has no audit envelope at all. The observer slot, the
per-request accumulator, the record store, the scheduler and VFS audit sinks
and the request context's audit counters are compiled out;
`verter_session::capture::normalize_host_config` clears `audit_enabled`,
`footprint_capture` and `audit_timing_capture` at host construction; and every
audited entry returns its real result with a record whose `capture_state` is
`Unavailable`.

## Capture availability API

`verter_audit::observe::CaptureAvailability::compiled()` reports `Unavailable`
without `semantic-observe` and `Available` with it. Availability is a binary
capability, independent of the profile and of whether a request asks for
capture. `ObserveMode::{Uncaptured, Captured}` is the execution vocabulary;
`ObserveMode::select(requested)` picks the mode for a root entry and is always
`Uncaptured` in a default build; its production caller is
`verter_session::capture::select_mode`. `ObserveExec`, `Captured`,
`Uncaptured` and `specialise!` turn the selection into a type (see "Concrete
execution modes"). `verter_session::capture::availability()` re-exports the
build's answer for the host layer.

`observe::capture(|| payload)` returns `None` when the feature is off without
calling the collector. With it on it calls the collector once and returns
`Some(payload)`. Construct metrics inside the closure.

Emission and storage use one shape across crates:

- `verter_audit::observe!(|obs| body)` runs `body` against the installed
  observer. Without the feature the body is type-checked inside a closure that
  is never built, so no argument or payload is constructed.
  `verter_type_engine::observe_request!(|ctx| body)` is the same form over the
  current request context.
- `ObservedU64`, `ObservedU32`, `ObservedU16` and `ObservedFlag` are atomics
  with the feature and zero-size without it; their writes compile to nothing
  and their readers exist only with the feature. `Stopwatch` reads the clock
  only with the feature, and `Sample<T>` evaluates its closure only with it.

Public surfaces report unavailability, never fabricated zero metrics:
`AuditCaptureState::Unavailable` marks every record a default build returns;
the NAPI and WASM bindings export `captureAvailability()` (`"Available"` or
`"Unavailable"`), and their record readers (`getLastAuditRecord`,
`getAuditRecords`, `getBundlerBatchSummary`, the why-loaded and
why-instantiated walkers, `getComponentMetaWithAudit`) fail with an error that
names the missing feature; the LSP audit methods answer with the same error.

## Verification boundaries

The feature-closure guard
(`crates/verter_source_policy_gate/tests/cases/observe_feature_closure.rs`)
resolves feature implications from `cargo metadata` and actual per-root
feature activation from `cargo tree` under resolver 2. Its seeds are read from
the workspace's own feature tables: every member's `semantic-observe` and
every member feature that implies a coordinated gate, so a new owner or
measurement mode is covered by declaring that dependency.
Metadata's workspace-wide resolved feature union alone is not production
build evidence: it includes other members' dev features. The guard checks each
of `verter_napi`, `verter_lsp`, `verter_wasm`, and `verter_tsc` with normal/build
edges and with the root's dev edges, plus `verter_session` with its dev edges,
on the host and WASM targets. Cargo feature
selection is profile-independent, so the same closure applies to default/dev,
debug, release and `no-debug-assertions`; profile compile lanes supply the
separate build evidence. Isolated fixture manifests demonstrate rejection of
direct, implied and dev-dependency enablement and acceptance of independent
test-support features.

The capture on/off equivalence suite
(`crates/verter_ffi/src/convert/capture_equivalence_tests.rs`) drives the same
component-meta, compile and named-type requests cold through the plain and
audited entries on hosts with capture off and on, and requires byte-identical
wire answers; it runs in both the default build and the instrumentation lane,
where it also requires the audited host to have stored every record.

Run `cargo nextest run -p verter_audit` both without and with
`--features semantic-observe` for availability proof,
`node scripts/observe-lane.mjs run` for the instrumentation lane, and
`cargo nextest run -p verter_source_policy_gate -E 'test(observe_feature_closure)'`
for closure proof. Formatting,
Clippy, docs build, production builds, and the canonical/shipped-cfg and WASM
lanes supply their respective independent checks. The docs site's build
excludes `docs/arch/**`, so policy fidelity also requires review against the
classification paragraphs; a docs build alone does not validate this page.
