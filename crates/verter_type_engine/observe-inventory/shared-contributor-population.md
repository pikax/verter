# Shared contributor population inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_session

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `GlobalContributorPopulation.views` (per-snapshot view memo keyed by target, name, symbol-space set, overlay population and `noLib`) | Every contributor answer (`OwnedLowering::contributor_answer` / `global_contributor_answer`) and the store-view validator's `observation_fingerprint`: each declaring base of a merged global shares the one materialized view instead of copying, sorting and fingerprinting the population per demand | REQUIRED | One published snapshot: built on first demand of a view, dropped with the snapshot when the next publication replaces it; a pure function of the immutable population, never validated separately | `src/global_contributors/mod.rs` | always |
| `GlobalContributorPopulation.project_views` | Program-scoped global lookup: filters and orders one immutable view instead of copying and scanning for each base | REQUIRED | Contributor snapshot and workspace membership revision | `src/global_contributors/mod.rs` | always |
| `GlobalContributorPopulation.collection_views` and its construction-local interface-declarer set | Augmentation collection: a shared slot order over immutable input arrays, preserving interface/namespace co-declaration rules | REQUIRED | Contributor snapshot, augmentation index and workspace membership revision | `src/global_contributors/mod.rs` | always |
| `GlobalContributorPopulation.materialized_view_entries` and `materialized_view_entry_count()` (contributor entries copied into views) | none; the all-base demand growth test only | OPTIONAL | One published snapshot | `src/global_contributors/mod.rs` | `cfg(any(test, feature = "semantic-observe"))` |
| `GlobalContributorPopulation.ordering_work` and `ordering_work_count()` (merge, rank and radix-pass work) | none; the linear-ordering scale tests only | OPTIONAL | One published snapshot | `src/global_contributors/mod.rs` | `cfg(any(test, feature = "semantic-observe"))` |

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| Finite indexed-access `SurfaceKeyIndex` | The path walker's shared known-member read over one reduced subject, with no per-key merged surface or copied contributor closure | REQUIRED | One finite projection | `src/project_semantic_dispatch/walk.rs` | always |
| `DeferredMemoEntry.signature`, `self_roots`, and retention charge; `DeferredEvaluationFrame.tracer` | Deferred evaluation's validated warm reads and dependency propagation into outer consumers | REQUIRED | Memo entry / one evaluation frame | `src/semantic_query_memo/hash_cons_memos.rs`, `src/project_semantic_dispatch/evaluate.rs` | always |

## verter_workspace

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `OwnersMemo.declaration_ranks` | Configured-file declaration precedence for contributor collections and global views | REQUIRED | One immutable workspace snapshot | `src/workspace_snapshot.rs` | always |
