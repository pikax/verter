# Off-store host caches — rehoming roadmap

Source plan: `D:/tmp/verter-component-meta-performance-plan.md` Phase 12.followup.

This document captures the follow-up architecture work to rehome the
remaining cache-shape fields on `VerterHost` into `ProjectTypeStore`.
The work itself is **deferred** out of the current component-meta
performance landing because it is an independent architecture rewrite
that deserves its own plan + review cycle. This file is the standalone
rehoming plan that will be picked up later.

The five fields in scope:

| Field on `VerterHost`   | Backing shape                                               | Phase 6b classification     |
|-------------------------|-------------------------------------------------------------|-----------------------------|
| `compile_cache`         | `DashMap<String, CompileCacheEntry>`                        | `legitimate-authority` (F1) |
| `resolved_type_cache`   | `Mutex<FxHashMap<ResolvedTypeCacheKey, …Entry>>`            | `legitimate-authority` (F2) |
| `eval_env_cache`        | `Mutex<FxHashMap<String, (Hash16, Arc<EvalEnv>)>>`          | `legitimate-authority` (F4) |
| `semantic_db`           | `Mutex<verter_semantic::db::SemanticDb>`                    | `legitimate-authority` (F5) |
| `query_profile`         | `Mutex<verter_semantic::profile::QueryProfile>` (NOT cache) | `legitimate-authority` (F10) |

`query_profile` is included in the inventory because the
`no_off_store_host_caches` architecture guard surfaces it as a
cache-shape field, but it is execution-policy state, not a result
memoiser. It is in scope only for **classification cleanup**, not
rehoming. The four caches above are the actual rehoming targets.

The Phase 6b sub-plan kept these on `VerterHost` because each one had
either a sub-mirror lifecycle (F1), a different invalidation contract
(F2 — bounded clear-all rather than per-canonical), an artifact type
that did not compose into `ProjectTypeStore`'s typed-DB family (F4 / F5),
or no consumer pressure for project-global sharing (F4). The
classifications were correct given Phase 6b's scope: no consumer
benefit motivated the rehoming, and inventing one mid-phase would have
been speculative. This document captures the consumer pressure that
has emerged since (component-meta warm-cache parity, multi-host test
isolation, project-generation invalidation correctness) and proposes
a uniform target.

## Context

### Current state

The five fields above are direct fields on `pub struct VerterHost` in
`crates/verter_session/src/lib.rs:375..429`. Every accessor goes
through `self.<field>.lock()` (or `DashMap` API for `compile_cache`).
Eviction is bespoke per field:

- `compile_cache`: per-canonical `remove`, `clear` on
  `clear_compile_cache`, partial scrubs on
  `smart_invalidate_dependents`.
- `resolved_type_cache`: bounded **clear-all** at 4096 entries
  (`RESOLVED_TYPE_CACHE_CAP` — `host_resolve.rs:1551..1553`); also
  `clear()` on configure-projects and similar host-wide events.
- `eval_env_cache`: full `clear()` on every workspace-shape change —
  upserts that change content (`host_upsert.rs:242`),
  `configure_projects` (`lib.rs:1374`), `set_workspace`
  (`lib.rs:1424`), `clear_compile_cache` (`lib.rs:1460`),
  `notify_close` (`lib.rs:1914`).
- `semantic_db`: per-canonical `invalidate(id)` on smart invalidation
  (`host_upsert.rs:68`); full re-construct (`*db = SemanticDb::new()`)
  on `configure_projects` (`lib.rs:1022`) and `clear_compile_cache`
  (`lib.rs:1427`).

### Why this is debt

Five separate eviction surfaces, each with its own exception list, is
a structural maintenance hazard:

1. **No single cascade.** A new project-shape change has to land
   per-field invalidation in five places and be re-derived per-cache
   from first principles. A field added in the future might miss the
   list; bugs surface as "stale cache after tsconfig change" weeks
   later.
2. **No project-shape generation gate.** `ProjectTypeStore` already
   has `bump_project_generation_and_evict` which evicts every typed
   DB in one call (`project_type_store.rs:1236..1275`). The off-store
   host caches do not participate. Tests that rely on
   `bump_project_generation_and_evict` matching the host's actual
   readable state today have to know that four extra `Mutex::clear`
   calls happen in `lib.rs::configure_projects` to stay coherent.
3. **No counters / observability.** `ProjectTypeStoreCounters`
   provides per-DB `live` / `stale_sweeps` counters surfaced through
   `MetaProvenance`. The off-store caches have no equivalent
   instrumentation. `resolved_type_cache`'s clear-all events in
   particular are invisible to perf observers.
