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

## 3. Input, Select, Textarea return no component meta

- **Component(s)**: Input.vue, Select.vue, Textarea.vue
- **Latest trace path**: `tmp/batch3-5-trace/`
- **Current hot spans**: Query completes (Closed) but no meta returned
- **Suspected root cause**: These components may use patterns that the meta resolver doesn't handle (e.g., complex type intersections with HTML element types from `../types/html.ts`).
- **Why not a simple fix**: Need to investigate what specific type pattern causes the resolver to return None.
- **Likely next fix**: Check if these components use `defineProps<Props & HTMLInputElement['$props']>()` or similar patterns.

## 4. Table.vue times out at 40s

- **Component(s)**: Table.vue
- **Latest trace path**: `tmp/batch3-5-trace/Table.trace.log` (24897 lines)
- **Current hot spans**: Still resolving macro types after 40s, 18 macros in the component
- **Suspected root cause**: Table.vue has 18 macros and imports from `@tanstack/vue-table` which has complex generic types (CoreOptions, etc.). The type solver may hit exponential expansion.
- **Why not a simple fix**: Requires type expansion budgets or solver-level cycle detection for deeply generic types.
- **Likely next fix**: Check `@tanstack/vue-table` type complexity and add budget limits for per-component type expansion.
