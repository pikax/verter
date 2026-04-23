# Corpus Performance Investigation: Per-Prop Type Walker Bottleneck

**Date:** 2026-04-23
**Branch:** `refactor/semantic-db-overhaul`
**Trace data:** `D:/tmp/corpus-trace/traces/` (177 nuxt-ui components, 60s/component cap)

## TL;DR

The per-prop walker is the bottleneck. Two independent dimensions of redundancy
contribute:

1. **Cross-phase redundancy** is the dominant cost. The compute phase already
   materializes each prop's member route (e.g. `loadingAnimation` resolved to
   `Table['variants']['loadingAnimation']` in 41.9ms). The rematerialize phase
   then re-walks the same prop and burns **4521ms** on the same `loadingAnimation`
   — a 108× slowdown for the same input. The compute-phase memo is dropped
   between phases.

2. **Cross-prop redundancy** is real but smaller. Sibling props that share a
   subgraph (`loadingAnimation` + `loadingColor`, both `Table['variants'][...]`)
   re-walk the shared `Table → variants` portion. The walker is not
   path-precise: a single prop produces 2680 child trace events for one indexed
   access, almost all of them looking up types that have already been resolved
   for the other sibling.

The **earlier theory** that `resolve_imported_type_root` redundancy was the
bottleneck is **wrong**. Import resolution is 366ms of 19000ms (1.9%) for
Table — the host-owned `ImportedRootDb` is doing its job. The expensive work
is downstream of import resolution, in the per-prop type-graph walker.

---

## Background: trace data used

| Component | Wall | Compute | Rematerialize | Top prop |
|---|---|---|---|---|
| Table.vue | 19,050 ms | 4,016 ms | **14,409 ms** | loadingAnimation 4521 ms |
| EditorToolbar.vue | 17,600 ms | **8,897 ms** | 7,969 ms | items 7965 ms |

Both components have a **single dominant prop** that consumes 30–40 % of
wall time. In Table, the rematerialize phase is the dominant cost. In
EditorToolbar, the compute phase dominates. The dominant phase is data-shape
dependent — it is not the case that one phase is always the slow one.

---

## D3.Q1 — Is the type walker re-expanding shared subgraphs across props?

**YES — within the rematerialize phase, sibling props re-walk overlapping
subgraphs.**

Two Table.vue props share the same root type `Table['variants'][…]`:

| Prop | rematerialize_public_prop_type | child events | ensure_indexed_ready_fast_hit | prepared_type_decl_result |
|---|---|---|---|---|
| `loadingAnimation` | 4521.6 ms | 2680 | 970 | 544 |
| `loadingColor` | 4247.8 ms | (similar) | 849 | 302 |
| `ui` | 2818.0 ms | (similar) | (similar) | (similar) |

The 970 + 849 + … `ensure_indexed_ready_fast_hit` events (one per shallow
state lookup) within back-to-back props strongly suggests the walker reaches
the same `Table` declaration repeatedly for each sibling prop, re-runs its
shallow-state lookup, and re-builds the same intermediate eval state.

`ComponentMetaQueryEngine::materialize_memo` is documented as the cache that
should prevent this, but the trace evidence shows it is not preventing
repeated descent into the shared `Table` / `ComponentConfig<theme, A>`
intermediate types when the terminal selectors differ. The memo appears to
key on `(prop_name, terminal_route)` rather than on the intermediate hops
that all three props share.

**Verdict:** the cross-prop sharing is real but the cost-per-event is small
(0.002 ms median for `ensure_indexed_ready_fast_hit`). 970 fast hits
multiplied by 4 µs each is ~4 ms — three orders of magnitude smaller than
the 4521 ms total for the prop. So cross-prop redundancy alone does **not**
explain the per-prop wall time.

---

## D3.Q2 — Can indexed-access projection short-circuit?

**NO — the walker is currently not path-precise enough for the heaviest
props.**