4. **Different completion contract.** Every rehomed DB uses
   `cooperative_get_or_insert` (admission control, panic safety,
   post-compute revalidation). The off-store caches use raw
   `Mutex::lock` for read-modify-write — concurrent cold callers
   for the same key recompute independently and the last-writer
   wins. `eval_env_cache::cache_eval_env_arc` is structurally a
   "lookup-or-insert" but it does NOT use the cooperative primitive,
   so two concurrent cold callers can both compute the same `EvalEnv`.
   `compile_cache`'s `DashMap::entry().or_insert_with` is even
   weaker: no post-compute revalidation, so a content edit racing
   with a compile slot insert can leave a stale slot pinned in the
   cache.
5. **Test isolation is leaky.** Multi-host tests have to manually
   `clear_compile_cache` and re-`new` the host to drop test-local
   state. With every cache rehomed, `ProjectTypeStore::new` produces
   a fresh, isolated cache root — `VerterHost` itself becomes
   nearly stateless aside from configuration handles.

### Phase 6b's classifications were correct in scope

Phase 6b classified F1, F2, F4, F5, F10 as `legitimate-authority`
because:

- F1 (`compile_cache`) is structured as the `CompileCacheEntry`
  super-shape — content overrides, compile slots, diagnostics, deps.
  Within that super-shape, `import_routes` is a sub-mirror of
  `IndexedReady.import_routes` with a *different invalidation
  trigger* (compile-event vs file-content-event — see the long
  comment at `types.rs:1215..1236`). Phase 6b did not have the time
  budget to split `CompileCacheEntry` so the safer call was to keep
  the whole struct off-store and document the sub-mirror.
- F2 (`resolved_type_cache`) is the **shared external-type cache**
  with **profile-gated writes**. The clear-all-at-4096 bound is
  intentional (host_resolve.rs:1551..1553) — not LRU, not bounded
  per-canonical. Reproducing the clear-all bound on a typed DB
  required a new artifact type that did not exist in 6b's scope.
- F4 (`eval_env_cache`) carries owned data (no allocator-lifetime
  constraints) and the consumer set is currently host-local. Phase
  6b's classification note at `lib.rs:402..405` explicitly says
  "Migration to a hypothetical `ProjectTypeStore.EvalEnvDb` is
  possible but unmotivated by current consumer patterns." That was
  honest then; consumer pressure has since changed (see Component-meta
  warm-cache rehydration in §1.1 below).
- F5 (`semantic_db`) is a **different crate's query-memo DB**.
  `verter_semantic::db::SemanticDb` is orthogonal to
  `ProjectTypeStore.semantic_graph()` — two crates, two artifact
  types. Phase 6b correctly refused to conflate them. Rehoming F5
  means hosting the `verter_semantic::db::SemanticDb` *handle*
  inside `ProjectTypeStore`, not folding it into `SemanticGraphStore`.
- F10 (`query_profile`) is execution-policy state and never was a
  cache. The structural-shape detection in
  `architecture_guards.rs::no_off_store_host_caches` flags it because
  `Mutex<QueryProfile>` matches the cache pattern; the allow-list
  documents the exception.

The classifications stay correct; what has changed is that the
Component-meta perf landing surfaced consumer evidence that motivates
the rehoming.

### What changed since Phase 6b

Three pressures that did not exist (or were undocumented) at 6b:

1. **Component-meta warm-cache rehydration (architectural-debt-closure
   step 4 / `04-step4-audit-warm-cache.md`).** The warm-cache short
   circuit in `try_with_resolution_cache_hit` now reads
   `ProjectTypeStore::indexed()` to rehydrate `FileAnalysisSnapshot`.
   The `EvalEnv` for the same canonical lives in
   `eval_env_cache` — a separate lock, separate eviction trigger.
   Two readers of the same logical "owner-version-pinned analysis
   state" go through two different cache surfaces. A single
   `ProjectTypeStore.EvalEnvDb` would let the warm path observe one
   coherent owner-version-pinned snapshot.
2. **`bump_project_generation_and_evict` exposure to
   `set_workspace` / `configure_projects`.** Today these hosts call
   `project_type_store.bump_project_generation_and_evict()` AND
   four `Mutex::clear` calls AND `*semantic_db = SemanticDb::new()`.
   That is two parallel invalidation cascades. Rehoming makes one
   cascade cover everything.
