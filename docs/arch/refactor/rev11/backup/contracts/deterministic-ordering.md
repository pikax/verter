# Deterministic Ordering and Stable-ID Contract

**Status:** Normative observable-output contract.

# 1. General rule

No observable output, public ID, serialization, map segment order, diagnostic order, dependency fingerprint, or proof digest may depend on:

- hash-map/set iteration order;
- pointer/allocation address;
- worker/shard assignment;
- task completion/follower arrival order;
- cache insertion/eviction history;
- process randomness;
- wall clock;
- ambient directory enumeration order.

# 2. Canonical ordering sources

Use, in priority order:

1. authored source order where language/product semantics preserve it;
2. preassigned stable operation/input ordinal before parallel fork;
3. canonical typed key order;
4. explicit kind rank plus stable local identity;
5. canonical byte/string order only when no semantic/source order exists.

Parallel workers return `(stable_ordinal, result)` and the owner merges by ordinal, not completion time.

# 3. Diagnostics

Unless an operation defines a stronger semantic order, diagnostics sort by:

```text
canonical source ID
start byte
end byte
severity rank
stable diagnostic code
canonical typed-argument tie breaker
```

Rendered message text is not used as the primary ordering key.

# 4. Graph and public IDs

Graph/public snapshot IDs are assigned through deterministic canonical traversal. Internal arena/hash-cons IDs do not escape. A recommended order is `(root ordinal, authored span, node-kind rank, stable local ordinal)`, with explicit tie-breaking for synthesized nodes.

Snapshot-local IDs are not promised stable across semantically different snapshots unless a public contract explicitly says so. Stable cross-snapshot IDs require content/owner identity, not allocation order.

# 5. Maps, dependencies, and strings

- mapping segments are emitted in canonical generated-position order with deterministic tie breaks;
- dependency/read sets are canonicalized before fingerprinting/serialization;
- string tables use canonical traversal/ordering, not concurrent interner insertion order;
- path display normalization is separate from path semantic identity;
- domain-separated hashes include a versioned canonical encoding.

# 6. Serialization

Canonical serialization fixes field order where the format permits, set/map ordering, integer encoding, optional-field policy, normalization of equivalent values, and schema/domain identity. Nondeterministic protobuf/map iteration must be normalized before bytes are compared or signed.

# 7. Tests

Run equivalent operations across:

- randomized collection insertion order;
- worker counts and chunking;
- randomized legal task delays;
- cold/warm/evicted states;
- direct/prepared/managed regimes with equal product contract;
- native threaded, native single-thread, and WASM where supported.

Compare output bytes, diagnostics, maps, dependencies, exactness, public IDs, and terminal serialization.
