# Exact operative source-clause attachment — NCK3

Schema: 1. Node: `NCK3`. Clause count: 10. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-LEGACY-EXISTING-FLOW-001

- Kind: `requirement`; source: `legacy-arch-reconciliation.md:203-205`; target: `node:D1`; text SHA-256: `bf72235cc22a1a167f2baedffc8f692206d6198a70dbf06a5411521b0339cea2`.

~~~~markdown
### EXISTING-FLOW-001

Accepted D-series flow/call/context/completion facts remain the sole program-analysis authority and are consumed by checker rules/regions. Related source: `docs/arch/native-flow-return.md`, blob `753967a660dc9a257d34bdb63f7ca3744b3731f8`.
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

### SRC-LEGACY-TRANSFER-141834CD8BE8

- Kind: `requirement`; source: `legacy-architecture-transfers.md:348-353`; target: `node:D8`; text SHA-256: `37d429ae2b6c71d9d169d3fc9731f99ec388a42ca74e1b6fb5c9199b22c56905`.

~~~~markdown
### LEGACY-TRANSFER-141834CD8BE8

- Original path: `docs/arch/native-typeinfo-parity-u2-reducers.md`; Git blob: `141834cd8be8474daced4b692c742419eeb19493`; exact source SHA-256: `84bc46262b51a34349bd9e0b3c27f85fc25fc48ed02ba5401fa4fc20b13df43f`.
- Exact retained source: `sources/legacy-architecture-transfers/native-typeinfo-parity-u2-reducers.md`.
- Applicable authority: `D8`, `TCM3`, `TIF0`, `NCK3`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-33FD0D49FD47

- Kind: `requirement`; source: `legacy-architecture-transfers.md:614-619`; target: `node:D7`; text SHA-256: `425b25a83d2f2499b01a4b0fb1e53c73d234dc001e60c9aa823f1f966dd34b7d`.

~~~~markdown
### LEGACY-TRANSFER-33FD0D49FD47

- Original path: `docs/arch/u6-flow-call-resolution-design.md`; Git blob: `33fd0d49fd4790412c8804ed91cc71f4548e22bb`; exact source SHA-256: `fda82ce9a689bd34eca561953e97bc3c3c0c90fee63648ba3f12035732f27ccb`.
- Exact retained source: `sources/legacy-architecture-transfers/u6-flow-call-resolution-design.md`.
- Applicable authority: `D7`, `D8`, `NCK3`.
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

### SRC-LEGACY-TRANSFER-753967A660DC

- Kind: `requirement`; source: `legacy-architecture-transfers.md:327-332`; target: `node:D1`; text SHA-256: `563f2c7b05a1ee5e3868384b855177b815cb9cb9100a9001d351929a1e1b2a8f`.

~~~~markdown
### LEGACY-TRANSFER-753967A660DC

- Original path: `docs/arch/native-flow-return.md`; Git blob: `753967a660dc9a257d34bdb63f7ca3744b3731f8`; exact source SHA-256: `0608c9d23ffd69b367b3223de8867b6e55c80d513600806d95bfe2c5bf6dfe9c`.
- Exact retained source: `sources/legacy-architecture-transfers/native-flow-return.md`.
- Applicable authority: `D1`, `D2`, `D3`, `D4`, `D5`, `D6`, `D7`, `D8`, `NCK1`, `NCK3`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-BCCE6B8ABA5C

- Kind: `requirement`; source: `legacy-architecture-transfers.md:607-612`; target: `node:D3`; text SHA-256: `498a17d14343335d54fb9bea631892a069e39a333983716e7beacb71507b9082`.

~~~~markdown
### LEGACY-TRANSFER-BCCE6B8ABA5C

- Original path: `docs/arch/u2-relation-infer-design.md`; Git blob: `bcce6b8aba5c439dab72e2f2398113b2af4129b3`; exact source SHA-256: `fbfe2da1023cb6f96e1b32b756e54d11231f6dde049dac4356dfc67f32520af6`.
- Exact retained source: `sources/legacy-architecture-transfers/u2-relation-infer-design.md`.
- Applicable authority: `D3`, `D8`, `NCK3`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-EC0ED2BF52B6

- Kind: `requirement`; source: `legacy-architecture-transfers.md:621-626`; target: `node:D8`; text SHA-256: `47b3d71871e4c3de2f96bb665ca5d3f0782ea1ddada700dd33645a3b715a77fc`.

~~~~markdown
### LEGACY-TRANSFER-EC0ED2BF52B6

- Original path: `docs/arch/u6-flow-return-gaps-and-target.md`; Git blob: `ec0ed2bf52b6afdd4089345641f41b77244e6109`; exact source SHA-256: `3065e49c7697acbf8adaceb080302513534eb2153aff5b9723aee8f7e9ff9e55`.
- Exact retained source: `sources/legacy-architecture-transfers/u6-flow-return-gaps-and-target.md`.
- Applicable authority: `D8`, `NCK3`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-SUCCESSOR-DAG-AMENDMENT

- Kind: `context`; source: `successor-dag-amendment.md:1-1`; target: `node:NCK0`; text SHA-256: `9413cba2563db3ebfda5614b0ecd45ba6757581a4f7a20da7341ed2b3dc1d128`.

~~~~markdown
# Rev11 legacy-architecture reconciliation and successor charter pack
~~~~