3. **Counters + provenance.** `ProjectTypeStoreCounters` already
   wires every typed DB into `MetaProvenance`. The off-store caches
   produce no provenance counters today; perf observers cannot tell
   when `resolved_type_cache` clears all 4096 entries vs when it
   serves warm hits. Rehoming attaches each DB to the counters
   harness uniformly.

## Changes

### Five new typed-DB destinations on `ProjectTypeStore`

Add five fields to `ProjectTypeStore` (the destination types are
`pub` because they are part of the same module's public surface).
Existing rehomed-DB constructors (`with_counter` / `with_counters`)
are the template:

```rust
// crates/verter_session/src/project_type_store.rs

pub struct ProjectTypeStore {
    // ... existing fields elided ...
    /// Phase 12.followup F1 — per-canonical compile state. Replaces
    /// `VerterHost::compile_cache`. Per-canonical eviction goes
    /// through `Self::evict_canonical`. Project-generation eviction
    /// is handled by `bump_project_generation_and_evict`.
    compile_cache: CompileCacheDb,
    /// Phase 12.followup F2 — shared external-type cache with
    /// profile-gated writes. Replaces `VerterHost::resolved_type_cache`.
    /// Bounded clear-all at `RESOLVED_TYPE_CACHE_CAP` is preserved
    /// inside the DB (NOT LRU); per-canonical eviction added.
    resolved_type_cache: ResolvedTypeCacheDb,
    /// Phase 12.followup F4 — `EvalEnv` snapshots keyed by
    /// `(canonical, whole_hash)`. Replaces
    /// `VerterHost::eval_env_cache`.
    eval_env_cache: EvalEnvCacheDb,
    /// Phase 12.followup F5 — handle to the `verter_semantic` query
    /// memo DB. Replaces `VerterHost::semantic_db`. Different crate,
    /// different artifact type than `Self::semantic_graph` — this
    /// is the **handle** sitting inside `ProjectTypeStore`, not a
    /// fold-in.
    semantic_db: parking_lot::Mutex<verter_semantic::db::SemanticDb>,
}
```

`query_profile` (F10) is **not** rehomed. It is execution-policy
state, not a result memoiser. The `no_off_store_host_caches`
allow-list keeps its entry; the rationale stays the same.

### F1 — `compile_cache` rehoming

**Backing**: `pub(crate) struct CompileCacheDb` wrapping
`DashMap<String, Arc<CompileCacheEntry>>` (note: `Arc<Entry>` to
match the cooperative-admission contract; today the off-store form
is `DashMap<String, CompileCacheEntry>`, and the change to `Arc`
is part of the rehoming).

**Sub-mirror lifecycle preservation**: the doc-comment block at
`types.rs:1215..1236` is the binding contract that the rehoming
must preserve. `CompileCacheEntry::import_routes` is a sub-mirror
of `IndexedReady.import_routes` with a *different invalidation
trigger*: compile-event invalidation drops it, file-content-event
invalidation drops both. The rehomed DB MUST keep this asymmetry
visible — option (a) keeps `CompileCacheEntry` as one struct and
documents the sub-shape lifecycle on the DB; option (b) splits
`CompileCacheEntry` into `ProfileState` + `DerivedRawState` +
`DependencyState` so each sub-shape is its own typed entry. Option
(b) is the correct end-state but is a separate scope; option (a)
is the rehoming. **Selected: option (a).** The split is filed as
a downstream follow-up to this rehoming, not part of it.

**API contract**: existing readers / writers of the off-store
`compile_cache` translate 1:1:

- `host.compile_cache.entry(canonical).or_insert_with(...)` →
  `project_type_store.compile_cache().get_or_insert(...)`
- `host.compile_cache.get(canonical)` →
  `project_type_store.compile_cache().get(canonical)`
- `host.compile_cache.remove(canonical)` →
  participates in `evict_canonical` cascade

**Cooperative admission**: cold compile-slot insertions go through
`cooperative_get_or_insert` (post-compute revalidation: if
`whole_hash` of the canonical changed during compute, the entry is
dropped). This closes the race documented in §0.4 above where two
concurrent cold callers could pin a stale slot.

**Project-generation invalidation**: `compile_cache` participates in
`bump_project_generation_and_evict`. Today `clear_compile_cache`
(`lib.rs:1448..1467`) does this with full `clear()`; the rehoming
makes it a one-line entry in the project-store cascade.

### F2 — `resolved_type_cache` rehoming

**Backing**: `pub(crate) struct ResolvedTypeCacheDb` wrapping the
existing `FxHashMap<ResolvedTypeCacheKey, ResolvedTypeCacheEntry>`
behind a `Mutex` plus a per-canonical reverse index.

**Bounded clear-all preservation**: the 4096-entry cap is part of
the contract (proven by `meta_tests.rs:9764..9799`; range
[1024, 16384] is asserted). The rehomed DB keeps the cap **inside**
the DB. Entry insertion follows the same rule:

```rust
if self.entries.lock().len() >= RESOLVED_TYPE_CACHE_CAP {
    self.entries.lock().clear();
    self.bounded_clear_count.fetch_add(1, Relaxed);  // NEW: counter
}
```

The `bounded_clear_count` counter is new — it surfaces clear-all
events through `ProjectTypeStoreCounters`. This is the visibility
gap from §0.4 closed.

**Per-canonical eviction (NEW)**: today `resolved_type_cache` only
evicts via clear-all. The rehomed DB adds per-canonical eviction
via reverse index (every entry's `dep_canonical_id` is recorded in
a side `FxHashMap<String, FxHashSet<ResolvedTypeCacheKey>>`). This
lets the DB participate in `evict_canonical` instead of waiting
for the next clear-all to flush stale entries.

**Profile-gated writes preserved**: the write path
(`store_resolved_external_type_cache`) gates on `profile_hash:
None`. The rehomed DB exposes the same gate via its writer signature:
profile-tainted callers cannot reach the DB writer. Today this is a
runtime convention; post-rehoming it is a compile-time API rule.

### F4 — `eval_env_cache` rehoming

**Backing**: `pub(crate) struct EvalEnvCacheDb` wrapping
`FxHashMap<String, (Hash16, Arc<EvalEnv>)>` behind a `Mutex`.

**Cache-key shape**: Phase 6b's classification preserves the
existing `(canonical_id, whole_hash)` key composition. The rehomed
DB exposes the same lookup signature
`get_or_compute(canonical_id, whole_hash, compute)`.

**Cooperative admission**: today `cache_eval_env_arc`
(`host_manage/eval_env.rs:28..47`) is a manual lookup-or-insert.
Two concurrent cold callers for the same `(canonical, whole_hash)`
both compute. The rehomed DB uses `cooperative_get_or_insert` so
the second caller waits.

**Per-canonical eviction**: today `eval_env_cache.lock().clear()`
fires on every workspace-shape change. The rehomed DB uses
`evict_canonical` on per-file invalidation and project-generation
eviction on workspace-shape changes — same total scope, structured
as one cascade.

**Why rehome F4 now (vs Phase 6b)**: §0.7 above. The warm-cache
rehydration path
(`try_with_resolution_cache_hit::ResolutionTemplate`) reads
`ProjectTypeStore::indexed()` for `FileAnalysisSnapshot` and the
off-store `eval_env_cache` for the matching `EvalEnv`. Two readers,
two cache surfaces, no joint-coherence guarantee. Rehoming gives
the warm path one snapshot.

### F5 — `semantic_db` rehoming

**Backing**: `parking_lot::Mutex<verter_semantic::db::SemanticDb>`
moves verbatim from `VerterHost` into `ProjectTypeStore`. No DB
wrapper — the `SemanticDb` is already a complete query-memo DB
inside `verter_semantic`.

**Why this is rehoming and not folding-in**: Phase 6b's note at
`lib.rs:411..422` is binding. `verter_semantic::db::SemanticDb` is
a separate crate's artifact (component surfaces, binding facts,
reactivity provenance). `ProjectTypeStore.semantic_graph()` is the
*resolved-named-type graph arena* — different domain, different
crate. The rehoming places the `SemanticDb` *handle* under
`ProjectTypeStore`'s ownership tree without fusing its API surface.

