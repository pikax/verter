# Relation working-state inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `RelationDomainRuntime.completed_members` (`CompletedRelations::members`: the decided relation members queued for the root's batched publish) | The machinery root drains them into the batched publish, the request completion tier keeps them, or an abort ends their flights | REQUIRED | One transaction's member batch, until its publish or abort | `src/project_semantic_dispatch/dispatch_txn.rs` | always |
| `CompletedRelations::settled` (positions of the queued members whose answers are cold, hashed by key; replaces the key-to-payload map that copied each key and payload) | `relation_warm_step` answers a pair met again before the batch publishes from its queued member | REQUIRED | Cleared with the batch it indexes | `src/project_semantic_dispatch/dispatch_txn.rs` | always |
| `PreparedQueryIdentity._roots` of a relation identity whose interned key already roots every operand in the issuing arena (`InternedRelateKey::roots_all_in`): left empty instead of a second root set over the same nodes | The identity's interned key keeps its operands live for as long as the identity holds it; a key whose operands are not all rooted there still takes its own root set, and a stale or foreign operand is still refused | REQUIRED-lifetime | One prepared identity | `src/semantic_query_memo/prepared.rs`, `src/semantic_query_memo/family_intern.rs` | always |
| `RELATION_PAIR_BYTES` (the construction bytes one structured comparison reserves; value unchanged) | The connected demand's construction-byte rail and the batched rejection of missing-property pairs | REQUIRED-budget | One connected demand | `src/project_semantic_dispatch/connected_demand.rs` | always |

## verter_session

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| Allocator attribution of the relation's working state (`a_relation_holds_no_more_than_it_charges`, the canary's `live_bytes` reading, and the `VERTER_RELATION_PAIR_ARMS` opt-in sizes) | none — measures peak and resident bytes per related pair against the demand's charge | OPTIONAL | One test process | `tests/allocation_cases/construction_bytes.rs`, `tests/allocator_canaries.rs` | the separate counting-allocator test binary only; absent from every library build |

No production counter, trace or history is added. The per-pair working state
shrinks by holding each key once: the queued member's, with the cold-answer
index holding positions and a relation identity reusing its interned key's
roots. The attribution that measures it lives only in the counting-allocator
test binary.
