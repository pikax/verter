# Multi-candidate storage proxy — pre-change overlay serialisation

Stage-0 sub-task 2(b) — pre-Stage-5 commitment of how today's
overlay machinery serialises concurrent sessions.

The fact-based cache plan (`R20`, Stage 5) introduces multi-candidate
storage: distinct overlay variants in the same query-identity slot
coexist via `ArcSwap<SmallVec<[Arc<Candidate<V>>; 2]>>` and concurrent
sessions never overwrite each other's results. Stage 0's job is to
record TODAY's behaviour so the Stage 5 test inverts a real observation
rather than an invented baseline.

## Conclusion

Today, **concurrent overlay sessions serialise through a single
`AtomicU64` CAS on `MetaProject::active_overlay_session`**. Only one
session's overlay view can be "applied to the shared host" at a time.
Switching ownership of the active overlay slot from session A to
session B requires reverting A's overlays from the host (one
`host.upsert(base_source)` per overlaid file) and then re-applying B's
overlays (one `host.upsert(overlay_source)` per overlaid file), in a
CAS-protected critical section. The shared `MaterializeStructureDb`
sees both sessions' writes interleaved through the same CAS-serialised
view, so a single cache slot is reused — never a multi-candidate.

This is the precise opposite of the Stage 5 target: today every overlay
acquisition is a host mutation; the target is a host-immutable
`SessionView` that puts each overlay variant into its own candidate
within the same slot.

## Code paths the investigation walked

1. `crates/verter_session/src/meta.rs:160-165` — `MetaProject` field:
   ```rust
   /// C15: lock-free tracking of which session's overlays are currently
   /// applied to the shared host. 0 = no session active. Replaces the
   /// retired `overlay_gate: Mutex<OverlayState>` — reads and writes
   /// are atomic, no Mutex contention between sessions.
   active_overlay_session: AtomicU64,
   ```
   The field is initialised to `0` in `MetaProject::new` (line 178).

2. `crates/verter_session/src/meta.rs:994-1054` —
   `MetaSession::with_overlay_target_context`. The full body
   implements the CAS contract:
   - If the calling session has overlays:
     - Atomic claim loop: `compare_exchange(current, self.id)`. On
       success, if `current != 0`, call
       `runtime.revert_other_session_overlays(current)` then
       `runtime.apply_own_overlays()` before proceeding.
     - On CAS failure, retry — another session raced and won.
   - If the calling session has NO overlays:
     - Atomic clear loop: `compare_exchange(current, 0)`. On success,
       call `runtime.revert_other_session_overlays(current)` to restore
       base state.
   - After the CAS region: `runtime.reapply_overlay_target(canonical)`
     if a canonical was supplied, then `runtime.refresh_view()`, then
     `f(&self.runtime)` executes inside the established view.

3. `crates/verter_session/src/session_runtime.rs:113-178` —
   `SessionRuntime::apply_own_overlays` and
   `SessionRuntime::revert_other_session_overlays`. Both methods walk
   the session's overlay map and call `host.upsert(...)` (or
   `host.remove(...)`) once per overlaid file. Each `host.upsert` is a
   full upsert against the shared `VerterHost`: it cascades through
   `project_type_store::evict_canonical` (per the inventory committed
   to `evict_canonical_inventory.json`), increments the store-view
   epoch (`bump_store_view_epoch`), and notifies the workspace.

4. `crates/verter_session/src/session_runtime.rs:60-67` — the
   per-session `view_writer_lock` is a `parking_lot::Mutex<()>` that
   serialises view publications WITHIN a single session. Inter-session
   contention happens on the project-wide
   `active_overlay_session` CAS (point 2 above), NOT on
   `view_writer_lock`.

## Observable contention signal

The CAS serialisation is observable in three ways, any of which a
Stage 5 multi-candidate test can invert:

