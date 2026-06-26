# Hot-Path Materialize Fence — Stage-9 Deferral (TEMPORARY debt row)

**Status**: DEFERRED — the final hot-path reverse-materialization fence
`hot_path_never_calls_materialize_type_expr` is the named **Stage-9** deliverable, enabled LAST
after the residual materialize allowlist shrinks to 0. This row records the KNOWN per-iteration /
fixpoint residual — the C5 route fixpoint — as the named-debt anchor for that Stage-9 gate; the C5
fixpoint must move onto the node-domain interned-key compare before the fence can become
structurally true. The EXHAUSTIVE enumeration + classification of every hot materialization site
(which the future fence will forbid) is itself a Stage-9 fence-build determination — it is NOT
settled by this 8-D deferral row. **This is a TEMPORARY debt row** (per Rule-File Integrity) — it is
cleared when Stage-9 lands that conversion and enables the fence, not before.

**Ruling source (pre-sanctioned, binding)**: the codex 2-leg RESCOPE decision at
`/tmp/mom/STAGE8/RESCOPE/DECISION.md` — both legs exit 0, both `__DONE__`, STRONG convergence on
all 5 questions, ratified. Two parts of that decision sanction this move directly:

- **Q3 (X3 disposition)**: "eliminate in 8-D or formally move to Stage 9 with a named gate."
- **The re-scoped block list, item "8-D: final fence closure"**: "land
  `hot_path_never_calls_materialize_type_expr` ONLY when the temp allowlist (X3) is zero; else move
  that exact debt to Stage 9 with a named gate."

Block 8-D determined FIRST-HAND that the residual is **NON-ZERO (X3 ≠ 0)**: there is at least one
hot materialization site that the fence would forbid — the C5 route fixpoint (the known
per-iteration / fixpoint residual, inventoried below). The pre-sanctioned move therefore applies as
written: the fence is NOT landed in 8-D, and the known debt is recorded here against the named
Stage-9 gate. (8-D did NOT exhaustively enumerate and classify every hot materialization site — that
full audit is the Stage-9 fence-build's job; 8-D established the non-zero residual via the C5
fixpoint, which is sufficient to trigger the "move to Stage 9 with a named gate" branch.) No new
codex-DEFER ruling was needed for the move itself — the RESCOPE decision already authorised it; this
row only records it honestly (one-path governance, not a dual implementation).

## The known fixpoint residual (the named-debt anchor)

The KNOWN per-iteration / FIXPOINT residual hot reverse-materialization site is the **C5 route
fixpoint** — the one site block 8-D determined first-hand that re-materializes a `TypeExpr` per loop
iteration to drive a fixpoint convergence decision:

- `crates/verter_session/src/resolver_core/component_meta_query_engine/route_keys.rs:899-950`,
  `fn solve_or_project_leaf_expr_until_stable`. It keeps a `TypeExpr` cursor
  (`let mut current = expr.clone()`), loops `for _ in 0..3`, and per iteration calls a TWO-ARM
  lower+project tail: the primary
  `crate::meta_resolve::lower_and_project_to_expanded_via_host_threaded(...)` `.or_else(...)` the
  fallback `crate::meta_resolve::project_expr_surface_expr_via_host_threaded(...)`
  (the fallback call-open is `route_keys.rs:931`; its `Expanded` base / `Expanded` terminal /
  `Published` mode arguments are `route_keys.rs:935-937`) — then converges by
  `TypeExpr::PartialEq` (`if next == current { return Some(next); }`).
