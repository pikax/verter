# Exact operative source-clause attachment — NCF-RO-EXCESS

Schema: 1. Node: `NCF-RO-EXCESS`. Clause count: 5. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

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

### SRC-LEGACY-NCK-CUTOVER-001

- Kind: `requirement`; source: `legacy-arch-reconciliation.md:54-60`; target: `node:NCK6`; text SHA-256: `af3127aa56753f5301fdb526fd117ce34d418dbe848f4a089d11c6fc6ffb0b13`.

~~~~markdown
### NCK-CUTOVER-001 — Family/slice/profile authority transition

- Authority states are `External`, `ObserveNative`, `CertifiedNative`, and `Disabled`.
- Shadow observation is non-publishing.
- Promotion atomically suppresses external publication for the exact key before native publication becomes visible.
- Rollback names a prior accepted receipt.
- Targets: `NCK6`, every generated `NCF-*` node.
~~~~

### SRC-LEGACY-NCK-SHARED-PROOF-001

- Kind: `requirement`; source: `legacy-arch-reconciliation.md:38-44`; target: `node:NCK3`; text SHA-256: `0997e868a549534ac49f69642b9b8580eb49d6f50457b3b36efa4d28b84ab429`.

~~~~markdown
### NCK-SHARED-PROOF-001 — Diagnostics derive from shared proofs

- Assignability diagnostics consume `Relate` outcomes/proofs.
- Call/overload diagnostics consume `ResolveCall`/`ResolveOverloadSet` evidence.
- flow diagnostics consume accepted flow/return/completion/narrowing facts.
- contextual diagnostics consume `ContextualTypeAt` and shared relation evidence.
- Targets: `NCK3`, generated `NCF-*` nodes.
~~~~

### SRC-SUCCESSOR-DAG-AMENDMENT

- Kind: `context`; source: `successor-dag-amendment.md:1-1`; target: `node:NCK0`; text SHA-256: `9413cba2563db3ebfda5614b0ecd45ba6757581a4f7a20da7341ed2b3dc1d128`.

~~~~markdown
# Rev11 legacy-architecture reconciliation and successor charter pack
~~~~