| Signal | Pre-change observation | Post-change (Stage 5) observation |
|---|---|---|
| `active_overlay_session.load(Acquire)` | Exactly one session id is non-zero at any wall-clock instant during a critical section. Transitions are CAS-claim / CAS-release. | The atomic is RETIRED. The Stage 5 cutover removes the field. |
| Number of `host.upsert` calls per overlay swap | `O(N)` where `N` is the count of overlaid files on the outgoing session plus the incoming session. Steady-state with `S` sessions alternating: `O(S × N)` upserts per swap loop. | `0`. No `host.upsert` runs from any query path. |
| `MaterializeStructureDb` slot at a hot key | One value at a time. Each session's overlay overwrites the previous session's value when the CAS rotates. Re-acquiring the prior session's view recomputes. | Up to 4 candidates per slot (R20 cap). Concurrent sessions read distinct candidates; no recompute thrash. |

## Stage 0 characterisation evidence

A small test in `crates/verter_session/tests/path_precise_invalidation_baseline.rs`
exercises this directly. The Stage-0 characterisation:

1. Constructs a host on a single canonical with a base source.
2. Creates two `MetaSession` handles, S1 and S2, on the same
   `MetaProject`.
3. Each session installs a differing overlay on the same canonical
   (different `defineProps` shapes).
4. Each session, in turn, calls
   `with_overlay_target_context(canonical, |runtime| { ... })`.
5. The characterisation observes:
   - `MetaProject::active_overlay_session.load(Acquire)` flips between
     `S1.id` and `S2.id` as control transfers between the sessions.
   - The `host.upsert` count, captured via a request observer, grows
     linearly with the number of overlay swaps. Each swap incurs
     **two** `host.upsert` calls per overlaid file: one revert (to the
     base source) and one re-apply (to the next session's overlay).

That second observation is the discriminating signal Stage 5 must
invert: post-change, the swap loop produces **zero** `host.upsert`
calls and the candidate set under the hot
`MaterializeStructureCacheKey` reports two candidates instead of one.

## What gets retired in Stage 5

From the plan's Legacy Deletions table:

| Symbol / file | Stage | Replaced by |
|---|---|---|
| `active_overlay_session` + CAS | 4d | `SessionView` (R17) |
| `apply_own_overlays` / `revert_other_session_overlays` / `reapply_overlay_target` | 4d | `SessionView::source()` (R17) |
| `with_overlay_target_context` | 4d | Explicit `view` arg (R18) |
| `view_writer_lock` / `view_snapshot` / per-session `SessionView` epoch | 4d | `SessionView` trait (R17) |
| Session-scoped `resolved_meta_cache` | 4d | Multi-candidate host cache (R20) |
| `Mutex<FxHashMap>` substrate inside `ValidatedFactCache` | 5 | `DashMap + ArcSwap<SmallVec>` (R20) |
| `ValidatedFactCache.archived` + `view.checks_archive()` (2 call sites) | 5 | Multi-candidate FIFO + per-candidate fact validation (R20) |

The Stage-0 characterisation therefore pins TWO observable invariants
about the current tree that Stage 4d / Stage 5 are explicitly
deleting:

1. The CAS-serialised swap loop exists and is the single concurrency
   oracle for overlays today.
2. The shared `MaterializeStructureDb` carries exactly one entry per
   `MaterializeStructureCacheKey` at all times; concurrent overlays
   either overwrite or read the same entry; there is no
   multi-candidate isolation.

## File references (audited base SHA `ccc05223`)

- `crates/verter_session/src/meta.rs:160-165`
  (`MetaProject::active_overlay_session: AtomicU64`)
- `crates/verter_session/src/meta.rs:994-1054`
  (`MetaSession::with_overlay_target_context`)
- `crates/verter_session/src/session_runtime.rs:60-67`
  (per-session `view_writer_lock` / `view_snapshot`)
- `crates/verter_session/src/session_runtime.rs:113-137`
  (`apply_own_overlays`)
- `crates/verter_session/src/session_runtime.rs:139-178`
  (`revert_other_session_overlays`)
- `crates/verter_session/src/session_runtime.rs:180-208`
  (`reapply_overlay_target` — same upsert-per-target shape)
- `crates/verter_session/src/component_meta_materialize.rs:129-147`
  (`MaterializeStructureCacheKey` — single-entry today)