- C5 is treated as ONE LOGICAL fixpoint allowlist entry, but its per-iteration materialization can
  route through MULTIPLE adapter legs depending on which loop arm fires and which fast-path the
  fallback takes:
  - the **surface publication adapters** — `materialize_published_node` (the helper, defined at
    `resolver_core::component_meta_query_engine::surface.rs:56-63`; the
    `cap.materialize_output_type_expr(node).map(|raised| raised.into_type_expr(&cap))` snippet is
    `surface.rs:61-62`, with the cap minted at `surface.rs:60`) driven from TWO call sites: the
    primary `lower_and_project_to_expanded_published` tail (`surface.rs:105`,
    `.then(|| materialize_published_node(&dispatch, result_node))`) reached by the
    `lower_and_project_to_expanded_via_host_threaded` route, and the
    `project_expr_surface_expr_published` adapter (`surface.rs:170-171`, also calling
    `materialize_published_node`) on the fallback's surface route — BOTH through the sealed
    `MetaQuerySurfaceOutputCap` (minted at `surface.rs:60`);
  - the **registry fast-path** inside the fallback `project_expr_surface_expr_via_host_threaded`
    (`meta_resolve::dispatch_helpers.rs:885-915`), which itself ATTEMPTS multiple materializing legs
    in a single iteration: first the registry indexed-access / utility route
    (`dispatch_helpers.rs:889` → `project_route_surface_expr_via_host_threaded`), which materializes
    through the sealed `MetaQueryRegistryOutputCap` at TWO registry legs (cap minted at
    `registry_decl.rs:1209`): the registry MEMBER-PATH route
    (`cap.materialize_output_type_expr(node).map(|raised| raised.into_type_expr(&cap))` at
    `registry_decl.rs:1210-1211`, with `.filter(dispatch_route_expr_is_materialized)` at
    `registry_decl.rs:1212`) AND the registry Pick/Omit UTILITY route (its own
    `MetaQueryRegistryOutputCap::new` + materialize-map-filter at `registry_decl.rs:1405-1408`) — and
    then FILTERS — so a rejected (`None`) materialized registry result is followed in the SAME iteration
    by a retry of `lower_and_project_to_expanded_via_host_threaded` (`dispatch_helpers.rs:897-898`) and
    then a fall-through to `project_expr_surface_expr_published` (`dispatch_helpers.rs:907`), each of
    which can materialize again at its surface sink.

So C5 admits ONE accepted `next` cursor per SUCCESSFUL iteration (≤ 3 iterations), but a SINGLE
iteration may ATTEMPT multiple materializing legs before one is admitted — the fallback's
registry-route materialize-then-filter, then the lower-and-project retry, then the surface-published
route — each materializing at a sealed output sink (the surface caps OR the registry cap, depending
on the arm/route taken). C5 stabilises the leaf by comparing successive admitted `TypeExpr` cursors
structurally. For `hot_path_never_calls_materialize_type_expr` to become structurally true, Stage-9
must remove ALL per-iteration materialization ATTEMPTS across EVERY leg (not just the one admitted
materialization) — the surface publication adapters AND every materializing leg of the registry
fallback — moving the C5 fixpoint onto the node-domain interned-key compare.
Each of these legs is the same sealed per-sink `OutputProjector` capability that block 8-A3 landed;
the fence is not yet true because C5 still drives them inside a per-iteration fixpoint, not because
the sinks themselves are unguarded.

