# Phase 4 / 5 Cutover Handoff — `_in_view` Signature Cut and Final Deletion

**Intended reader:** the next agent (fresh Claude or equivalent) taking
this branch to completion.
**Branch:** `refactor/semantic-db-overhaul`
**Authoritative plan:** `C:\Users\david\.claude\plans\component-meta-project-global-cache-overhaul.md`
**Last audit:** `.claude/audits/project-global-type-rewrite-correctness-audit.md`
**Repo root:** `D:\dev\personal\verter`

---

## 0. Read-before-anything

Before you touch a single file, read in this order:

1. `.claude/audits/project-global-type-rewrite-correctness-audit.md` — what
   landed, what is left, the mandatory completion contract.
2. `C:\Users\david\.claude\plans\component-meta-project-global-cache-overhaul.md`
   — the full rewrite plan. Pay particular attention to §G1 (explicit
   deletions), §C (dep-signature propagation), §J (old-vs-new audit),
   §Phase 4 and §Phase 5.
3. `D:\dev\personal\verter\CLAUDE.md` — project-level rules.
4. Skills: `/type-resolution`, `/component-meta`, `/host-session`.
5. `crates/verter_session/src/host_request_view.rs` — the module you are
   deleting. Read the whole file; the migration requires replacing every
   method it exposes with an equivalent live-host probe.

## 1. What the cut actually is

The rewrite has landed every piece of scaffolding (Phases 0 → 3, plus the
Phase 4 memo-retirement slice). What remains is the mechanical-but-deep
cutover that deletes the request-view architecture from the hot path.

**Target end-state**: `crates/verter_session/src/host_request_view.rs`
deleted. Zero `_in_view` signatures in production hot-path code. Zero
`RequestStoreView` / `CURRENT_REQUEST_VIEW` references outside tests that
assert it is gone.

**Current occurrence counts** (as of commit `ab48b7e1`):

| File | `_in_view` | `RequestStoreView` | Must drop to |
| ---- | ---------: | -----------------: | :----------: |
| `crates/verter_session/src/host_manage.rs`                                | 228 | 83 | 0 |
| `crates/verter_session/src/host_resolve.rs`                               | 150 | 41 | 0 |
| `crates/verter_session/src/meta_resolve.rs`                               |  56 | 22 | 0 |
| `crates/verter_session/src/resolver_core/component_meta_query_engine.rs`  |  20 | 10 | 0 |
| `crates/verter_session/src/resolver_core/solver_host.rs`                  |  18 |  5 | 0 |
| `crates/verter_session/src/resolver_core/component_meta_registry.rs`     |   9 |  3 | 0 |
| `crates/verter_session/src/meta.rs`                                       |   3 |  2 | 0 |
| `crates/verter_session/src/host_request_view.rs`                          |   —  |  — | **deleted** |

Plus test files (lower priority, but they call `owned_or_ambient_request_view`
pervasively — `host_manage_tests.rs` has 60 call sites, `meta_resolve_tests.rs`
has 48). Those tests either rewrite against the new API or delete per plan §C1.

## 2. Migration pattern (the exact mechanical rewrite)

Every `_in_view` helper today follows one of two patterns:

### Pattern A — view-gated lookup

```rust
pub(crate) fn foo_in_view(
    &self,
    canonical: &str,
    store_view: Option<&crate::host_request_view::RequestStoreView>,
) -> Option<T> {
    let eff = crate::host_request_view::effective_request_view(store_view);
    if let Some(view) = eff.as_view() {
        // probe through the view (whole_hash, derived_hash, import_route, etc.)
        let hash = view.whole_hash(canonical)?;
        self.resolver.runtime.module_facts.get(canonical, view)
            .map(|facts| facts.shallow_state.clone())
    } else {
        // fallback: live host probe
        let hash = self.get_whole_hash(canonical)?;
        self.resolver.runtime.module_facts.get_any(canonical)
    }
}
```

**Rewrite to:**

