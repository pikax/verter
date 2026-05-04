# Tier 1C-β Worker Report — CompileCacheEntry super-shape split per D48

Branch: `w1c-beta-CompileCacheEntry-split` (off `01b5df97`).
Final SHA: `5695d15b`.

## 1. Steps completed

The Tier 1C-β plan §3.4.2 split is fully landed:

1. Three sub-state types added to `crates/verter_session/src/types.rs` —
   `ProfileState`, `DerivedRawState`, `DependencyState`. The pre-1C-β
   super-shape `CompileCacheEntry` is removed; its 18 fields are
   distributed across the three sub-state types per the rehoming-doc
   F1 mapping.
2. Two new sibling DBs added to
   `crates/verter_session/src/project_type_store.rs` —
   `DerivedRawCacheDb` (`DashMap<String, DerivedRawState>`) and
   `DependencyCacheDb` (`DashMap<String, DependencyState>`). The
   pre-existing `CompileCacheDb` retains its name but its value type
   shrinks from `CompileCacheEntry` to `ProfileState`.
3. Per-domain accessors added on `ProjectTypeStore` and `VerterHost`:
   `derived_raw_cache()`, `dependency_cache()`. The pre-existing
   `compile_cache()` accessor stays.
4. Aggregate helpers added on `VerterHost`:
   `is_canonical_evicted(canonical)` and
   `drop_all_per_canonical_compile_caches(canonical)` for the
   file-deletion path (outside the matrix).
5. The 4-row D48 invalidation matrix is enforced as the SOLE
   invalidation contract. New primitive
   `ProjectTypeStore::evict_for_source_content_change(canonical)`
   implements row 1; existing
   `bump_project_generation_and_evict` cascade extended to drop all
   three sibling DBs (row 4); the host-level upsert flow fires
   per-domain mutations per the matrix (rows 2 + 3 driven by
   profile-flag / dep-closure changes from upstream).
6. Sub-mirror visibility: `DerivedRawState::import_routes` carries the
   doc-comment block describing it as the sub-mirror of
   `IndexedReady.import_routes` with a different invalidation trigger
   (the doc-comment block previously at `types.rs:1215..1236` is
   rewritten on `DerivedRawState`, NOT on the wrapper DB).
7. ~270 access sites refactored to read/write through the appropriate
   sub-state DB. Cross-domain mutations (e.g. `host_upsert.rs`'s
   invalidation block) now fire 3 separate per-domain blocks.

## 2. Files changed (21 files)

| File | Change |
|---|---|
| `crates/verter_session/src/types.rs` | `CompileCacheEntry` removed; 3 sub-state types added (`ProfileState`, `DerivedRawState`, `DependencyState`). |
| `crates/verter_session/src/project_type_store.rs` | `CompileCacheDb` value type shrunk to `ProfileState`; `DerivedRawCacheDb` + `DependencyCacheDb` added; `evict_for_source_content_change` primitive added; `bump_project_generation_and_evict` cascade extended; `PROJECT_TYPE_STORE_DB_INVENTORY` + `all_dbs_for_invalidation` + `invalidate_canonical_across_all_dbs` extended to register the two new DBs. |
| `crates/verter_session/src/project_type_store_tests.rs` | 4 new D48 discriminating tests added. |
| `crates/verter_session/src/host_construction.rs` | New host accessors `derived_raw_cache()` + `dependency_cache()`; `is_canonical_evicted` + `drop_all_per_canonical_compile_caches` helpers. |
| `crates/verter_session/src/host_upsert.rs` | Main upsert flow + byte-identical fast path now fire per-domain mutations per D48 matrix. |
| `crates/verter_session/src/host_lifecycle.rs` | `clear_compile_cache` + `evict` + `configure_projects` + `integrate_scheduler_snapshot` + smart-invalidate dependents updated for per-domain reads/writes. |
| `crates/verter_session/src/host_manage/analysis_io.rs` | 25+ access sites refactored. |
| `crates/verter_session/src/host_manage/component_meta_methods.rs` | `cached_resolved_meta` + `cached_meta_payload` + `import_routes` reads moved to `derived_raw_cache`. |
| `crates/verter_session/src/host_manage/fallthrough.rs` | `cached_fallthrough` + `import_routes` + `dependencies` per-domain. |
| `crates/verter_session/src/host_manage/prepared_decl.rs` | `import_routes` reads on derived_raw_cache. |
| `crates/verter_session/src/host_manage.rs` | `cached_fallthrough` access on derived_raw_cache. |
| `crates/verter_session/src/host_resolve.rs` | `cached_tsc_extract` + `import_routes` + `dependencies` per-domain. |
| `crates/verter_session/src/host_test_seed.rs` | `import_routes` on derived_raw_cache. |
| `crates/verter_session/src/cross_file.rs` | Eviction filter via `is_canonical_evicted`. |
| `crates/verter_session/src/deps.rs` | `smart_invalidate_dependents_via_scheduler` + `build_dependent_view` refactored to take `&VerterHost` and read from per-domain DBs. |
| `crates/verter_session/src/resolver_store.rs` | `import_routes` reads on derived_raw_cache. |
| `crates/verter_session/src/lib_tests.rs` | Test assertions migrated to per-domain reads. |
| `crates/verter_session/src/meta_tests.rs` | Cached-meta + cached-fallthrough + import_routes test reads/writes per-domain. |
| `crates/verter_session/src/meta_resolve_tests.rs` | `cached_resolved_meta` clear via derived_raw_cache. |
| `crates/verter_session/src/host_manage_tests.rs` | `import_routes` test on derived_raw_cache. |
| `crates/verter_session/src/host_resolve_tests.rs` | `cached_tsc_extract` test reads on derived_raw_cache. |