**Per-canonical invalidation preserved**: today
`smart_invalidate_dependents` calls
`host.semantic_db.lock().invalidate(canonical_id)`. The rehomed
form: `project_type_store.semantic_db().invalidate(canonical_id)`.
1:1 translation. The rehomed `evict_canonical` cascade adds a call
to `self.semantic_db.lock().invalidate(canonical_id)` so per-file
content edits drop the entry through the unified path.

**Project-generation reset preserved**: today
`*self.semantic_db.lock() = SemanticDb::new()` fires on
`configure_projects` and `clear_compile_cache`. The rehomed
`bump_project_generation_and_evict` does the same.

**Test policy**: `verter_semantic::db::SemanticDb` is exercised by
its own test suite. The rehoming does not move tests; it moves the
*handle*.

### Accessor surface

```rust
impl ProjectTypeStore {
    pub fn compile_cache(&self) -> &CompileCacheDb { &self.compile_cache }
    pub(crate) fn resolved_type_cache(&self) -> &ResolvedTypeCacheDb { &self.resolved_type_cache }
    pub(crate) fn eval_env_cache(&self) -> &EvalEnvCacheDb { &self.eval_env_cache }
    pub(crate) fn semantic_db(&self) -> parking_lot::MutexGuard<'_, verter_semantic::db::SemanticDb> {
        self.semantic_db.lock()
    }
}
```