```rust
pub(crate) fn foo(&self, canonical: &str) -> Option<T> {
    // Direct live-host probe — project_type_store + module_facts.get_any
    // service every request symmetrically because project-global caches
    // already validate through HostFenceValidator.
    let hash = self.get_whole_hash(canonical)?;
    self.resolver.runtime.module_facts.get_any(canonical)
        .map(|facts| facts.shallow_state.clone())
}
```

### Pattern B — view-threaded plumbing

```rust
pub(crate) fn bar_in_view(
    &self,
    arg: X,
    store_view: Option<&crate::host_request_view::RequestStoreView>,
) -> Option<T> {
    // ...uses store_view purely to pass it to child calls...
    self.sub_in_view(arg, store_view)
}
```

**Rewrite to:**

```rust
pub(crate) fn bar(&self, arg: X) -> Option<T> {
    self.sub(arg)
}
```

### Semantics contract — this is not a parameter rename

The plan is explicit (§1, §C, §C1): the new resolver path **abandons
request-snapshot consistency** in favor of live-host probes validated
through `HostFenceValidator` + `CompletionFence`. Do not preserve the
view's "point-in-time captured snapshot" semantics anywhere in the cut.

Every probe that today reads through `RequestStoreView` has a live-host
equivalent already in the tree:

| Old (view probe)                          | New (live host probe)                                               |
| ----------------------------------------- | ------------------------------------------------------------------- |
| `view.whole_hash(canonical)`              | `host.get_whole_hash(canonical)`                                    |
| `view.derived_hash(canonical, kind)`      | `host.derived_hash(canonical, kind)` (add accessor if missing)      |
| `view.import_route(canonical, specifier)` | `host.cached_import_route_resolution(canonical, specifier)` or `OwnerImportSurfaceDb::get` |
| `module_facts.get(canonical, view)`       | `module_facts.get_any(canonical)` (the view-gated flavor disappears) |
| `view.is_evalable(canonical)`             | `host.get_whole_hash(canonical).is_some()`                          |
| `view.tracks_whole_hash(canonical)`       | same                                                                |

The `effective_request_view` / `EffectiveView` helpers and the
`current_request_view()` / `CURRENT_REQUEST_VIEW` thread-local all delete
outright. Nothing replaces them — the live-host probe is always valid
because `project_type_store` entries are either current or evicted.

### `ensure_loaded` integration

Today `ensure_loaded` calls `VerterHost::record_current_request_extension_for`
to push the newly-loaded canonical into the ambient `RequestStoreView`'s
extension store. After the cut, `ensure_loaded` just publishes into
`ProjectTypeStore` / `ModuleFactsDb` and the live-host probes read it
naturally. Delete the extension-store plumbing.

### Tests

The `host_manage_tests.rs` / `meta_resolve_tests.rs` / `frontier_tests.rs`
tests that capture a view via `owned_or_ambient_request_view()` either:

- **Rewrite**: if the test is asserting cache-invalidation or
  resolver-behavior semantics, drop the view capture and call the new
  non-`_in_view` API directly. The behavior is identical for tests that do
  not rely on snapshot staleness.
- **Delete**: per plan §C1, some tests explicitly assert view-staleness
  semantics that the new architecture intentionally abandons. Examples:
  - `stale_store_view_keeps_owner_import_route_when_workspace_candidates_change` — delete
  - `stale_store_view_does_not_fallback_to_live_import_route_when_route_was_missing` — delete
  - `stale_store_view_keeps_resolved_exports_on_captured_reexport_graph` — delete
- **Rewrite around live validation**: the "captured view rejects stale
  dep" tests rewrite to assert that `HostFenceValidator` rejects stale
  dep signatures. Examples:
  - `stale_store_view_rejects_changed_dependency_eval_state`
  - `stale_store_view_rejects_changed_import_routes_and_reexports`
  - `prepared_type_decl_lookup_rejects_stale_cache_entries`
  - `regression_stale_prepared_decls_after_dep_resolution_change`

See plan §C1 for the full test-disposition list.

## 3. Execution plan (ordered slices)

Work bottom-up: leaf helpers first, then their callers, then the public
entry points. Each numbered slice below is a phase-final checkpoint that
must pass `cargo test --workspace --tests --verbose` and
`cargo clippy --workspace --lib --tests -- -D warnings` before moving on.

