# Component-Meta Trace Progress

## Methodology

Each component is traced with a 40s hard timeout using `packages/benchmark/src/_trace-component.ts`.
Traces are validated against desired trace specs under `packages/benchmark/trace-specs/component-meta/`.
A component is "done" when its trace passes validation and workspace tests pass.

## Batch Status

### Batch 1: Accordion, Alert, App

| Component | Status | Baseline (ms) | Current (ms) | Props | Follow-ups |
|-----------|--------|---------------|--------------|-------|------------|
| Accordion | in-progress | 2224 | - | 13 | - |
| Alert | in-progress | 658 | - | 13 | - |
| App | in-progress | 4113 | - | 7 | - |

#### Fix 1: Accept untracked dependency files in store view validation

**Root cause**: `HostStoreView::validates` rejected `FileWholeHash` facts for files
not in `whole_hashes` (dependency files loaded after the store view snapshot was taken).
This caused `ValidatedFactCache::get_if_valid` to miss on every access to dependency files,
forcing `ensure_module_facts_in_view` through the expensive permissive fallback path.

**Symptom**: During Accordion macro expansion, `read_analysis_source` was called 328 times
(140 times for Accordion.vue alone, 57 for runtime-core.d.ts, 25 for reactivity.d.ts).
`build_snapshot_from_cached_parse` was called 200 times. All redundant.

**Fix**: Modified `HostStoreView::validates` to return `true` for untracked files (where
`whole_hashes.get(canonical_id)` is `None`), matching the existing behavior of
`store_view_allows_current_whole_hash`.

**Files changed**: `crates/verter_session/src/resolver_store.rs`

## Artifact Directories

| Directory | Description |
|-----------|-------------|
| `tmp/first3-alpha-trace-rerun7/` | Pre-fix baseline (from prior session) |
| `tmp/batch1-trace-001/` | Fresh baseline with rebuilt native |

## Components Covered

- [ ] Batch 1 (3): Accordion, Alert, App
- [ ] Batch 2+ (174): remaining alphabetical
