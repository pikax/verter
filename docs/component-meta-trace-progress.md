# Component-Meta Trace Progress

## Methodology

Each component is traced with a 40s hard timeout using `packages/benchmark/src/_trace-component.ts`.
Traces are validated against desired trace specs under `packages/benchmark/trace-specs/component-meta/`.
A component is "done" when its trace passes validation and workspace tests pass.

## Batch Status

### Batch 1: Accordion, Alert, App

| Component | Status | Baseline (ms) | Current (ms) | Props | Follow-ups |
|-----------|--------|---------------|--------------|-------|------------|
| Accordion | investigating | 2224 | 2107 | 13 | module_facts ImportRoute validation |
| Alert | investigating | 658 | 628 | 13 | same |
| App | investigating | 4113 | 4102 | 7 | same |

#### Fix 1: Accept untracked dependency files in store view validation (committed)

**Root cause**: `HostStoreView::validates` rejected `FileWholeHash` facts for files
not in `whole_hashes` (dependency files loaded after the store view snapshot was taken).
This caused `ValidatedFactCache::get_if_valid` to miss on every access to dependency files,
forcing `ensure_module_facts_in_view` through the expensive permissive fallback path.

**Impact**: ~5% reduction (Accordion: 2224→2107, Alert: 658→628, App: 4113→4102).
Small because the permissive path returns from in-memory caches quickly. The dominant
cost is in the solver itself and the ImportRoute derived fact still causes cache misses
(see follow-ups).

**Files changed**: `crates/verter_session/src/resolver_store.rs`, `host_manage.rs`,
`resolver_core/mod.rs`, `resolver_core/module_facts_db.rs`, `resolver_core/resolver_runtime.rs`

#### Key finding: ImportRoute validation fact on ModuleFactsDb entries

Module_facts entries include `DerivedFactHash::ImportRoute` as a validation fact.
For dependency files not tracked by the store view, this fact ALWAYS fails validation
(the store view doesn't have the ImportRoute hash). This causes 327 redundant
`read_analysis_source` calls (139 for Accordion.vue alone) and 199 redundant
`build_snapshot_from_cached_parse` calls during macro expansion.

Removing ImportRoute from module_facts breaks invalidation tests for workspace-injected
files (test-only scenario). The proper fix requires either:
- A `tracks_file`-aware insertion path for module_facts
- Or populating `whole_hashes` for compile_cache entries without scheduler data

Deferred to follow-up — the architectural fix is non-trivial and the overhead per call
is ~0.2ms (total ~33ms out of 1577ms).

## Artifact Directories

| Directory | Description |
|-----------|-------------|
| `tmp/first3-alpha-trace-rerun7/` | Pre-fix baseline (from prior session) |
| `tmp/batch1-trace-001/` | Fresh baseline with rebuilt native |
| `tmp/batch1-trace-002/` | After FileWholeHash acceptance fix |
| `tmp/batch1-trace-003/` | With ensure_module_facts_fast_hit instrumentation |

## Components Covered

- [ ] Batch 1 (3): Accordion, Alert, App — in progress
- [ ] Batch 2+ (174): remaining alphabetical
