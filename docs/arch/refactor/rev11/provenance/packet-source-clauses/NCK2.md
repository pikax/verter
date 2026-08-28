# Exact operative source-clause attachment — NCK2

Schema: 1. Node: `NCK2`. Clause count: 4. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-LEGACY-EXISTING-CACHE-001

- Kind: `requirement`; source: `legacy-arch-reconciliation.md:199-201`; target: `node:G1`; text SHA-256: `ecf95ce060d2ba6503880cd6ec607e851edb63871b62779606fd5ef981cdc27e`.

~~~~markdown
### EXISTING-CACHE-001

Fact/query/result caches use exact structural identity, read-set validation, complete-only admission, singleflight, cancellation, and reclaimable storage. Targets: `G1`, `G2`, `E4`, `H1`, all successor caches. Related source: `docs/arch/fact-based-cache.md`, blob `1f97d9be730193400629485e8c86415b35834f27`.
~~~~

### SRC-LEGACY-NCK-QUERY-001

- Kind: `requirement`; source: `legacy-arch-reconciliation.md:23-29`; target: `node:NCK0`; text SHA-256: `9372ba4833d43deb3133fe48c881dff958d1009effc4094bda39c522c1d28ec4`.

~~~~markdown
### NCK-QUERY-001 — Scoped first-class diagnostic queries

- Diagnostic results are first-class query values, not `GraphTypeNode` arms or identity-less side products.
- Primitive operations are region/file/project-rule/expression scoped.
- Whole-program checking is a bounded coordinator/stream, not a monolithic cache key.
- Complete-only cache admission applies.
- Targets: `NCK0`, `NCK2`, `NCK7`.
~~~~

### SRC-LEGACY-TRANSFER-3E96BF48EC48

- Kind: `requirement`; source: `legacy-architecture-transfers.md:320-325`; target: `node:NCK0`; text SHA-256: `9e49f7ccc41ffb05aa34dea9d059d333873074c8f55d5c84b5438285a90737e2`.

~~~~markdown
### LEGACY-TRANSFER-3E96BF48EC48

- Original path: `docs/arch/native-checker.md`; Git blob: `3e96bf48ec481e97b9fd3067041e21099d194944`; exact source SHA-256: `2a7124d22a468e005faad16b43bf2d64a5472e3bea30bb39f436c2f33b1cde06`.
- Exact retained source: `sources/legacy-architecture-transfers/native-checker.md`.
- Applicable authority: `NCK0`, `NCK1`, `NCK2`, `NCK3`, `NCK4`, `NCK5`, `NCK6`, `NCK7`, `NCK8`, `NCKF0`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-SUCCESSOR-DAG-AMENDMENT

- Kind: `context`; source: `successor-dag-amendment.md:1-1`; target: `node:NCK0`; text SHA-256: `9413cba2563db3ebfda5614b0ecd45ba6757581a4f7a20da7341ed2b3dc1d128`.

~~~~markdown
# Rev11 legacy-architecture reconciliation and successor charter pack
~~~~