`compile_cache()` is `pub` because `verter_napi` and `verter_lsp`
already touch the off-store `compile_cache` field (it is `pub(crate)`
today, surfaced via `VerterHost` methods that the FFI layer wraps).
The other three are `pub(crate)` — only `verter_session` consumes
them, matching the current accessor scope.

### Eviction cascade extensions

```rust
impl ProjectTypeStore {
    pub fn evict_canonical(&self, canonical_id: &str) {
        // ... existing cascade ...
        // F1, F2, F4, F5 join the per-canonical cascade.
        self.compile_cache.invalidate_canonical(canonical_id);
        self.resolved_type_cache.invalidate_canonical(canonical_id);
        self.eval_env_cache.invalidate_canonical(canonical_id);
        self.semantic_db.lock().invalidate(canonical_id);
    }

    pub fn bump_project_generation_and_evict(&self) -> u64 {
        let generation = self.bump_project_generation();
        // ... existing cascade ...
        // F1, F2, F4, F5 join the project-generation cascade.
        self.compile_cache.invalidate_all();
        self.resolved_type_cache.invalidate_all();
        self.eval_env_cache.invalidate_all();
        *self.semantic_db.lock() = verter_semantic::db::SemanticDb::new();
        generation
    }
}
```

### Counters

`ProjectTypeStoreCounters` adds five fields:

```rust
compile_cache_live: Arc<AtomicU64>,
compile_cache_stale_sweeps: Arc<AtomicU64>,
resolved_type_cache_live: Arc<AtomicU64>,
resolved_type_bounded_clear_count: Arc<AtomicU64>,  // NEW visibility
eval_env_cache_live: Arc<AtomicU64>,
eval_env_cache_stale_sweeps: Arc<AtomicU64>,
// F5 inherits counters from `verter_semantic::db::SemanticDb`'s
// existing `stats_snapshot` surface — no new counter here.
```

The counters wire into `MetaProvenance` the same way the existing
ones do (`provenance.rs::record_*`).

### Files to modify

The rehoming touches the four cache-owning files plus their
consumers. Concrete files (in dependency order):

1. **`crates/verter_session/src/project_type_store.rs`** — add
   `CompileCacheDb`, `ResolvedTypeCacheDb`, `EvalEnvCacheDb` types;
   add the four new fields on `ProjectTypeStore`; extend
   `evict_canonical` and `bump_project_generation_and_evict`; extend
   `ProjectTypeStoreCounters`.
2. **`crates/verter_session/src/lib.rs`** — delete the four fields
   from `VerterHost` (`compile_cache`, `resolved_type_cache`,
   `eval_env_cache`, `semantic_db`); delete the four `*.lock().clear()`
   call sites in `configure_projects`, `set_workspace`,
   `clear_compile_cache`, `notify_close`; replace every accessor
   with `self.project_type_store.compile_cache()` / `.resolved_type_cache()` /
   `.eval_env_cache()` / `.semantic_db()`.
3. **`crates/verter_session/src/host_resolve.rs`** — rewrite
   `lookup_resolved_external_type_cache` and
   `store_resolved_external_type_cache` to delegate to the rehomed
   DB. The bounded clear-all bound moves inside the DB.
4. **`crates/verter_session/src/host_manage/eval_env.rs`** — rewrite
   `cache_eval_env_arc` and `clone_cached_eval_env_arc` to delegate
   to the rehomed `EvalEnvCacheDb`.
5. **`crates/verter_session/src/host_upsert.rs`** — replace the four
   off-store invalidation calls with their rehomed equivalents (or
   delete them if the unified `evict_canonical` cascade subsumes
   them). The `semantic_db.lock().invalidate(id)` site at
   `host_upsert.rs:68` is a candidate for deletion if
   `evict_canonical` is the new path.
6. **`crates/verter_session/src/host_compile.rs`** — every
   `compile_cache.entry().or_insert_with` site translates to
   `project_type_store.compile_cache().get_or_insert`. The
   `cooperative_get_or_insert` post-compute revalidation closes the
   stale-slot race documented in §0.4.
