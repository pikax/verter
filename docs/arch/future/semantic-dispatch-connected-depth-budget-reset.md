# Deferred: the connected-query depth budget can be reset by constructing a new dispatcher

**Status:** DEFERRED — recorded, not repaired.
**Why deferred:** Verter's own typeinfo / semantic type resolver is out of scope for the
current effort (see the ratified scope directive: native type resolution is being taken off
the LSP path entirely, so resolver defects are not on the critical path). Nothing here may
be changed without the owner's direction — this record exists so the finding is not lost.

**Audit verdict (2026-07-22): OUT-OF-SCOPE.** This is an internal semantic type-engine defect, which the binding scope explicitly excludes.

**Confidence:** the mechanism is read from the source and is unambiguous; the *live*
consequence is evidence-consistent but **not proven**. See "What was NOT proven" below.

---

## Symptom

`MAX_CONNECTED_QUERY_DEPTH` is the dispatcher's guard against runaway recursive query
demand. Across every instrumented run of a real multi-package Vue workspace, the deepest
`query_depth` ever observed at the shared cold-build choke point was **13**, against a cap
of **24** — the guard never fired, on any thread, in any run, including runs that ended in a
stack overflow elsewhere in the process.

That is consistent with two readings, and this record does not distinguish them:

1. the workload genuinely never demands more than 13 levels of connected query; or
2. the counter is being restarted before it can reach the cap.

Reading (2) is structurally possible, which is what makes this worth recording: a budget
that can be silently restarted is not a budget.

## Mechanism

The budget lives on the dispatcher instance, not on the thread or the request:

- `crates/verter_session/src/project_semantic_dispatch/mod.rs:178`
  `const MAX_CONNECTED_QUERY_DEPTH: u16 = 24;`
- `crates/verter_session/src/project_semantic_dispatch/mod.rs:320`
  `connected_demand: ConnectedDemandState` — a **per-instance field**.
- `crates/verter_session/src/project_semantic_dispatch/mod.rs:425`
  the field is initialised in the constructor.
- `crates/verter_session/src/project_semantic_dispatch/mod.rs:466-472`
  `enter_connected_demand` treats itself as the ROOT whenever `!state.active.get()` and
  calls `state.begin(work_limit, query_depth_limit)`, which sets `query_depth` back to `0`.
- `crates/verter_session/src/project_semantic_dispatch/mod.rs:1940`
  the depth is only charged for a query boundary, and only when the incoming key is not an
  exact same-path re-entry.

`ProjectSemanticDispatch::new(...)` is called from **30+ production sites**, among them:

- `crates/verter_session/src/component_meta_materialize.rs` (13 sites, e.g. `:2631`, `:2695`,
  `:2805`, `:2901`, `:3009`, `:3060`, `:3521`, `:3692`, `:3777`, `:3876`, `:3970`, `:4242`)
- `crates/verter_session/src/component_meta_resolution_policy/core.rs:168`
- `crates/verter_session/src/component_meta_resolution_policy/mod.rs:330`
- `crates/verter_session/src/host_manage/component_meta_methods.rs:1279`, `:1648`
- `crates/verter_session/src/host_manage/component_meta_methods/macro_output_expansion.rs:236`,
  `:286`, `:351`
- `crates/verter_session/src/host_manage/jsdoc_resolve.rs:220`, `:389`
- `crates/verter_session/src/meta_resolve/graph_predicates.rs:1082`, `:1184`
- `crates/verter_session/src/meta_resolve/macro_member_walk.rs:123`
- `crates/verter_session/src/meta_resolve/materialize/field_types.rs:37`, `:71`
- `crates/verter_session/src/meta_resolve/slot_binding_graph.rs` (slot-binding synthesis)
- `crates/verter_session/src/host_construction.rs:533`
- `crates/verter_session/src/host_resolve_type_audit.rs:276`, `:281`

If any of those sites is reachable from *inside* a build that is already charging depth on
the same thread, the new instance starts a fresh `ConnectedDemandState`, `state.active` is
false for it, `begin()` runs, and the depth restarts at 0 — while the native stack, which is
the resource actually at risk, keeps growing.

## Reproduction

No self-contained reproduction is known. What was observed:

- Instrument `execute_via_cold_build_helper`
  (`crates/verter_session/src/project_semantic_dispatch/mod.rs:1834`) with a probe that
  reports, on every entry, the current `connected_demand.query_depth` alongside the thread's
  native stack consumption measured from an anchored thread base.
- Drive a real multi-package Vue workspace through an LSP session (open an SFC, issue
  hover / definition / completion).
- Observe the pairs. Maximum `query_depth` seen: 13. Depth 24 was never reached.

A decisive reproduction would instead assert the invariant directly — see below.

## Evidence

Measured with a throwaway probe committed only to the investigation branch
(`perf/inv-opus`, commits `1a34847dd` and `31ad96631`, reverted by `dc959bb80`; the probe
source is recoverable with `git show 31ad96631:crates/verter_session/src/stack_probe.rs`).

Observed pairs, real workspace, debug profile:

| thread | native stack used | `query_depth` at that point |
|---|---|---|
| CPU-pool worker | 285 KiB | 5 |
| CPU-pool worker | 525 KiB | 9 |
| CPU-pool worker | 768 KiB | 13 |
| serve thread | 2632 KiB | 0 |

Raw artifacts were written to a session-scoped scratch directory and are **not durable**;
the numbers above are the whole of the evidence and are reproduced here so the record stands
alone.

## What was NOT proven

**No test was constructed that drives a nested `ProjectSemanticDispatch::new(...)` while a
budget is live and observes the depth restarting.** The mechanism is read from the source;
the runtime consequence is inferred. Treat this record as an evidence-consistent concern,
not an established bug.

The settling test:

> Install a `ProjectSemanticDispatch`, drive it to a known non-zero `query_depth`, and from
> inside that in-flight build reach a code path that constructs a second dispatcher on the
> same thread (the component-meta materialise sites above are the shortest route). Assert
> that the depth observed by the inner dispatcher is a continuation of the outer one, not 0.
> If it is 0, the budget is per-instance in effect as well as in declaration and the defect
> is confirmed.

## Proposed fix and falsifiable prediction

**Proposed fix (do not implement without the owner's direction):** make the connected-demand
state per-thread or per-request rather than per-dispatcher instance — e.g. a thread-local or
a value carried on `RequestContext` — so constructing another `ProjectSemanticDispatch` on a
thread that is already charging depth joins the live budget instead of starting a new one.

**Falsifiable prediction:** if the budget is being reset in practice, then after making the
state per-thread the maximum observed `query_depth` on a real workspace rises above 13 and
some queries begin returning `PartialReasonSet::CONNECTED_QUERY_DEPTH_LIMIT` where they
previously returned complete results. If the maximum stays at 13 and no new depth-limit
partials appear, the workload never exceeded the budget and this record can be closed as a
latent-only hazard.

## Blast radius

- **If fixed:** queries that today run deeper than 24 levels by virtue of a reset would start
  returning depth-limited partials. Anything that silently depended on the reset to complete
  would change its result — most plausibly deep component-meta materialisation. The change
  is therefore not behaviour-preserving and needs its own acceptance work.
- **If left alone:** the guard remains unreliable. It cannot be cited as the reason any
  particular recursion terminates, and it does not bound native stack growth at all — the
  depth counter and the stack are separate resources, and only the former is watched.
- **Related:** see `docs/arch/future/vue-public-instance-generic-bound-recursion.md`, a real
  unbounded recursion that this budget did not stop.
