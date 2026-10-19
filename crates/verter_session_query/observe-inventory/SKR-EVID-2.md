# Member-precise class member evidence inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

The member-precise read adds validity evidence only: one parse fact, the
producer that fingerprints it and the read that observes it. It adds no
retained store, counter, hook or trace.

## verter_session_query

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `compute_class_member_read_hash` (the class `Member` fact's fingerprint: the declaring elements' fingerprint folded with the `extends` reference's) | The lazy `Member` fact producer, at the read that records it and at every warm validation that recomputes it | REQUIRED | Pure function; retains nothing | `src/facts/hashing.rs` | always |

## verter_semantic_source

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `DeclBodyMemo::class_member_read_hash_in` (fingerprints the `ClassMembersNamed` selection of one name and the `ClassHeritage` selection of a single-statement class through lease-only re-borrows; absent for a class that is not one) | The lazy `Member` fact producer; a broken lease is a non-cacheable refusal | REQUIRED | Retains nothing; each call re-borrows the retained parse snapshot under its lease | `src/decl_body_memo.rs` | always |
| `DeclLoweringCounters::class_elements_lowered` (existing) now also counts the elements the fingerprint re-borrows | none; measurement only | OPTIONAL | Unchanged | `src/decl_lowering.rs` | `cfg(any(test, feature = "test-support", feature = "semantic-observe"))`, unchanged |

## verter_session

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `FactKey::Member { exporter: class, name, space: Type }` computed by `LazyBodyFactSource::compute` (the class member-read fact) | Recorded by a reference-free class member read in place of the class file's whole hash; every warm read revalidates it through `FileFacts::lookup_or_compute` against the live artifact, and an absent fact (no single class) validates only an absent observation | REQUIRED | Memoized in the artifact's lazy-fact side store with the other lazily computed body facts; released with the artifact | `src/fact_emission.rs` | always |

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `reference_free_class_member_read`: the `Member` fact observed onto the active fact tracer, and the scope-free (`NodeScopeId::Global`) answer object | The enclosing traced compute's read set and its strict self-roots: the class file no longer enters them through the read's nodes or through a locator lowering rooted on the class file | REQUIRED | Observed once per read into the enclosing traced compute; the nodes follow the graph's counted ownership | `src/project_semantic_dispatch/build.rs` | always |