`Table['variants']['loadingAnimation']` resolves a two-level indexed access
into an intersection type. The CLAUDE.md "Macro Type Traversal Rule"
mandates path-precise projection: the terminal hop should run in the
caller's mode while intermediate hops run in `Navigate`, and
"non-contributing intersection arms are ignored". The trace evidence shows
this rule is **not** being honoured at the rematerialize boundary.

Inside the 4521 ms `rematerialize_public_prop_type` span for
`loadingAnimation`, span 15937 has:

- 970 `ensure_indexed_ready_fast_hit`
- 544 `prepared_type_decl_result`
- 204 `current_eval_state`
- 202 `materialize_prepared_decl_bundle`
- 182 `resolve_local_import_symbol_target`
- 182 `resolve_direct_type_reexport_target`
- 82  `read_analysis_source`
- 82  `external_type_analysis`
- 82  `base_eval_env`
- 60  `resolve_local_export_symbol_target`
- 52  `authoritative_import_route_result`
- 22  `resolve_imported_type_root`

That is 2680 children for **one prop's** indexed access. For an
indexed-access projection, only the `variants` arm of the intersection is
contributing — the other intersection arms (`TableHTMLAttributes`, the
inline object, etc.) cannot supply a `loadingAnimation` member. A
path-precise walker should:

1. Resolve `Table` (one shallow lookup).
2. Project `['variants']` — at this hop, immediately discard intersection
   arms that do not have a `variants` member.
3. Project `['loadingAnimation']` on the surviving arm only.

The trace shows the walker visiting 82 distinct
`(read_analysis_source, base_eval_env, external_type_analysis)` triples,
which means it is touching 82 separate canonical files to resolve this
single prop. That number is consistent with walking the entire `Table`
intersection tree (which transitively imports the `@tanstack/table-core`
feature interfaces) rather than projecting the `variants` arm only.

`@tanstack/table-core` exposes `Table<TData>` as an intersection of ~15
feature-Options interfaces. Resolving the `variants` arm should not require
walking those 15 interfaces; it should only require resolving the
`ComponentConfig<typeof tv, MissingAppConfig>` half of the intersection.

**Verdict:** indexed-access projection IS short-circuitable in principle
(the rule already exists in CLAUDE.md), but the rematerialize-phase code
path appears not to short-circuit. Confirming this requires reading the
walker code at `materialize_member_route_*` callsites — outside the scope
of this investigation.

---

## D3.Q3 — Is the rematerialize phase re-doing work the compute phase already cached?

**YES — and this is the dominant source of slowdown, not Q1.**

The compute phase already runs `materialize_define_props_member` per prop
(inside `materialize_component_meta_macro_shape_member_types`, which is the
inner driver of compute). Compare timings for the same prop across phases:

| Prop | Compute `materialize_define_props_member` | Compute `materialize_member_route_current` | Rematerialize `rematerialize_public_prop_type` | Cross-phase ratio |
|---|---|---|---|---|
| Table `loadingAnimation` | 41.9 ms | 36.5 ms | **4521.6 ms** | **108×** |
| Table `data` | 888.2 ms (large) | (in line) | (small after compute did the work) | n/a |
| EditorToolbar `items` | 7697.2 ms | 7696.6 ms | 7964.6 ms | ~1× |

Two distinct patterns:

- For `Table.loadingAnimation`, compute did the cheap path-precise work
  (41.9 ms), but the rematerialize phase re-walked everything from the
  beginning (4521.6 ms). The 108× factor matches the difference between
  "look up a memoised result" and "rewalk the entire intersection tree".
- For `EditorToolbar.items` (`ArrayOrNested<EditorToolbarItem>`, recursive
  generic), both phases pay the full cost. There is no cheap precomputed
  result for compute to memoise, so rematerialize cannot skip work.

