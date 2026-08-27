# Exact operative source-clause attachment — TIF1

Schema: 1. Node: `TIF1`. Clause count: 2. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-EXP-L905-11D308957060

- Kind: `context`; source: `successor-expansion.md:905-905`; target: `node:TIF1`; text SHA-256: `11d30895706049fbbc74bf63c86f9fe631194b244f8bfe9ab54d5a617585e181`.

~~~~markdown
### `TIF1.md` — TypeInfo-first ComponentInfo and component-meta cutover
~~~~

### SRC-EXP-L907-C60E2BD01999

- Kind: `forbidden`; source: `successor-expansion.md:907-912`; target: `node:TIF1`; text SHA-256: `c60e2bd01999b8595926d740593ae1ae5156000a6fac8433ce7413cd61eedf5a`.

~~~~markdown
**Intent:** make component information a versioned TypeInfo view plus framework facets and replace parallel metadata authority.
**Predecessors:** `TIF0`, `CAT0`.
**Subblocks:** (1) inventory existing component-meta fields/consumers; (2) define TypeInfo-root/type-role references; (3) define open tagged framework facets and partiality; (4) implement thin component-meta and vue-component-meta-compatible projections; (5) migrate consumers/public bindings to the accepted generic observation identity plus `TIF0` operation descriptors; (6) delete the old resolver/cache/schema authority atomically.
**Acceptance:** current Vue/Svelte component-meta use cases remain equivalent or receive an explicit breaking-schema disposition; every type-bearing field traces to its exact TypeInfo observation; compat output changes cannot alter semantic caching.
**Forbidden:** `ComponentContractEnvelope` as another type graph, metadata-owned resolution, type flattening without provenance, or universal required props/events/slots for inapplicable frameworks.
**Deletion/abort:** delete old resolver/cache/schema authority after cutover; rescope on any consumer that cannot identify whether it needs semantic facts or presentation compatibility.
~~~~
