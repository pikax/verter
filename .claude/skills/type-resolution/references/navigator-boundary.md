## Navigator Boundary Contract

Architectural target for the project-global cache cutover:

- Navigators are not a second resolver. They are thin path-walkers over the shared semantic query system.
- Navigators may perform only non-owning normalization:
  - unwrap already-resolved aliases
  - apply already-known substitutions
  - inspect already-materialized member or keyspace shape
  - choose the next hop in the requested path
- Navigators must not privately perform reusable semantic work such as:
  - recursively resolving a new declaration identity
  - crossing imports or barrel routes
  - instantiating a new generic body outside the query system
  - expanding a mapped, conditional, or indexed-access type through an ad hoc path
  - populating shared caches from outside the shared semantic query API
- Boundary rule: same semantic node may continue inline; a new semantic node must enter through the shared query API.
- Any operation that can recurse, cross files, instantiate meaning, or produce a reusable cached result must be represented as a semantic subquery.
- Prefer enforcing this boundary as a Rust API/trait split, not only as prose. Navigators should not receive owning semantic query operations directly.

Concrete expectation:

- While navigating `A['c']['full']['bar']`, the navigator may determine the next hop after `"full"` points at instantiated `C`, but resolving or expanding that instantiated `C` must occur through the shared semantic query layer rather than by private navigator recursion.

