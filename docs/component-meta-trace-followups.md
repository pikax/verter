# Component-Meta Trace Follow-ups

Issues that are not simple implementation fixes and require deeper investigation.

## Format

Each entry includes:
- **Component(s)**: affected components
- **Latest trace path**: where to find the trace
- **Current hot spans**: what's expensive
- **Suspected root cause**: why it's expensive
- **Why not a simple fix**: what makes it complex
- **Likely next fix**: what to try next

---

## 1. ModuleFactsDb ImportRoute validation causes O(n) redundant materialization

- **Component(s)**: ALL components (Accordion: 139 redundant builds, App: more)
- **Latest trace path**: `tmp/batch1-trace-003/`
- **Current hot spans**: 199 `build_snapshot_from_cached_parse`, 327 `read_analysis_source` during `compute_evaluated_types_expand_macros`
- **Suspected root cause**: `ModuleFactsDb.get_or_materialize` stores entries with `DerivedFactHash::ImportRoute` validation fact. For dependency files not tracked by the store view, this fact always fails `get_if_valid`, forcing re-materialization on every access.
- **Why not a simple fix**: Removing ImportRoute from module_facts validation breaks invalidation for workspace-injected files that have `set_import_dependencies` routes. A `tracks_file`-aware insertion path requires new `StoreView` trait methods and careful interaction with `snapshot_module_fact_hashes` which invalidates freshly-materialized entries.
- **Likely next fix**: Two options:
  1. Add `tracks_file` to `StoreView`, skip ImportRoute fact for untracked files in `get_or_materialize`. Requires also populating `whole_hashes` from module_facts for compile_cache entries without scheduler data.
  2. Have `ensure_module_facts_in_view` use a secondary lookup when the validated cache misses: check if the primary entries map has the key (regardless of validation) — if yes, the entry was just materialized in this session and can be returned directly.

## 2. Solver type resolution is the dominant cost (~95% of macro expansion)

- **Component(s)**: ALL components (Accordion: 1577ms total, ~1350ms in solver after subtracting resolve_imported_type_root)
- **Latest trace path**: `tmp/batch1-trace-002/`
- **Current hot spans**: `compute_evaluated_types_expand_macros` (1577ms), `resolve_imported_type_root` (219ms), solver internal resolution (~1350ms)
- **Suspected root cause**: The type solver performs deep recursive type resolution for each macro. For nuxt-ui's Accordion with 4 macros, it resolves ~129 prepared type declarations across 71 unique files. Each resolution involves multiple callbacks through `SessionSolverHost` into the host's module_facts and prepared_decl_bundle caches.
- **Why not a simple fix**: This is the inherent cost of deep type resolution. Reducing it requires either:
  - Caching fully-resolved type shapes (not just prepared declarations)
  - Reducing the number of types that need resolution (better import graph pruning)
  - Making the solver's type-expansion more incremental
- **Likely next fix**: Profile the solver's internal resolve loop to find which types are most expensive to resolve, and whether any resolution is redundant across macros in the same file.