7. **`crates/verter_session/src/deps.rs`** — `DependentView` and
   `should_invalidate_dependent_view` reference
   `&dashmap::DashMap<String, CompileCacheEntry>` directly today.
   Retype to `&CompileCacheDb` (the wrapper exposes the same
   iteration shape).
8. **`crates/verter_session/tests/architecture_guards.rs`** —
   rewrite `phase_8_allow_list()` to remove F1, F2, F4, F5; keep
   F10 (`query_profile`), F12 (`alias_to_canonical`), F13
   (`last_const_prop_overrides`), and the `workspace` /
   `last_upsert_priority` exceptions. The guard's body is unchanged;
   only the allow-list shrinks.

### Per-host singleton constraint

`ProjectTypeStore::new()` produces one independent cache root.
Multi-host tests today rely on `VerterHost::new` producing a fresh
cache-bag. Post-rehoming, every cache lives inside `ProjectTypeStore`,
so `VerterHost::new` instantiates a fresh `ProjectTypeStore` and gets
a fresh cache root automatically. No test changes required for
isolation; `VerterHost::new` continues to allocate a fresh
`ProjectTypeStore` per host.

### Backward-compat shims policy

**No shims, no dual paths, no deprecated accessors.** The rehoming
is a clean cutover per CLAUDE.md "Legacy Code Deletion". The four
fields on `VerterHost` are deleted; every accessor is rewritten to
go through `self.project_type_store`; the `no_off_store_host_caches`
guard's allow-list is shrunk in the same commit so the guard fires
if a future commit re-adds an off-store cache.

## Legacy Deletions

### Fields deleted from `VerterHost` (`lib.rs`)

- `compile_cache: dashmap::DashMap<String, CompileCacheEntry>` — line 375.
- `resolved_type_cache: parking_lot::Mutex<FxHashMap<ResolvedTypeCacheKey, ResolvedTypeCacheEntry>>` — line 393.
- `eval_env_cache: parking_lot::Mutex<FxHashMap<String, (Hash16, Arc<EvalEnv>)>>` — line 408.
- `semantic_db: parking_lot::Mutex<verter_semantic::db::SemanticDb>` — line 422.

### Methods / call sites deleted

- `VerterHost::compile_cache()` (if any direct accessor exists today
  — the field is `pub(crate)` and most consumers reach it directly
  through `self.compile_cache`).
- `VerterHost::lookup_resolved_external_type_cache` /
  `store_resolved_external_type_cache`
  (`host_resolve.rs:1516..1561`) are rewritten to delegate to the
  rehomed DB; the helpers stay but their bodies shrink to one line.
- All `self.eval_env_cache.lock().clear()` sites:
  `host_upsert.rs:242`, `lib.rs:1055`, `lib.rs:1374`, `lib.rs:1424`,
  `lib.rs:1460`, `lib.rs:1914`. Replaced by participation in
  `bump_project_generation_and_evict`.
- All `self.semantic_db.lock()` sites in
  `lib.rs:801, 814, 841, 854, 881, 890, 1014, 1022, 1427, 1702`
  are rewritten to go through `self.project_type_store.semantic_db()`.
  The full reset `*db = SemanticDb::new()` happens inside
  `bump_project_generation_and_evict`.
- `host_upsert.rs:68` `self.semantic_db.lock().invalidate(id)` —
  evaluated for deletion (subsumed by `evict_canonical` cascade).
  Delete if and only if `evict_canonical` runs at every site that
  the off-store invalidate fires today; otherwise keep with rehomed
  accessor.

### Allow-list entries removed from `phase_8_allow_list()`

(`crates/verter_session/tests/architecture_guards.rs:1934..1992`)

- `compile_cache` (F1)
- `resolved_type_cache` (F2)
- `eval_env_cache` (F4)
- `semantic_db` (F5)

The allow-list keeps:

- `query_profile` (F10) — execution-policy state, not a cache.
- `alias_to_canonical` (F12), `last_const_prop_overrides` (F13) —
  not caches; documented exceptions.
- `workspace` — single-cell config handle.
- `last_upsert_priority` — `#[cfg(test)]` test mailbox.

### Sub-mirror documentation

The doc-comment block at `types.rs:1215..1236` describing
`CompileCacheEntry::import_routes` as a sub-mirror of
`IndexedReady.import_routes` is **rewritten**, not deleted. The
rewrite says: "post-rehoming, both lifecycles live inside
`ProjectTypeStore`, but the field is still a sub-mirror with a
different invalidation trigger; see `CompileCacheDb`'s lifecycle
docs for the full asymmetry."

