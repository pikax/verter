# Source Unit and Mapping Product Contract

**Status:** Normative generated-artifact interpretation contract.  
**Binding ADR:** ADR-010.

# 1. Separate mapping products

The architecture distinguishes:

1. `PlacementMap` — internal source-unit placement/composition;
2. `SourceProjectionMap` — required to interpret an IDE/provider companion;
3. `RuntimeSourceMapData` — optional runtime/build map segments;
4. `EncodedSourceMap` — terminal external serialized map.

These are different identities and products. A single “maps enabled” boolean is insufficient at architecture/API/benchmark level.

# 2. Source units

Framework frontends produce logical script/template/style/custom units with:

- stable logical lineage ID;
- exact source revision and placement;
- exact content/syntax identity;
- unit-relative spans wherever source-neutral reuse is claimed;
- deterministic unit/product order.

Moving unchanged bytes may preserve source-neutral syntax/semantic artifacts and rebuild only placement-dependent composition.

# 3. Atomicity

Generated code publishes atomically with every mapping required to interpret that exact code.

- an IDE companion requiring `SourceProjectionMap` cannot be Ready/published without it;
- runtime code without a requested runtime source map constructs no `RuntimeSourceMapData` or encoding;
- requesting encoded output may require map data and encoding as explicit terminal prerequisites;
- an operation requiring no map constructs no universal empty map.

# 4. Identity and ordering

Every map is bound to exact source/unit revision, generated artifact, output profile, and map compatibility domain. Segments use canonical deterministic ordering and reject overlap/ambiguity according to the product contract.

Map encoding/serialization identity is separate from semantic/generated code identity. Changing JSON field order or encoded format does not invalidate semantic/code computation when map data is unchanged.

# 5. Correctness

Tests cover:

- source-to-generated and generated-to-source round trips;
- inserted/deleted/moved unchanged units;
- multi-unit composition and boundary positions;
- Unicode/byte offset conventions;
- synthetic/helper ranges and unmapped segments;
- diagnostics/navigation/rename through IDE maps;
- runtime source maps on/off and terminal encoding;
- stale map paired with new code rejected;
- direct/prepared/managed equality;
- one construction/encoding per requested product identity.
