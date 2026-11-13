# Union rank recovery inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| Union rank map (`union_rank_order`: node id → first position in `VerterStableV1` union order, built once after the sort) | The flow-return supertype reduction (`reduce_arms_to_supertypes`) visits its operands in union order through it, one map insert and one lookup per arm instead of a search of the sorted list per comparison | REQUIRED | One rank recovery call | `src/semantic_query/stable_key.rs` | always |
| Rank-map probe count (`union_rank_order_observed`) | none; the linear-growth rank recovery test only | OPTIONAL | One rank recovery call | `src/semantic_query/stable_key.rs` | `cfg(any(test, feature = "semantic-observe"))` |