The EXHAUSTIVE enumeration + classification of all hot materialization sites — separating the
genuine terminal one-shot output sinks (permitted by the fence) from any other hot
materialize-then-decide sites that would also have to move onto node-domain facts — is a STAGE-9
fence-build determination, not settled by this 8-D deferral row. (There ARE other hot
materialize-then-decide sites in the engine — e.g. `project_direct_utility_surface_shape`'s inner
`projected_target_shape` (`route_keys.rs:551-590`), which materializes a `TypeExpr` to derive an
`ExpandedObjectShape` and decide `Pick`/`Omit`/utility-route surface semantics: its FALLBACK BRANCH
(`route_keys.rs:571-590`) materializes via `project_expr_surface_expr_via_host_threaded`
(`route_keys.rs:579`) then `type_expr_to_object_shape` (`route_keys.rs:589`), and its PRIMARY route
`project_expr_surface_shape_via_host_threaded` — whose GENERAL node-core arm returns a shape with no
`TypeExpr` materialization — STILL materializes a `TypeExpr` on its REGISTRY-route arm
(`dispatch_routed_expr_surface_expr` → `type_expr_to_object_shape` at `registry_decl.rs:1311-1320`),
so the primary route is NOT a never-materializes path either — so this row does NOT assert "every
other materialize caller is a one-shot sink".) This row records the KNOWN
per-iteration / fixpoint residual (C5) as the named-debt anchor; the full audit that the Stage-9
fence formalises is owned by Stage-9.

## Why deferral is sound (no correctness gap)

The C5 deviation is behaviour-PRESERVING at runtime — its debt is a hot-path PERF objective AND a
Stage-9 fence-governance blocker (the fence `hot_path_never_calls_materialize_type_expr` cannot
truthfully land while C5 still materializes per iteration):

- C5 already materializes at a SEALED output sink (capability-gated through
  `MetaQuerySurfaceOutputCap` on the surface legs and `MetaQueryRegistryOutputCap` on the registry
  fast-path), produces the correct published `TypeExpr`, and the convergence/gating decision
  (fixpoint over successive cursors, bounded to 3 iterations) is correct. There is no semantic
  divergence and no second resolver — there is ONE resolver; C5 just keeps a `TypeExpr` cursor
  instead of a node cursor.
- The DEFERRED objective is the PERF rewrite: move C5 onto the interned-key `RaisedShapeKey`
  node-domain compare so the fixpoint stabilises on graph identity and NO mid-flight `TypeExpr` is
  materialized per iteration. That is purely a hot-path allocation/throughput improvement; it
  changes how C5 decides convergence, not what it publishes.

Because C5 is a known, behaviour-preserving per-iteration perf deviation at a sealed sink (and the
residual is non-zero), landing `hot_path_never_calls_materialize_type_expr` now would be a FALSE
guard (it would assert a fence that is not yet structurally true). Deferring it to Stage-9 — where
the C5 conversion removes its per-iteration materialization and the fence-build settles the full hot
materialization audit — keeps the guard honest.

## The named Stage-9 gate (the deliverable)

`hot_path_never_calls_materialize_type_expr` — enabled LAST in Stage-9, only AFTER the materialize
allowlist is MECHANICALLY zero: C5 moves onto the node-domain interned-key compare AND every OTHER
hot materialization entry that the Stage-9 fence-build's exhaustive audit identifies is removed.
C5 is the KNOWN named-debt anchor, NOT the sufficient condition on its own — the fence-build's
exhaustive enumeration may surface additional hot materialize-then-decide sites (per lines 88-101)
that must ALSO be converted before the allowlist reaches 0 and this guard can be enabled. It is
enabled alongside the global Unknown-as-control-flow fence. This is exactly Stage-9's existing
acceptance ("ZERO transitional materialize allowlist entries") and its already-recorded guard line in
`docs/arch/parselower-design.md` (`hot_path_never_calls_materialize_type_expr` enabled LAST, after
allowlists → 0). No new stage, no new guard surface is introduced by this row — it names the
existing Stage-9 gate as the owner of the known fixpoint-residual debt (C5), with the full hot
materialization audit owned by the Stage-9 fence-build.

## Closure criterion

This debt row is DELETED when Stage-9 lands the C5 node-domain conversion (the route fixpoint
stabilises on the interned `RaisedShapeKey` with no mid-flight materialization) AND removes every
OTHER hot materialization entry its fence-build audit identifies — so the materialize allowlist is
MECHANICALLY zero — then enables `hot_path_never_calls_materialize_type_expr` (last) and the global
Unknown fence. C5 is the KNOWN named-debt anchor, but its conversion alone is NOT the closure
condition: the allowlist reaching 0 (after the fence-build's exhaustive audit converts every hot
site it finds) is. At that point the fence is structurally TRUE and this debt row is obsolete.
