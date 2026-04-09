# Component-Meta Trace Follow-ups

## Resolved

### 1. ~~ModuleFactsDb ImportRoute validation causes O(n) redundant materialization~~ RESOLVED

**Root cause**: `get_or_materialize` unconditionally included `DerivedFactHash::ImportRoute`
in validation facts. For untracked dependency files, this always failed `get_if_valid`.

**Fix**: Added `StoreView::tracks_file` trait method. `get_or_materialize` only includes
ImportRoute for tracked files (owner files in scheduler/compile_cache). Untracked dependency
files never have `set_import_dependencies` called, so their route facts are safe to omit.

**Impact**: ~14% improvement for Alert (628→540ms), ~3% for Accordion (2107→2037ms).
33% reduction in `read_analysis_source` calls during macro expansion.

**Commit**: `78430ea7`

### 2. ~~Solver type resolution is the dominant cost~~ PROFILED, INHERENT

**Finding**: Solver resolves 127-131 unique types per component with zero redundancy.
Each type averages 11-26ms for deep recursive expansion through Vue/reka-ui barrel type
definitions. No low-hanging optimizations found.

**Architecture note**: Improvement requires fundamental solver changes — incremental type
evaluation, shared arena node pools across queries, or type-shape memoization. These are
out of scope for the tracing campaign.

### 3. ~~Input, Select, Textarea return no component meta~~ RESOLVED

**Root cause**: `build_effective_args` in the type solver produced `NodeId::UNRESOLVED`
(u32::MAX) for generic type parameters without explicit arguments or defaults. This
value was later passed to `arena.get()` which panicked in release builds.

**Fix**: `arena.get()` returns a static `Unknown` node for UNRESOLVED IDs. Also,
`build_effective_args` now produces `arena.primitive(PrimitiveKind::Unknown)` instead.

**Results**: Input (43 props, 526ms), Select (42 props, 8.9s), Textarea (38 props, 309ms).

**Commit**: `c78feab2`

### 4. ~~Table.vue times out at 40s~~ RESOLVED

Same root cause as #3 — the arena panic was being caught/retried, causing the 40s timeout.

**Results**: Table (46 props, 8.7s).

**Commit**: `c78feab2`

### 5. ~~Description newlines stripped in JSDoc extraction~~ RESOLVED

**Root cause**: `parse_jsdoc()` in `crates/verter_semantic/src/analysis/jsdoc.rs` joined
multi-line description parts with `" "` (space) instead of `"\n"` (newline). Blank lines
between paragraphs were also dropped.

**Fix**: Join description parts with `"\n"`, preserve blank lines as empty strings for
paragraph breaks (`\n\n`). Added 3 tests.

**Commit**: `574e64fb`

## Open

### 6. Slot binding type collapse for function-typed slots

- **Component(s)**: Accordion (body, content, leading, trailing slots), likely others
- **Symptom**: Slot schema returns `{} | undefined` instead of full binding shape
  like `{ item: T; index: number; open: boolean; ui: Accordion['ui'] }`
- **Root cause**: These slots are typed as `SlotProps<T>`, a function type alias:
  `(props: { item: T, index: number, open: boolean, ui: Accordion['ui'] }) => VNode[]`.
  The type solver fails to resolve `Accordion['ui']` which is a deeply nested indexed
  access type: `ComponentConfig<typeof theme, AppConfig, 'accordion'>['ui']`. When
  the indexed access fails, the entire function parameter type falls back to `{}`.
- **Why the `default` slot works**: Its bindings are inline `{ item: T, index: number,
  open: boolean }` without indexed access types, so the solver resolves it fully.
- **Fix needed**: The type solver needs to either:
  1. Handle indexed access types on computed generic compositions
  2. Or preserve partial resolution (keep resolved members even when some fail)
- **Impact**: Affects nuxt-ui's Accordion, Breadcrumb, CheckboxGroup, CommandPalette,
  and any component using function-typed `SlotProps<T>` with complex UI type access.
