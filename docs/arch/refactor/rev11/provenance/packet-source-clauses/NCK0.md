# Exact operative source-clause attachment — NCK0

Schema: 1. Node: `NCK0`. Clause count: 7. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-LEGACY-EXISTING-TYPEINFO-001

- Kind: `requirement`; source: `legacy-arch-reconciliation.md:207-209`; target: `node:E1`; text SHA-256: `80907890ae0cdb9344c99bc73988697f7cbf0e12a81bb168db6b3940ec07dc06`.

~~~~markdown
### EXISTING-TYPEINFO-001

TypeInfo semantic value/query/public graph contracts remain owned by E/TCM/UAO/PUB authority; the checker and language service do not create a second TypeInfo engine. Related source: `docs/arch/native-typeinfo-parity.md`, blob `2041fbfbd635086ec718a84e314a53f89d1566ac` and child plans.
~~~~

### SRC-LEGACY-NCK-AUTH-001

- Kind: `requirement`; source: `legacy-arch-reconciliation.md:15-21`; target: `node:NCK0`; text SHA-256: `654de84730f7d32a408348bb81ee224674ac622afb1ecc93af88a782992a7825`.

~~~~markdown
### NCK-AUTH-001 — One resolver, separate diagnostic authority

- Diagnostics may evaluate shared symbol/type/relation/call/flow/context/module/project facts.
- No checker-private resolver, type walker, relation engine, overload resolver, flow engine, symbol table, module resolver, or project graph exists.
- Semantic fact ownership remains with Rev11 authorities; diagnostic evaluation is owned by `expansion.native-checker`.
- Targets: `NCK0`, `NCK3`, every generated `NCF-*` charter.
- Source: `docs/arch/native-checker.md`, blob `3e96bf48ec481e97b9fd3067041e21099d194944`.
~~~~

### SRC-LEGACY-NCK-CERT-001

- Kind: `requirement`; source: `legacy-arch-reconciliation.md:46-52`; target: `node:NCK0`; text SHA-256: `fd3208a2cd7d054e945f223cf15017477ad01323bc7f6330a4c29f8924f68837`.

~~~~markdown
### NCK-CERT-001 — Oracle outside runtime

- TypeScript/tsgo is a pinned oracle and residual owner, never called by native query-time checker evaluation.
- Native resolver behavior is single-spec.
- Clear TypeScript bugs are represented by review-gated correction-overlay data, not a compatibility mode or cache-key dimension.
- Targets: `NCK0`, `NCK4`, generated `NCF-*` nodes.
- Related source: legacy TypeScript compatibility model.
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

### SRC-LEGACY-TRANSFER-2041FBFBD635

- Kind: `requirement`; source: `legacy-architecture-transfers.md:355-360`; target: `node:E1`; text SHA-256: `c0d46f5d4f4b7948eb0d04483d333de9bb4741019eab423d31ba0fad97877835`.

~~~~markdown
### LEGACY-TRANSFER-2041FBFBD635

- Original path: `docs/arch/native-typeinfo-parity.md`; Git blob: `2041fbfbd635086ec718a84e314a53f89d1566ac`; exact source SHA-256: `5039c1d88e71b4f2a9f5d4d52aac64ad4e535fa9e6c0fad3569427d8f5a736dc`.
- Exact retained source: `sources/legacy-architecture-transfers/native-typeinfo-parity.md`.
- Applicable authority: `E1`, `E2`, `E3`, `E4`, `TCM3`, `TCM4`, `TIF0`, `TIF1`, `UAO0`, `PUB0`, `NCK0`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
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