### Deferred follow-ups (NOT part of this rehoming)

- **`CompileCacheEntry` super-shape split** (option (b) in §1.1).
  Splitting `CompileCacheEntry` into `ProfileState` +
  `DerivedRawState` + `DependencyState` is a separate scope; this
  rehoming keeps the super-shape intact and rehomes it as one DB.
- **Move `verter_semantic::db::SemanticDb` into `verter_session`**.
  The crate split is correct (`SemanticDb` serves the surfaces /
  bindings / reactivity provenance pipeline). This rehoming places
  the `Mutex<SemanticDb>` *handle* under `ProjectTypeStore`'s
  ownership; it does not fold the artifact into another crate.
- **Promote `eval_env_cache` to project-global sharing** (cross-host
  reuse of `EvalEnv` for the same `(canonical, whole_hash)`).
  Phase 6b's note at `lib.rs:402..405` says the migration is
  "unmotivated by current consumer patterns." This rehoming closes
  the host-local-vs-project-store split but does not enable
  cross-host sharing — `ProjectTypeStore` is still per-host today.
  Cross-host sharing is its own scope.
- **Profile-aware key composition for `eval_env_cache`**. Today the
  cache key is `(canonical, whole_hash)`. Profile-gated writes (the
  F2 model) could apply but are not motivated by current consumer
  patterns. Filed as a downstream follow-up.

## Verification

### Workspace-wide test gate

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --tests -- -D warnings
cargo test --workspace --tests --verbose 2>&1 | Tee-Object -FilePath tmp/test-output.txt
```

All workspace tests must pass. Pre-rehoming baseline at this
worktree's HEAD is **10425 tests passing** (per the bundle's base
commit `2a2d93dc`). Post-rehoming target: **same count**, no
regressions. The rehoming is a structural move; net behaviour is
unchanged.

### Architecture-guard gate

```powershell
cargo test --workspace --tests --verbose -- architecture_guards
```

Specifically:

- `no_off_store_host_caches` must pass with the shrunk allow-list.
  After deleting F1/F2/F4/F5 entries, the guard's body asserts that
  the only cache-shape fields on `VerterHost` are the documented
  exceptions (`query_profile`, `alias_to_canonical`,
  `last_const_prop_overrides`, `workspace`, `last_upsert_priority`).
- `no_off_store_host_caches_self_test` (the discriminator self-test)
  must pass.

### Discriminating tests added (must FAIL on pre-rehoming tree, PASS
on post-rehoming tree)

Per CLAUDE.md "Stub Prevention" — every characterization test must
discriminate. The rehoming PR lands these new tests:

1. **`compile_cache_lives_on_project_type_store`** — uses
   `syn::parse_file` to parse `lib.rs` and assert that
   `VerterHost` has no field named `compile_cache`. Pre-rehoming:
   FAILS (field exists). Post-rehoming: PASSES.
2. **`resolved_type_cache_evict_canonical_drains_dep_canonical`** —
   inserts a resolved-type entry for `dep_canonical: "X"`; calls
   `project_type_store.evict_canonical("X")`; asserts the entry is
   gone. Pre-rehoming: FAILS (per-canonical eviction does not exist;
   entry survives until clear-all). Post-rehoming: PASSES.
3. **`eval_env_cache_two_concurrent_cold_callers_compute_once`** —
   spawns two threads computing `EvalEnv` for the same
   `(canonical, whole_hash)`; asserts `compute()` is called once
   (via a `CaptureToken` counter). Pre-rehoming: FAILS (manual
   lookup-or-insert allows two computes). Post-rehoming: PASSES.
4. **`semantic_db_evict_canonical_invalidates_via_unified_cascade`** —
   inserts a semantic-db entry for canonical "X"; calls
   `project_type_store.evict_canonical("X")`; asserts
   `verter_semantic::db::SemanticDb::is_invalidated("X")` returns
   true. Pre-rehoming: FAILS (only `smart_invalidate_dependents`
   touches `semantic_db`; `evict_canonical` does not). Post-rehoming:
   PASSES.
5. **`bump_project_generation_evicts_all_four`** — populates one
   entry in each of the four rehomed caches; calls
   `bump_project_generation_and_evict`; asserts all four are empty.
   Pre-rehoming: FAILS (the off-store caches have separate clear
   paths). Post-rehoming: PASSES.

Each test is scoped to verify ONE of the rehoming invariants. None
of them passes against the pre-rehoming tree; that is the
discriminating-test contract.

### Hermetic-test gate (per Tier C verification protocol)

```powershell
$hermeticWt = "D:/dev/personal/verter-wt/hermetic-verify-cache-rehoming"
git worktree add $hermeticWt HEAD
Push-Location $hermeticWt
if (Test-Path .integration-tests/repos/nuxt-ui) {
    Remove-Item -Recurse -Force .integration-tests/repos/nuxt-ui
}
cargo test --workspace --tests --verbose 2>&1 | Tee-Object -FilePath ../../verter/tmp/test-output-hermetic.txt
```

Per CLAUDE.md "Testing-Hermeticity (MANDATORY)": tests run without
external corpora. The rehoming does not add new corpus dependencies.
Test count parity with the regular gate is the success criterion.

### Performance gate

The rehoming is **structural**, not behavioural. The expected
delta is:

- **Cooperative admission saves redundant compute.** F1
  (`compile_cache`) and F4 (`eval_env_cache`) close the
  two-concurrent-cold-callers race documented in §0.4. Net: small
  reduction in cold-path compute under contention; no change under
  warm hits.
- **Per-canonical `evict_canonical` cascade unifies invalidation.**
  Today the host clears `eval_env_cache` on every `notify_close` /
  `set_workspace` / `configure_projects`. Post-rehoming, the same
  set of events route through `bump_project_generation_and_evict`
  (which already runs on those paths). Net: zero change in
  invalidation scope; one fewer lock acquired per event.
- **Per-canonical eviction (F2 NEW)**:
  `resolved_type_cache.evict_canonical("X")` is finer-grained than
  the existing clear-all-at-4096. Pre-rehoming, a single canonical's
  edit invalidated 0 entries (until the next 4096 fill). Post-rehoming,
  the same edit invalidates exactly the entries whose `dep_canonical_id
  == "X"`. Net: more cache hits on the warm path after small edits.

The repo-first-pass benchmark (`tests/repo_first_pass_diagnosis_corpus.rs`)
must show no regression and a small improvement on the F2 path. The
benchmark's own tolerance bands cover the expected range.

### Conventional-commit shape

The rehoming lands as one commit:

```
refactor(meta): rehome off-store host caches into ProjectTypeStore (Phase 12.followup)

