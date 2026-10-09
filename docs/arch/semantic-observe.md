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

These rules publish the existing production-only-state policy; they do not
reclassify or remove existing bookkeeping. A later consolidation applies them
to existing mixed mechanisms. Ambiguous classifications need a ruling from the
policy owner; record the item, competing readings and actual consumers rather
than silently choosing a new policy.

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
| Name the concrete item; split mixed fields | Name the decision/API consuming it, or `none` for measurement-only state | `REQUIRED`, `REQUIRED-budget`, `REQUIRED-lifetime`, or `OPTIONAL` | State who retains it and when it retires/resets | Crate-relative module owning it | `always` or `cfg(feature = "semantic-observe")` |

Rows must name real consumers and lifetimes, not infer them from a telemetry or
cache-shaped name. A required count is a current occupancy or policy charge,
not a history of visits, allocations or peaks. A mixed snapshot can contain
both required occupancy and optional historical counters; classify those
fields separately. The seeded inventory lives at
[`crates/verter_audit/observe-inventory/SKR-OBS-D.md`](../../crates/verter_audit/observe-inventory/SKR-OBS-D.md).

During consolidation, a seeded OPTIONAL row lists its legacy gate alongside
the coordinated gate. `semantic-observe` enables that legacy gate; legacy
opt-ins still work independently. This annotation describes current code,
not a claim that the old `cfg` sites have already been replaced. New OPTIONAL
items use `cfg(feature = "semantic-observe")` directly.

## Feature and generator constraints

`semantic-observe` is a coordinated, default-off Cargo feature. The audit crate
declares it; scheduler, workspace, semantic, compiler, the semantic source
layer, the type engine and the session forward it through existing dependency
edges without enabling it. The session forwarding also implies its existing
attribution and currency probe features. The benchmark harness exposes the
combined opt-in through the session.

Existing measurement features remain available until consolidation. The
coordinated opt-in implies attribution, currency probes and hotpath
instrumentation. `test-support` remains a separate capability. Only the
benchmark harness and an owner's own test self-edge may request optional
capture. No production dependency edge or default feature may enable it or
its implied measurement features, even through the production root's
dev-dependency feature unification. Tests that need optional capture can run
with an explicit `--features semantic-observe`.

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

An instrumented binary currently captures unconditionally. Per-request off
selects concrete uncaptured/captured execution once at root entry, carrying
that choice through resumptions; background work selects at job submission.
That selection and the existing-bookkeeping sweep belong to consolidation,
not this substrate. Bound specialization to these two modes, not every
host/counter combination. Do not use per-operation enabled checks,
NoOpObserver virtual calls or observer TLS polling in hot loops.

## Capture availability API

`verter_audit::observe::CaptureAvailability::compiled()` reports `Unavailable`
without `semantic-observe` and `Available` with it. Availability is a binary
capability, independent of the profile and of whether a request asks for
capture. `ObserveMode::{Uncaptured, Captured}` is the execution vocabulary;
`ObserveMode::compiled()` gives the current unconditional build mode.

`observe::capture(|| payload)` returns `None` when the feature is off without
calling the collector. With it on it calls the collector once and returns
`Some(payload)`. Construct metrics inside the closure. This capture boundary
does not install observers, alter REQUIRED state or perform runtime enabled
checks. It does not migrate existing audit endpoints; their consolidation
must report unavailable capture through this capability, never fabricated
zero metrics.

## Verification boundaries

The audit integration tests resolve feature implications from `cargo metadata`
and actual per-root feature activation from `cargo tree` under resolver 2.
Metadata's workspace-wide resolved feature union alone is not production
build evidence: it includes other members' dev features. The guard checks each
of `verter_napi`, `verter_lsp`, `verter_wasm`, and `verter_tsc` with normal/build
edges and with the root's dev edges, on the host and WASM targets. Cargo feature
selection is profile-independent, so the same closure applies to default/dev,
debug, release and `no-debug-assertions`; profile compile lanes supply the
separate build evidence. Isolated fixture manifests demonstrate rejection of
direct, implied and dev-dependency enablement and acceptance of independent
test-support features.

Run `cargo nextest run -p verter_audit` both without and with
`--features semantic-observe` for availability and closure proof. Formatting,
Clippy, docs build, production builds, and the canonical/shipped-cfg and WASM
lanes supply their respective independent checks. The docs site's build
excludes `docs/arch/**`, so policy fidelity also requires review against the
classification paragraphs; a docs build alone does not validate this page.