The architecture has two `ComponentMetaQueryEngine` lifetimes (one per
phase). Each engine builds its own `materialize_memo` and the memo is
dropped at the end of the phase. The compute-phase memo entry for
`Table['variants']['loadingAnimation']` is therefore not visible to the
rematerialize phase, which restarts from the unevaluated indexed-access
expression.

The compute-phase result IS available — the trace shows
`materialize_member_route_current_result` carrying a fully resolved
`current_materialized=…` value. But that value is consumed locally by
`materialize_define_props_member` and not promoted into a host-owned cache
that the rematerialize phase could read.

**Verdict:** the dominant fix is to share materialization results across the
two phases via a host-owned cache, keyed on `(canonical_owner_id,
member_name, route_signature)`. This is consistent with the host-owned cache
principle (the cache lives on `VerterHost::project_type_store()`, not on
either engine).

---

## D3.Q4 — What fraction of walker time is genuine complexity vs redundant re-expansion?

**For Table: ~99 % redundant. For EditorToolbar: ~50 % redundant.**

Lower-bound estimate of the genuine cost: the compute phase wall time for
the same prop. The compute phase's `materialize_define_props_member` is
the closest analogue to the rematerialize prop walk — both walk the prop's
declared type and produce a materialized result.

| Prop | Cheapest single-pass walk (compute) | Re-walked total (compute + rematerialize) | Redundant fraction |
|---|---|---|---|
| Table `loadingAnimation` | 41.9 ms | 41.9 + 4521.6 = 4563.5 ms | **99.1 %** |
| Table `loadingColor` | (similar) | (similar) | ~99 % |
| EditorToolbar `items` | 7697 ms | 7697 + 7965 = 15,662 ms | ~49 % |

For Table's hot props, the genuinely-needed work is a few tens of
milliseconds per prop. The remaining 99 % is the rematerialize phase
repeating the compute phase from scratch.

For EditorToolbar's `items` prop, the type is genuinely complex
(`ArrayOrNested<EditorToolbarItem>` where `EditorToolbarItem` is a deeply
recursive type with conditional and mapped subtypes). Both phases pay
~7.7 s, and both are doing real work — there is less redundancy to remove.
This is a separate problem (genuine type complexity) that requires either
type-walk depth limits, lazy expansion, or a coarser memoisation key.

**Verdict:** for the most common shape of slow prop (indexed access into
a complex type, where compute already did the work), removing cross-phase
redundancy alone would cut Table wall time by roughly **~10 s out of 19 s
(53 %)**. That estimate is the sum of `loadingAnimation` (4521) +
`loadingColor` (4248) + `ui` (2818) rematerialize times, all of which
should drop to near-zero with a host-owned cross-phase cache.

