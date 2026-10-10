# Semantic node ownership inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| Slot reference count (`Chunk::refs`: counted child edges plus roots, with the `PERMANENT` and `DYING` states) | Intern hits (atomic live upgrade that refuses a dying node), child-edge counting on intern, root acquisition and release, destruction | REQUIRED-lifetime | The slot: set at allocation, cleared when the slot is released | `src/semantic_query_memo/arena.rs` | always |
| Slot fingerprint (`Chunk::fingerprints`) | Destruction forgets the node's dedup entry without re-hashing its payload | REQUIRED | The slot | `src/semantic_query_memo/arena.rs` | always |
| Destruction queue (`ArenaCore::pending`, `ArenaCore::draining`) | The one drainer destroys every dying node iteratively; a releaser that finds a drain running leaves its node queued | REQUIRED-lifetime | Per arena; a node leaves it when destroyed | `src/semantic_query_memo/arena.rs` | always |
| `ArenaCore::released_any` | The store's warm-read liveness guards (`result_is_live`, `family_names_released_node`, `unresolved_reach`) stay one relaxed load until a slot is first released, by a close sweep or by its count | REQUIRED | Per arena; set once, never cleared | `src/semantic_query_memo/arena.rs` | always |
| `ArenaCore::identity` | A lease read against a store other than the one that minted it is refused (`LeaseError::Foreign`) | REQUIRED | Per arena | `src/semantic_query_memo/arena.rs` | always |
| `NodeLease` and its pinned `RetentionCharge` | A caller outside the store holds one node and everything it retains readable; the handle's own bytes are pinned in the process retention account while it lives | REQUIRED-lifetime | Until the lease drops | `src/semantic_query_memo/node_roots.rs` | always |
| `NodeRootSet` | A retained holder's multiset of named nodes, counted once per occurrence | REQUIRED-lifetime | Until the holder drops | `src/semantic_query_memo/node_roots.rs` | always |
| Root scope (`ScopeRoots` held set, the thread-local `SCOPES` stack, `RootScopeHandle`) | Every intern made inside a scope is rooted by it; nested scopes of one store join; a handle carries the scope to another thread | REQUIRED-lifetime | Until the scope's last guard or handle drops | `src/semantic_query_memo/node_roots.rs` | always |

`SemanticGraphStore::node_refs_for_tests` and `SemanticGraphStore::pending_destruction_for_tests` exist only under `test`/`test-support` and are absent from default builds.

No optional counter, trace or allocation/attribution history is added: every item above is ownership or policy-charge state, so nothing is gated behind `semantic-observe`.