### Slice 1 — `meta.rs` (3 / 2)

Smallest file, only `with_overlay_target_context_view` threads the view
through. Convert `with_overlay_target_context_view` to not create or
pass a `RequestStoreView`. The callers in `get_declared_component_meta_with_resolution`,
`get_declared_component_meta_payload`, `get_component_meta_payload` stop
receiving the view parameter; their downstream calls
(`host.resolve_component_meta_in_view`, `host.try_get_cached_meta_payload`,
`host.store_meta_payload`, `host_manage::extract_component_meta_from_resolved*`)
have already been prepared to accept live-host-probe flow — but those
flows don't exist yet, so slice 1 depends on slices 2–8. Do slice 1 last
of the production-code slices.

### Slice 2 — `resolver_core/component_meta_registry.rs` (9 / 3)

Second smallest. Migrate its `_in_view` helpers to live-host probes.
Cross-check every call site in `host_manage.rs` / `host_resolve.rs` that
calls into this module — update the parameter list there.

### Slice 3 — `resolver_core/solver_host.rs` (18 / 5)

`SessionSolverHost` methods. All internal. Drop the view parameter and
replace view probes with `host.*` equivalents.

### Slice 4 — `resolver_core/component_meta_query_engine.rs` (20 / 10)

`TypeQueryEngine` owns a `store_view` field today. That field is removed.
The `solve_scoped` path similarly loses its view plumbing. Test coverage
in `component_meta_query_engine` tests (via `owned_or_ambient_request_view()`)
rewrites to the live-host variant.

### Slice 5 — `meta_resolve.rs` (56 / 22)

Resolver step. Migrate in one pass — calls into `host_resolve.rs` and
`host_manage.rs` will be broken temporarily because slice 6/7 haven't
run yet. This slice's commit may leave the tree red; the phase-final
commit is after slice 7.

### Slice 6 — `host_resolve.rs` (150 / 41)

Large. `HostFrontierAdapter`, `resolve_external_type_from_loaded_files_in_view`,
the whole `resolve_component_meta_*_in_view` surface, `resolve_route_type_edge_in_view`,
`cached_route_owned_*_in_view`, `route_owned_shallow_state_in_view`,
`build_named_type_export_route_entry_in_view`, `read_dep_source_for_type_resolution_in_view`.

### Slice 7 — `host_manage.rs` (228 / 83)

The bulk. Every `prepared_type_decl_in_view`, `shallow_file_state_in_view`,
`ensure_module_facts_in_view`, `current_eval_state_in_view`,
`resolve_imported_type_root_in_view`, `owner_import_surface_in_view`,
`resolve_owner_direct_import_in_view`, `external_type_analysis_in_view`, etc.

After slice 7 the tree must compile and tests must pass. This is the
first phase-final commit that can publish cleanly.

### Slice 8 — Test migration

Rewrite or delete the tests per plan §C1 and §3 (tests section) above.
The `host_manage_tests.rs` 60 call sites and `meta_resolve_tests.rs` 48
call sites drop their view captures.

### Slice 9 — G1 deletions

Only after slice 8 lands:

- Delete `RequestStoreView`, `RequestViewGuard`, `TouchOutcome`,
  `RequestExtension`, `EffectiveView` from `host_request_view.rs`
- Delete `CURRENT_REQUEST_VIEW` thread-local
- Delete `current_request_view`, `effective_request_view`
- Delete `VerterHost::owned_or_ambient_request_view`,
  `VerterHost::build_request_store_view`,
  `VerterHost::record_current_request_extension_for`
- Fold `host_owned_resolved_named_types` into `SemanticGraphStore`
  (cf. §4 below) — or if that is too large a scope, leave it for a
  follow-up slice with its own test coverage
- Delete scheduler-freshness probes in `base_eval_env_arc_in_view` /
  `current_eval_state_in_view` once those helpers lose their view
  parameter in slice 6/7

### Slice 10 — delete `host_request_view.rs`

After all code that references types from that module is gone, remove
the file. Remove its `mod` declaration from `lib.rs`. The
`phase4_in_view_surface_ratchet` test and the
`phase4_request_view_memo_retirement_source_audit` test both delete in
the same commit.

