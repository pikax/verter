# Component-Meta Trace Follow-ups

Issues that are not simple implementation fixes and require deeper investigation.

## 1. ModuleFactsDb ImportRoute validation causes O(n) redundant materialization

- **Component(s)**: ALL components
- **Latest trace path**: `tmp/batch1-trace-003/`
- **Current hot spans**: 199 `build_snapshot_from_cached_parse`, 327 `read_analysis_source` during expansion
- **Suspected root cause**: Module_facts entries include `DerivedFactHash::ImportRoute` validation fact. For untracked dependency files, this always fails `get_if_valid`.
- **Why not a simple fix**: Removing ImportRoute breaks invalidation for test-only workspace-injected files. Requires `tracks_file`-aware insertion or whole_hash population for compile_cache-only entries.
- **Likely next fix**: Add `StoreView::tracks_file` and conditionally include ImportRoute fact in `get_or_materialize`.
- **Impact**: ~33ms overhead (small relative to total)

## 2. Solver type resolution is the dominant cost (~95% of macro expansion)

- **Component(s)**: ALL slow components
- **Latest trace path**: `tmp/batch3-5-trace/`
- **Current hot spans**: `compute_evaluated_types_expand_macros` (1.5-18s)
- **Suspected root cause**: Deep recursive type resolution through reka-ui and Vue barrel files. 127+ types resolved per component, ~10ms average per type.
- **Why not a simple fix**: Inherent computation cost of type expansion. Requires solver-level caching or type-shape memoization.
- **Likely next fix**: Profile solver internals to find redundant work across macros in the same file.

## 3. ~~Input, Select, Textarea return no component meta~~ RESOLVED

**Root cause**: `build_effective_args` in the type solver produced `NodeId::UNRESOLVED`
(u32::MAX) for generic type parameters without explicit arguments or defaults. This
value was later passed to `arena.get()` which panicked in release builds.

**Fix**: `arena.get()` returns a static `Unknown` node for UNRESOLVED IDs. Also,
`build_effective_args` now produces `arena.primitive(PrimitiveKind::Unknown)` instead
of `NodeId::UNRESOLVED`. Commit: `c78feab2`.

**Results**: Input (43 props, 526ms), Select (42 props, 8.9s), Textarea (38 props, 309ms).

## 4. ~~Table.vue times out at 40s~~ RESOLVED

Same root cause as #3 — the arena panic was being caught/retried, causing the 40s timeout.

**Results**: Table (46 props, 8.7s). Commit: `c78feab2`.
