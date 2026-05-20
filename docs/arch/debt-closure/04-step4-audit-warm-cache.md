# Step 4 — Audit warm-cache short-circuit

Source plan: `D:/tmp/architectural-debt-closure.md` (revision 10), Step 4.

## What landed in this commit

**Sub-task 4.1 — `RequestAuditRecord::from_cache`.**
`crates/verter_session/src/component_meta_audit/mod.rs::RequestAuditRecord`
gains a `pub from_cache: bool` field. Serde-defaulted for back-compat
with old audit payloads:

```rust
#[serde(default, skip_serializing_if = "std::ops::Not::not")]
pub from_cache: bool,
```

`bool::default() == false` matches cold-resolver behavior so existing
audit consumers keep working. The TS binding emits the field as
`from_cache: boolean` (ts-rs warns about the serde attribute but the
field is still emitted; back-compat is preserved on the wire).

**Sub-task 4.2 — `CachedComponentMetaResult` + `ResolutionTemplate`
types.** `crates/verter_session/src/component_meta_result_db.rs` gains
two new types per D4.1:

- `ResolutionTemplate` — sanitized snapshot of `ResolvedComponentMetaState`.
  Excludes per-request fields (`request_id`, `compute_audit`) and the
  `FileAnalysisSnapshot` (reloaded from `ProjectTypeStore::indexed()`
  on rehydrate). Includes the content-addressed sidecars
  `surface_identities` and `origin_graph`.
- `CachedComponentMetaResult` — `{ analysis, resolution_template,
  canonical_id, whole_hash }`. The DB generic migrates from
  `ComponentMetaResultDb<ComponentMetaAnalysis>` to
  `ComponentMetaResultDb<CachedComponentMetaResult>` so warm-cache
  hits on the audit-enabled path can rehydrate both halves without
  rerunning the cold resolver.

The two existing consumers (`try_component_meta_cache_hit` for the
plain `get_component_meta` path, `publish_component_meta_cache_entry`
for cold-resolver tail) migrate to the new payload shape with one
indirection added (`entry.payload.analysis` instead of
`entry.payload`).

**Sub-task 4.3 — cache-hit short-circuit in
`VerterHost::get_component_meta_with_resolution`.** A new private
method `try_with_resolution_cache_hit` runs AFTER the
`RequestContextGuard::install` (so `current_request_id()` returns the
fresh id even on the warm path):

1. Look up the result DB at `(canonical, whole_hash)`.
2. Revalidate the entry's `ReadSetSignature.facts` against the live
   `StoreView` via `StoreView::validates_fact_signature` (and, for
   cache layers that carry self-roots, also
   `ReadSetSignature::validate_with_self_roots`).
3. Rehydrate the cached `ResolutionTemplate` into a fresh per-request
   `ResolvedComponentMetaState` (snapshot reloaded from
   `ProjectTypeStore::indexed()`).
4. Synthesize a `RequestAuditRecord { from_cache: true, total_ms: 0.0,
   request_id: <fresh>, ... }` and publish into `host.audit_records`
   (when audit is on) so consumers via
   `take_audit_record(resolution.request_id)` work uniformly.
5. Return `(analysis, resolution)` to the caller.

On miss, stale dep_signature, or the bounded eviction-race rehydrate
miss, returns `None` and the caller falls through to the cold resolver.
The cold-resolver tail publishes the new
`CachedComponentMetaResult` back into the DB so subsequent identical
calls short-circuit through this path.

**Sub-task 4.0 — entry-point enumeration.** Per Codex P1 #1, the
audit-enabled resolver fans out across:
- `VerterHost::get_component_meta_with_resolution` (the implementation
  is here; this commit changes the cache-hit logic).
- `MetaProject::get_component_meta_with_resolution` (`meta.rs`) — thin
  wrapper that delegates to `VerterHost`; no change needed.
- `AuditedRequest::resolve` — already a thin wrapper over
  `VerterHost::get_component_meta_with_resolution` +
  `take_audit_record(resolution.request_id)`. No change needed; the
  synthesized `from_cache` record published from the cache-hit path
  flows through the existing `take_audit_record` contract.
- `verter_napi` / `verter_wasm` audit-enabled exports — they go through
  `AuditedRequest::resolve` which goes through
  `get_component_meta_with_resolution`. No additional plumbing required.
- LSP `hover_provenance` — consumes `RequestAuditRecord` (now includes
  `from_cache`); rendering may need a follow-up to surface the warm
  state visually, but the structural addition is back-compat.

## FAIL-FIRST tests

`crates/verter_session/src/audit_warm_cache_tests.rs` — three tests
covering the warm-cache contract:

1. `audit_warm_path_first_call_is_cold_and_publishes_record` — first
   call yields `from_cache = false` (cold).
2. `audit_warm_path_second_call_short_circuits_with_from_cache_true`
   — second call (no dep change) yields `from_cache = true` and
   `total_ms = 0.0`. The synthesized record's `request_id` matches
   `resolution.request_id` (uniform `take_audit_record` contract).
3. `audit_warm_path_dep_change_invalidates_cache` — after `/types.ts`
   mutates, the second call's `dep_signature` is stale, the cache
   entry is dropped, and the cold resolver runs producing a
   `from_cache = false` record.

All 3 tests pass on the post-Step-4 tree.

## What is deferred

**Sub-task 4.4 — Memory Audit (`cached_resolution_template_memory_bounded`).**
The memory-bound test is a follow-up; the partition design (snapshot
NOT retained, content-addressed sidecars retained) follows D4.6.

**`ResolutionTemplate.snapshot` deep-copy optimization.** The `rehydrate`
clones `(*indexed.snapshot).clone()` because
`ResolvedComponentMetaState.snapshot` is owned `FileAnalysisSnapshot`,
not `Arc`. Profile-driven follow-up could change the field to
`Arc<FileAnalysisSnapshot>` to make warm-cache rehydrate near-zero
cost.

**Bench scenarios `single_warm` and `repo_warm_second_pass`.** The
plan calls for re-adding these to `packages/benchmark/src/scenarios.ts`
to validate the ~2 ms post-Step-4 SLA. Out of scope for this commit;
the test suite's three FAIL-FIRST tests provide the architectural
contract.

**Bench aggregation `include_warm_records: false` flag.** Out of
scope; the JS-side bench harness change is independent of the Rust
commit.

## Verified API surface

- `VerterHost::get_component_meta_with_resolution` — `host_manage.rs:5131`.
- `next_request_id()` — `host_manage.rs:5208`.
- `request_context::increment_requests_created()` — existing.
- `RequestContextGuard::install(...)` — existing TLS guard.
- `ProjectTypeStore::indexed()` — `project_type_store.rs:626`.
- `ProjectTypeStore::component_meta_results()` —
  `project_type_store.rs:655` (now generic over
  `CachedComponentMetaResult`).
- `ComponentMetaResultKey { owner_canonical, owner_whole_hash,
  query_kind, options_fingerprint }` — `component_meta_result_db.rs`.
- `ComponentMetaResultDb::get(key)` —
  `component_meta_result_db.rs::get`.
- `ComponentMetaResultEntry<P> { payload: Arc<P>, dep_signature:
  DepSignature }` — generic shape unchanged.
- `RequestAuditRecord.canonical_id: String` (verified — NOT `canonical`).
- `take_audit_record(request_id)` — `host_manage.rs:5217`.
- `publish_audit_record(record)` — `host_manage.rs:5226`.
