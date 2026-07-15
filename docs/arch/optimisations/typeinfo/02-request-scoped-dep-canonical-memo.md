# T2 — Request-scoped positive memo for `resolve_eval_dependency_canonical`

**Level:** macro. **Risk:** low-medium — probe-side-effect audit done; positive-only caching.
**Reference implementation:** branch `perf/t2-dep-canonical-memo`, commit `4769b876a` (measurement machine).

## Problem (profiler evidence)

`VerterHost::resolve_eval_dependency_canonical` (`crates/verter_session/src/host_manage/eval_env.rs:664`)
— the TS-first dependency-canonical normalizer (`.js` → `.d.ts` etc. via candidate probing) — is
**42.6 % of total pass CPU inclusive**. Callers: `observe_content_pinned_indexed` (15.2 % — every
artifact observation normalizes first), `route_shallow_state_serve` (7.5 %),
`resolve_imported_type_root_with_facts_with_store_view` (8.4 %), `build_named_type_export_route_entry`
(4.5 %), `overlay_artifact_identity` (2.3 %). Each call probes up to 14 candidates through
`analysis_source_exists` (artifact store `get_any` + scheduler + real-filesystem `file_exists`).
The same dependency canonicals are re-normalized tens of thousands of times per request.

## Design (as landed)

- `RequestContext` (`crates/verter_session/src/request_context.rs`) gains
  `dep_canonical_memo: parking_lot::Mutex<FxHashMap<String, String>>` — matching the file's existing
  per-request map patterns; Sync across the scheduler workers the request fans out to (the SAME
  `Arc<RequestContext>` is installed on every worker via `RequestContextLike::install_tls`).
  Lifetime = exactly one top-level audited host request (created per request, TLS guard restores prior
  slots on drop).
- `resolve_eval_dependency_canonical`: consult the memo on entry (when a request context is installed);
  insert ONLY computed `Some(resolved)` results. **POSITIVE-ONLY: `None` results are never memoised**
  (negative caching has staleness hazards around mid-request artifact publication). Empty ids and
  `is_raw_import_specifier_id` inputs bypass the memo. No context → identical behavior to today.
- Second caller of the probing core (`host_executor.rs:295`, `extract_deps`) intentionally NOT
  memoised: its probe closure is workspace-`file_exists`-only (no scheduler state, no artifact lane,
  no store-view gate) — different probe semantics; sharing the memo would be unsound in both directions.

## Probe-side-effect audit (PASS — re-verify on a moved base)

`analysis_source_exists` (`host_manage/eval_program.rs:~371`) is mutation-free:
`effective_file_state` → `scheduler.try_get_source` → `node.current_source()` are pure atomic/arc-swap
reads; `store_view_allows_current_whole_hash` is currently constant `true`;
`artifact_only_entry_exists` = DashMap read + `get_any` (telemetry bumps only) + `ws().file_exists`;
`file_exists` populates only the VFS-internal dir-index (workspace-owned readdir memo, invalidated by
the VFS change pipeline). No `ensure_loaded`, no host semantic-cache population — memoizing skips no
required side effect.

## Test contract (landed, TDD red→green)

(a) compute-once within an installed request context (probe counting via a `CountingWorkspace`);
(b) no-context path unchanged; (c) `None` is never memoised and re-probes; (d) per-request isolation
(fresh context → fresh map). Discrimination proven by reverting the wiring (tests (a)/(d) fail).
Note: `cold_synthesis_terminates_within_500ms_for_50_member_heritage` is a pre-existing load-sensitive
wall-clock test that can flake under parallel load (passes isolated).

## Measured result (this machine)

Smoke (12 components, repeats=3 both sides): steady p50 1305 → 948 ms (−27 %); per-component medians
−20–47 % for 9/11 successes.
Full pass (post-fix protocol, median of 3 interleaved runs): steady 20 480 → **12 032 ms (−41.2 %)**;
p50 42.7 → 27.4 ms; p95 345 → 213 ms; max 1985 → 1305 ms; peak RSS 720 → 698 MB. (The earlier one-round
preliminary read −3.6 % under decaying ambient load — discard it; these are the authoritative numbers.)
