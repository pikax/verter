# Shared contributor population inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_session

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `GlobalContributorPopulation.views` (per-snapshot view memo keyed by target, name, symbol-space set, overlay population and `noLib`) | Every contributor answer (`OwnedLowering::contributor_answer` / `global_contributor_answer`) and the store-view validator's `observation_fingerprint`: each declaring base of a merged global shares the one materialized view instead of copying, sorting and fingerprinting the population per demand | REQUIRED | One published snapshot: built on first demand of a view, dropped with the snapshot when the next publication replaces it; a pure function of the immutable population, never validated separately | `src/global_contributors/mod.rs` | always |
| `GlobalContributorPopulation.materialized_view_entries` and `materialized_view_entry_count()` (contributor entries copied into views) | none; the all-base demand growth test only | OPTIONAL | One published snapshot | `src/global_contributors/mod.rs` | `cfg(any(test, feature = "semantic-observe"))` |

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| Per-collection interface-declarer set (`interface_declarers`) in augmentation contribution collection | The file-scope namespace fold: a namespace whose file also declares the interface of the name is skipped, decided by one set lookup instead of a scan of the population | REQUIRED | One contribution collection | `src/project_semantic_dispatch/build.rs` | always |