### Slice 11 — ModuleFactsDb retirement

Per plan §Phase 5:

```bash
grep -rn 'ModuleFacts\b' crates/verter_session/src | grep -v module_facts_db.rs
```

must return zero hits. Migrate every consumer to
`ProjectTypeStore::indexed()` (`IndexedReadyDb`) + direct
`ShallowFileState` lookups. Then delete:

- `crates/verter_session/src/resolver_core/module_facts_db.rs`
- The `transitional_module_facts_db_coexists_with_indexed_ready` test
- Any `ModuleFactsDb` field on `VerterHost` / resolver runtime

### Slice 12 — Full SemanticQueryApi dispatch

Today `ProjectSemanticDispatch::execute` only wires `ResolveDecl`
meaningfully (and returns an `Alias(Opaque(Miss))` placeholder at that).
Wire the remaining variants through existing solver entry points (now
that their `_in_view` parameters are gone):

- `Instantiate` → existing solver instantiation path
- `ProjectMember` / `IndexedAccess` → `TypeNavigator` for intermediate
  hops, query API for new nodes
- `Expand` → existing `ExternalTypeFrontier` expansion behind the memo
- `KeyOf`, `MappedType`, `Conditional`, `TypeOf`, `NormalizeUnion`,
  `NormalizeIntersection`

Each variant must populate dep-signature fragments observed during the
build so warm hits contribute transitive deps to the active
`CompletionFence`.

### Slice 13 — Dep-signature propagation

Per plan §C ("Dependency-signature propagation rule — mandatory"): wire
every `ValidatedFactCache` write site to also publish into the
semantic-graph memo and component-meta result-cache dep-signatures.
Without this, `ComponentMetaResultDb` serves stale warm-hit composites
even though each entry looked valid in isolation.

### Slice 14 — Old-vs-new correctness audit (plan §J)

Immediately before the final `host_request_view.rs` deletion commit (or
after it if the legacy path was already deleted — in which case the
diff is between this branch and the pre-cut commit `5d92dae6` or the
most recent green state before the cut).

```bash
# Capture baseline from pre-cut commit (one-off; stash and git worktree)
git worktree add /tmp/verter-pre-cut 5d92dae6
pushd /tmp/verter-pre-cut
pnpm install && pnpm run build:native
node scripts/benchmark/trace-component-corpus.mjs --output-dir=tmp/cm-trace-pre
popd

# Capture new-path trace
node scripts/benchmark/trace-component-corpus.mjs --output-dir=tmp/cm-trace-new

# Diff
npx tsx packages/benchmark/src/trace-check.ts tmp/cm-trace-new \
  --batch "Accordion,Alert,App" --strict --check-expected
```

Diff native payloads byte-for-byte. Record intentional deltas in the
audit file. Delete the transient baseline worktree.

### Slice 15 — Skill + CLAUDE.md updates

Update:
- `CLAUDE.md` — drop the "Project-global cache status (Phase 2-4 landed)"
  block and replace with the final-state description; remove the
  "Legacy request-view note" paragraph
- `.claude/skills/host-session/SKILL.md` — drop the "Request-view state"
  and "Ambient-view-first helper pattern" paragraphs; describe the
  project-global cache as the single authority
- `.claude/skills/type-resolution/SKILL.md` — drop "Legacy request-view
  era" language if any remains
- `.claude/skills/component-meta/SKILL.md` — drop the "install a
  RequestStoreView" wording in the final-result-cache section

### Slice 16 — Final audit archive

Update `.claude/audits/project-global-type-rewrite-correctness-audit.md`
to the "**COMPLETE**" state. Delete this handoff file (`.claude/audits/project-global-cache-phase4-cutover-handoff.md`)
in the same commit since it's no longer needed.

## 4. Folding `host_owned_resolved_named_types` into `SemanticGraphStore`

The plan's §G1 calls this out explicitly:

> `host_owned_resolved_named_types`: fold into the host-owned semantic
> query cache keyed by resolved declaration / expansion identity, then
> delete the dedicated map