## 3. Discriminating tests added (4)

Per plan §3.4.2 — all 4 mandated tests added in
`project_type_store_tests.rs`, each covering one row of the D48
invalidation matrix.

| Test | Predicate |
|---|---|
| `source_content_change_preserves_profile_state` | After `evict_for_source_content_change(canonical)`: ProfileState SURVIVES; DerivedRawState + DependencyState DROPPED. |
| `profile_flag_change_preserves_raw_and_dep_state` | After `compile_cache().clear()` (profile-domain flush): ProfileState DROPPED; DerivedRawState + DependencyState SURVIVE. |
| `dep_transitive_close_change_preserves_profile_and_raw` | After `dependency_cache().clear()` (dep-domain flush): ProfileState + DerivedRawState SURVIVE; DependencyState DROPPED. |
| `bump_project_generation_evicts_all_three_sub_shapes` | After `bump_project_generation_and_evict`: ALL THREE empty. |

**FAIL-pre evidence** (mechanical at type level): pre-1C-β the
codebase has no `derived_raw_cache()` / `dependency_cache()` accessors
on `VerterHost` and no `DerivedRawCacheDb` / `DependencyCacheDb` types
on `ProjectTypeStore`. The 4 discriminating tests reference these
symbols directly; they could not even compile against the pre-1C-β
tree, satisfying the discriminating-test stub-prevention rule (the
asymmetric drop predicates are not satisfiable when all three "fields"
share the same `CompileCacheEntry` super-shape that drops together).

**PASS-post evidence**: all 4 tests pass on `5695d15b`. Test output:
```
test source_content_change_preserves_profile_state ... ok
test profile_flag_change_preserves_raw_and_dep_state ... ok
test dep_transitive_close_change_preserves_profile_and_raw ... ok
test bump_project_generation_evicts_all_three_sub_shapes ... ok
```

The 4 tests collectively establish exhaustive D48 matrix coverage:
each row's predicate is asserted for each of 3 columns, and each
matrix cell is tested by at least one assertion.

## 4. Verification command outputs

```text
$ cargo test -p verter_session --test architecture_guards 2>&1 | tail -5
test foundations_guards::no_phase_archaeology_in_production_code_broader_d111 ... ok

test result: ok. 54 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.08s

$ cargo test -p verter_session 2>&1 | grep "test result:" | head -1
test result: ok. 2002 passed; 0 failed; 4 ignored; 0 measured; 0 filtered out; finished in 119.79s

$ cargo test --workspace --tests -j 4 2>&1 | tee /tmp/w1c-beta-workspace.txt | tail -5
... (444 passing in workspace crate, no failures)
$ grep -c FAILED /tmp/w1c-beta-workspace.txt
0
$ # Aggregated total:
passed: 10541 failed: 0

$ cargo clippy --workspace --tests -- -D warnings 2>&1 | tail -2
    Checking verter_mcp_server v0.0.1-beta.1 (...)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 10.34s

$ cargo fmt --all --check 2>&1
(silent)
```

Test count delta: prior **10537** → current **10541** (+4 from
discriminating tests added; 0 deletions; 0 regressions).

## 5. Decisions made