- F1 compile_cache: DashMap<String, CompileCacheEntry>
  → ProjectTypeStore.compile_cache (CompileCacheDb)
- F2 resolved_type_cache: bounded clear-all preserved inside DB,
  per-canonical eviction added
  → ProjectTypeStore.resolved_type_cache (ResolvedTypeCacheDb)
- F4 eval_env_cache: cooperative admission closes two-concurrent-
  cold-callers race
  → ProjectTypeStore.eval_env_cache (EvalEnvCacheDb)
- F5 semantic_db: handle moved (different crate, different artifact;
  not folded into SemanticGraphStore)
  → ProjectTypeStore.semantic_db (Mutex<SemanticDb>)
- no_off_store_host_caches allow-list shrunk to F10 + non-cache
  exceptions only

5 new discriminating tests; 0 expected regressions.
```

### Rollback contract

The rehoming is a clean cutover. Rollback is `git revert` of the
landing commit, which restores the four fields on `VerterHost` and
the four allow-list entries simultaneously. There is no partial
rollback — F1/F2/F4/F5 are interlocked through the unified
`evict_canonical` cascade.

## Why this is filed under Phase 12.followup

Phase 12.followup of the component-meta performance plan
(`D:/tmp/verter-component-meta-performance-plan.md` lines 1201..1207)
explicitly stages this work as a follow-up because:

1. The component-meta performance landing's scope is component-meta
   correctness + performance (selective `Pick` materialisation,
   symbolic `Omit` preservation, repo-first-pass invariants). The
   rehoming is orthogonal to those phases — it does not change any
   component-meta semantics or performance characteristics by itself.
2. Each rehomed cache surfaces consumer-visible API changes through
   the FFI / LSP / playground boundary. Bundling those changes with
   the component-meta landing would conflate two reviewable scopes.
3. The rehoming requires its own discriminating test suite (§3.3
   above) and its own architecture-guard allow-list shrink. Neither
   is in the component-meta landing's scope.

This document satisfies the Phase 12.followup acceptance criterion:
*"Follow-up doc exists with concrete sections (Context, Changes,
Legacy Deletions, Verification per CLAUDE.md plan structure). Not a
placeholder."*

The rehoming itself will be a separate plan / PR cycle picked up
when the component-meta landing has stabilised. This document is
the entry point for that cycle.