Today `host_owned_resolved_named_types` is a `DashMap<HostResolvedNamedTypeKey,
Arc<ResolvedElements>>` keyed by `(canonical_id, whole_hash,
ResolvedNamedTypeCacheKey)` where `ResolvedNamedTypeCacheKey` comes from
the parser crate (`verter_compiler::utils::oxc::vue::resolve_type::cache_keys`).

This is not a direct drop-in into `SemanticGraphStore` because
`ResolvedElements` is not `SemanticNodeData`-shaped. Two landing options:

**Option A** (simpler, recommended for this cut): Replace the DashMap
with a dedicated `LiveValidatedCache<HostResolvedNamedTypeKey, Arc<ResolvedElements>>`
owned by `ProjectTypeStore`. This gets the cache onto the same validation
rail as the other project-global caches without requiring `ResolvedElements`
to be translated into `SemanticNodeData`. Delete the `host_owned_resolved_named_types`
field on `VerterHost`, move the adapter to consult `project_type_store.resolved_named_types()`.

**Option B** (harder, correct long-term): Translate `ResolvedElements`
into `SemanticNodeData` and unify with `SemanticGraphStore`. This is a
separate architectural track and should not block the `_in_view` cut.

Pick Option A unless there's a compelling reason for Option B.

## 5. Verification gates (run at every phase-final commit)

```bash
# Tight gates (first):
cargo test --package verter_session frontier_tests
cargo test --package verter_session host_manage
cargo test --package verter_session --lib

# Broad gates (before publishing):
cargo test --workspace --tests --verbose 2>&1 | tee /tmp/test-output.txt
cargo clippy --workspace --lib --tests -- -D warnings
cargo fmt --all --check

# TS side:
pnpm test

# Integration (slow — run once at slice 14, not at every checkpoint):
pnpm integration-test --skip-baseline --no-clone nuxt-ui element-plus coreui vuetify

# Component-meta corpus (also at slice 14):
node scripts/benchmark/trace-component-corpus.mjs --output-dir=tmp/cm-trace
npx tsx packages/benchmark/src/trace-check.ts tmp/cm-trace \
  --batch "Accordion,Alert,App" --strict --check-expected

# Warm-rerun regression (existing test):
cargo test --package verter_session --lib component_meta_warm_rerun_hits_final_result_cache_phase3

# LSP E2E if LSP behavior changes (rare for this cut, but run if in doubt):
pnpm run test:e2e
```

## 6. Non-negotiable invariants (plan § End-State)

- `RequestStoreView` / `CURRENT_REQUEST_VIEW` not in the component-meta /
  type-resolution hot path (final-tree grep returns zero)
- `host_request_view.rs` deleted
- `ModuleFactsDb` has zero production consumers; its file is deleted
- `IndexedReady` is the single canonical post-parse artifact
- Shared semantic work keyed by `SemanticQueryKey` through the host-owned memo
- Warm hits contribute transitive dep-signature fragments into the active `CompletionFence`
- `CompletionFence` retries bounded to 3; `UnstableState` publishes nothing
- Direct owner imports resolve once per owner version via `OwnerImportSurfaceDb`
- Navigators stay non-owning; new semantic nodes enter through `SemanticQueryApi::execute`
- No reserved-name intrinsic handling for `Pick` / `Omit`-style aliases
- No feature flags, compat shims, fallback branches, or dormant helpers survive
- Source-audit test asserts `RequestStoreView` / `CURRENT_REQUEST_VIEW` do
  not appear in hot-path module tree (extend existing
  `phase4_request_view_memo_retirement_source_audit` to cover every
  hot-path file listed in §1 above, or delete the ratchet test entirely
  if its assertions are subsumed)

## 7. Commit cadence

Use conventional commits (`refactor(session): …`). Phase-final commits
must be green on every gate in §5. Intermediate commits inside a large
slice may temporarily break tests. Suggested structure:

- `refactor(session): cut _in_view from component_meta_registry (slice 2)`
- `refactor(session): cut _in_view from solver_host (slice 3)`
- `refactor(session): cut _in_view from component_meta_query_engine (slice 4)`
- `refactor(session): cut _in_view from meta_resolve (slice 5) [wip]`
- `refactor(session): cut _in_view from host_resolve (slice 6) [wip]`
- `refactor(session): cut _in_view from host_manage (slice 7) — tree green`
- `refactor(session): cut meta.rs overlay view plumbing (slice 1)`
- `test(session): rewrite/delete request-view-era tests (slice 8)`
- `refactor(session): delete RequestStoreView and friends (slice 9)`
- `refactor(session): delete host_request_view.rs (slice 10)`
- `refactor(session): retire ModuleFactsDb (slice 11)`
- `feat(session): wire full SemanticQueryApi dispatch (slice 12)`
- `feat(session): propagate dep-signatures through every ValidatedFactCache write (slice 13)`
- `test(session): old-vs-new corpus correctness audit (slice 14)`
- `docs: sync CLAUDE.md + skills to final architecture (slice 15)`
- `docs(session): archive final audit, retire cutover handoff (slice 16)`

Do not skip hooks (`--no-verify`) or amend previous commits. Create new
commits for every fix after a hook failure.

## 8. Pitfalls and sharp edges

1. **`upsert()` returns `HostUpdateResult` which is `#[must_use]`**.
   Test code calling `host.upsert(…)` without binding needs `let _ = …`.
   The last commit (`ab48b7e1`) scoped-allowed this in `verter_lsp` test
   modules; the `verter_session` tests already handle it.
2. **`with_overlay_target_context_view`** (`meta.rs`) holds the
   `RequestViewGuard` for the duration of the callback. Deleting it means
   the overlay gate alone guards the closure. Verify that nothing
   downstream still expects `CURRENT_REQUEST_VIEW` to be populated —
   grep for `current_request_view()` in every call path that
   `f(&self.project.host, …)` reaches.
3. **`effective_request_view(None)` returns `OutsideRequest`** today and
   lets live probes through. After the cut, *every* probe is live. If
   any code depends on the `OutsideRequest` branch enabling fallbacks
   the cut might unintentionally promote fallback behavior to the
   default — inspect every `OutsideRequest` arm before deleting.
4. **`tracks_file` in `impl StoreView for RequestStoreView`** has a
   subtle behavior: extension-store entries deliberately don't count as
   "tracked" for `HostStoreView::validates_all` purposes. This compat
   rule must survive the cut — but the equivalent live-host path is
   simpler (project-global cache entries either exist or they don't,
   and the `HostFenceValidator` handles staleness through dep signatures).
5. **`owned_or_ambient_request_view` is called from tests to capture a
   stable snapshot for later assertions**. In the new world, tests that
   want to freeze host state between two observations must use an
   explicit generation snapshot (read `project_type_store.project_generation()`
   before and after the mutation under test) rather than a captured view.
6. **The Phase 4 ratchet test** (`phase4_in_view_surface_ratchet` in
   `crates/verter_session/src/project_global_cache_tests.rs`) will fail
   with your reductions. Lower the ceilings *in the same commit* that
   drops the counts, or delete the whole test when the cut completes.
7. **`host_manage.rs` has `pub(crate) fn host_owned_resolved_named_types_len_for_test`**
   — it's only used by one test in `host_manage_tests.rs`. Delete both
   in the same commit when you migrate the cache (option A in §4 above
   renames the accessor; option B deletes it).
8. **Integration tests depend on production behavior byte-for-byte** —
   the `pnpm integration-test` suite against `nuxt-ui`, `element-plus`,
   `coreui`, `vuetify` must match or improve the pre-cutover baseline.
   A subtle semantic drift in the cut (e.g., stopping at `module_facts.get_any`
   when the view path would have called `ensure_module_facts` on miss)
   could silently break these. Run the integration suite before the
   final deletion commit.

## 9. If you run out of runway

Commit what you have. Update the ratchet ceilings in
`phase4_in_view_surface_ratchet` to match the new counts. Update this
handoff file with your progress and what's left. The ratchet guarantees
your work is not regressed by later commits.

Do **not** leave the tree in a red state across session boundaries. If a
slice is half-done, finish the compilation fixes before you commit even
if some tests temporarily fail — the next agent needs a tree that builds.

---

*Generated 2026-04-18 by Claude Opus 4.7.*