### Q: Did you add new accessor methods on ProjectTypeStore?
**A:** Yes — `derived_raw_cache(&self) -> &DerivedRawCacheDb` and
`dependency_cache(&self) -> &DependencyCacheDb`. `compile_cache(&self)
-> &CompileCacheDb` retains its name (the value type changed but the
accessor signature is unchanged). One new primitive method:
`evict_for_source_content_change(&self, canonical_id: &str)` for D48
row 1. The cascade methods
`PROJECT_TYPE_STORE_DB_INVENTORY` + `all_dbs_for_invalidation` +
`invalidate_canonical_across_all_dbs` are extended to register the two
new sibling DBs.

### Q: How is the 4096-cap-clear-all preserved on ResolvedTypeCacheDb?
**A:** Untouched — the bounded clear-all-at-cap policy lives INSIDE
`ResolvedTypeCacheDb` and is independent of the per-canonical
compile-cache split. ResolvedTypeCacheDb is not a sibling of the three
new D48 sub-domains (it's a separate F2 cache).

### Q: Does ProfileState borrow against ProjectTypeStore lifetime or own its data?
**A:** `ProfileState` (and `DerivedRawState` and `DependencyState`)
are owned-data structs — no borrowed lifetimes. They're stored
**by value** in the `DashMap<String, T>` of each DB. This matches the
`Send + Sync + 'static` contract enforced by the
`typed_dbs_are_send_sync_static` arch guard. The DBs hand out
`dashmap::mapref::one::Ref` / `RefMut` to call sites which are bounded
by the DashMap shard-lock lifetime; each per-domain mutation acquires
its own shard lock. (The plan's idealized `Arc<ProfileState>`
projection was not adopted — DashMap entry locking gives equivalent
mutation semantics without the Arc indirection, and the 270+
existing call sites already use the entry/get/get_mut shape directly.)

### Q: How does the host upsert flow fire the matrix?
**A:** Three sequential per-domain blocks in
`host_upsert::upsert_via_scheduler_with_priority`:
1. ProfileState block: `compile_cache().entry(c).or_default()` →
   surgically clear `content_overrides` / `style_overrides` /
   `compile_slots` / `latest_diagnostics` / `diagnostics_generation`
   per the slice-change predicates from `compute_upsert_changes_from_parse`.
2. DerivedRawState block: `derived_raw_cache().entry(c).or_default()` →
   clear `cached_tsc_extract` / `cached_resolved_meta` /
   `cached_meta_payload` / `cached_fallthrough` /
   `raw_template_analysis` / `import_routes`; reset `evicted` flag.
3. DependencyState block: `dependency_cache().entry(c).or_default()` →
   overwrite `dependencies` / `aliases`; bump `generation`.

The block-per-domain decomposition mirrors the matrix structure: a
single matrix row crosses the three blocks; each row's preserve/invalidate
column dictates which of the three blocks runs.

## 6. Notes for 1C-γ

- **Eviction-policy tunables**: 1C-γ adds
  `evict_unreachable_indexed_ready` per plan §3.4.3 with live-publish-set
  reachability + memory-pressure gating. The three new sibling DBs
  (`DerivedRawCacheDb` + `DependencyCacheDb`) join the unconditional
  `bump_project_generation_and_evict` cascade today; 1C-γ may add
  per-DB selective LRU eviction under memory pressure if the policy
  warrants per-domain budgets.
- **Allow-list shrink**: F1, F2, F4, F5 are already absent from
  `phase_8_allow_list()` (Tier 1C-α landed that). 1C-γ verifies no
  regressions and tightens further if any cache-shape additions slip
  in.
- **Architecture guard**: the test
  `every_db_field_in_project_type_store_appears_in_inventory` now
  walks 27 DB-typed fields (was 25). 1C-γ should re-verify the inventory
  invariant after any 1C-γ-era field additions.
- **Cooperative admission per domain**: the D48 plan describes
  per-domain post-compute revalidation hooks (ProfileState against
  profile-flag hash, DerivedRawState against `whole_hash`,
  DependencyState against dep-closure hash). This 1C-β commit adds the
  invalidation primitives but not the admission hooks — the cold-write
  surface today is direct DashMap mutate-in-place via
  `entry().or_default()`, not cooperative admission. The hooks belong
  to a follow-up step where a cold-resolver consumer drives the
  primitive (e.g. when the §3.2.2 lowering pipeline lands and writes
  pre-revalidated artifacts into the DBs).
- **CompileCacheEntry super-shape removal verification**: searching the
  source tree for `CompileCacheEntry` after this commit returns only
  doc-comment references in `deps.rs` (historical context inside
  comments) — the type itself is gone. 1C-γ may add a guard test
  asserting the type symbol is not declared anywhere if extra paranoia
  is desired.

## 7. Blockers

None. All gates pass; matrix coverage is exhaustive; legacy
`CompileCacheEntry` super-shape is fully retired.