For EditorToolbar, removing cross-phase redundancy of `items` (which is
already small relative to compute's 7.7 s) would only save ~250 ms in the
rematerialize phase. To meaningfully speed up EditorToolbar, the compute
phase itself needs to short-circuit deeply-recursive type expansion.

---

## Recommended fix plan (NOT implemented in this commit)

Per the plan, this commit produces this investigation only. The actual
fix lands in a follow-up commit. The recommended approach:

### Fix 1 (P0) — Cross-phase materialization cache (host-owned)

**Owner:** `verter_session::ProjectTypeStore`

**Cache identity:** `(owner_canonical_id, member_name, expanded_route_signature)`
keyed by `whole_hash` of the owner's `IndexedReady` and dependency-signature
revalidation via `HostFenceValidator`.

**Cache value:** the `current_materialized` artifact that the compute phase
produces in `materialize_member_route_current_result`.

**Producer:** `ComponentMetaQueryEngine` during the compute phase, after
`materialize_member_route_current` completes for a prop. Promote the result
into the project-type-store before the engine's local memo is dropped.

**Consumer:** the rematerialize phase, before invoking
`rematerialize_public_prop_type`. Look up the cache; if hit, skip the
walker entirely and use the cached materialized type.

**Cache invariants** (per the project-global cache rule in CLAUDE.md):
- Immutable `Arc<MaterializedRoute>` payload.
- Validated cache writes record dep-signatures; warm reads revalidate via
  `HostFenceValidator`.
- Path-independent: the same materialized route from two different entry
  points must populate the same cache entry.
- Cancelled / interrupted / partial results must NOT be promoted.

**Expected impact (Table.vue):** rematerialize wall drops from 14.4 s
to ~1 s (cache hits replace 60 walker invocations).

### Fix 2 (P1) — Path-precise indexed-access projection in rematerialize

**Owner:** `verter_session::resolver_core::component_meta` (the
materialize_member_route_* family).

**Behaviour:** for `Indexed_access(Intersection([…]), Literal(name))`, the
walker should discard intersection arms that cannot contribute a `name`
member before recursing. CLAUDE.md already mandates this via the Macro
Type Traversal Rule but the rematerialize-phase code path appears not to
honour it.

**Expected impact:** even if Fix 1 leaves cold-cache cases (first prop in
a session), the cold-cache cost for `loadingAnimation` drops from 4521 ms
to ~50 ms.

### Fix 3 (P2 — different problem) — Type-complexity budget for genuine recursion

**Scope:** props like `EditorToolbar.items` with deeply recursive generic
types where both phases pay the cost. Requires either:

- A depth/budget limit in the type walker, or
- A coarser memo key that catches structural cycles (`materialize_memo`
  currently keys on a finer identity than the structural cycle), or
- Lazy expansion: stop expanding past the requested member-route hop.

This is a separate, smaller win after Fixes 1 and 2 land.

### Why NOT a per-request resolve_imported_type_root memo

The original plan proposed adding a request-local
`resolve_imported_type_root` memo. This is rejected here for three reasons:

1. **Wrong target.** Import resolution is 366 ms of 19,000 ms wall time
   for Table (1.9 %). Even a perfect cache here saves ~366 ms. The walker
   is the bottleneck; import resolution is downstream noise.
2. **Violates host-owned cache principle** (per `feedback_host_owned_caches`):
   request-local caches with `RefCell` / per-request `FxHashMap` are
   prohibited. Fixes must land in host-owned caches.
3. **Existing host caches already cover this.** The trace shows
   `ensure_indexed_ready_fast_hit` firing 970 times in 4 ms total — these
   are essentially free. The host-owned `ImportedRootDb` is doing its job.

---

## How to validate the fix

After Fix 1 lands, the corpus benchmark (`scripts/benchmark/trace-component-corpus.mjs`)
should show:

- `trace_materialize_ms` for Table.vue drops from ~854 ms to ~50 ms (the
  rematerialize phase becomes a series of cache hits).
- Specifically, `rematerialize_public_prop_type` per-prop maxima drop from
  ~4521 ms to ~50 ms for `loadingAnimation` and `loadingColor`.
- Total wall for Table.vue drops from 19,050 ms to ~6 s (compute still
  dominant).
- Total corpus wall drops from 516 s to ~250 s (rough lower bound — many
  props benefit from the same cache).

After Fix 2 lands, the FIRST prop walk in a session for any
`Table['variants'][…]` prop drops from 4521 ms to ~50 ms (the path-precise
walker no longer descends into non-contributing intersection arms).

Fix 3 only affects components like EditorToolbar where compute itself is
slow — measure separately on those.

---

## Trace data references

- Table.vue trace: `D:/tmp/corpus-trace/traces/src__runtime__components__Table__vue.trace.log` (31,847 lines)
- EditorToolbar.vue trace: `D:/tmp/corpus-trace/traces/src__runtime__components__EditorToolbar__vue.trace.log` (63,661 lines)
- Per-prop child-event analysis script: `D:/tmp/analyze-walker.mjs`
- Span IDs cited above are stable for the captured traces; they will not
  match a fresh run.
