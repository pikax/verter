# Exact operative source-clause attachment — VCP3

Schema: 1. Node: `VCP3`. Clause count: 16. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-COMP-L1287-65234AAE74C1

- Kind: `context`; source: `compiler-proposal.md:1287-1287`; target: `node:VCP3`; text SHA-256: `65234aae74c1f02c18abe5fb7d837212615688828dd39149c8672b51d5b136de`.

~~~~markdown
## `VCP3.md` — Vue VDOM Default compiler
~~~~

### SRC-COMP-L1289-7BFAEEA664C5

- Kind: `context`; source: `compiler-proposal.md:1289-1289`; target: `node:VCP3`; text SHA-256: `7bfaeea664c5910d5016cc9aeaaf52df70e65f9337f642e5262fc2bb2e6de525`.

~~~~markdown
**Intent:** implement the primary Vue runtime target on the new semantic and structural authorities.
~~~~

### SRC-COMP-L1291-A5E1EA79D8EC

- Kind: `context`; source: `compiler-proposal.md:1291-1291`; target: `node:VCP3`; text SHA-256: `a5e1ea79d8ec220e5dcee47949e62baa0c23287ceb394ba55d125bbf38f2523e`.

~~~~markdown
**Problem:** target code can rediscover semantic facts, dynamically dispatch per node, allocate whole target trees, and mix maps/emission decisions.
~~~~

### SRC-COMP-L1293-56832D9ECFE1

- Kind: `context`; source: `compiler-proposal.md:1293-1293`; target: `node:VCP3`; text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`.

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L1295-8A8747FA32DE

- Kind: `context`; source: `compiler-proposal.md:1295-1295`; target: `node:VCP3`; text SHA-256: `8a8747fa32de4426c77910aebc2d52568452887be86856f277cfe44cf32798ad`.

~~~~markdown
- monomorphic Vue+VDOM executor;
~~~~

### SRC-COMP-L1296-3781BC6F95B1

- Kind: `context`; source: `compiler-proposal.md:1296-1296`; target: `node:VCP3`; text SHA-256: `3781bc6f95b128a8d169be508ec934592b58a312f814d499ad4e4b4d4d2a9562`.

~~~~markdown
- sparse target plan for patch classes, dynamic props, hoists, cache slots, helpers and target diagnostics;
~~~~

### SRC-COMP-L1297-1DDC4230E787

- Kind: `context`; source: `compiler-proposal.md:1297-1297`; target: `node:VCP3`; text SHA-256: `1ddc4230e7879095cf7a8c3be5eb20c2c7fdf3a85dc57433d6bbbbd3bab1eae6`.

~~~~markdown
- use `Default` canonical component-local facts, including stronger cheap alias-proven reactivity where safe;
~~~~

### SRC-COMP-L1298-5B5454FC5B2C

- Kind: `context`; source: `compiler-proposal.md:1298-1298`; target: `node:VCP3`; text SHA-256: `5b5454fc5b2c7bfedfb93566ea1dfb1f9fbf9edb77df14a21f77d4ef73a0eb88`.

~~~~markdown
- no SSR/Vapor/effect/style-query work;
~~~~

### SRC-COMP-L1299-F425A5E5BC0D

- Kind: `context`; source: `compiler-proposal.md:1299-1299`; target: `node:VCP3`; text SHA-256: `f425a5e5bc0d67a015d1176b79006dab775928ed823e694b170fcc7a6f5e1565`.

~~~~markdown
- segmented emission and map/no-map specialization;
~~~~

### SRC-COMP-L1300-88BCDDD2E573

- Kind: `requirement`; source: `compiler-proposal.md:1300-1300`; target: `node:VCP3`; text SHA-256: `88bcddd2e573c5c6e15bd00356d30afe514b7f78b46b55326acb78357f9960a7`.

~~~~markdown
- exact runtime/module/map contract from `VCP0`.
~~~~

### SRC-COMP-L1302-1E4BF1D3A01D

- Kind: `context`; source: `compiler-proposal.md:1302-1302`; target: `node:VCP3`; text SHA-256: `1e4bf1d3a01d79737e78ee77dc804362fd1e31beac1e93e294a66d41aa363639`.

~~~~markdown
**Suggested predecessors:** `VCP2`, `VST0`.
~~~~

### SRC-COMP-L1304-0E0870DAEBE8

- Kind: `requirement`; source: `compiler-proposal.md:1304-1304`; target: `node:VCP3`; text SHA-256: `0e0870daebe8a4dafc9542915950de07280909350f67fc167bdc465f2db5cda6`.

~~~~markdown
**Suggested subblocks:** element/text/interpolation, directives/bindings/events, components/slots/control flow, patch/hoist/cache planning, emission/maps, conformance/performance closure.
~~~~

### SRC-COMP-L1306-B946285742B5

- Kind: `acceptance`; source: `compiler-proposal.md:1306-1306`; target: `node:VCP3`; text SHA-256: `b946285742b5869e0c35dd996f882704f055cab743b9ef53ad49dce075ba0433`.

~~~~markdown
**Acceptance:** all locked VDOM cells pass runtime and map validators; no compiler-local semantic rederivation; no per-node dynamic dispatch; VDOM/no-map work ledger contains zero SSR/Vapor/VST1 work.
~~~~

### SRC-COMP-L1308-09E07AE2894F

- Kind: `forbidden`; source: `compiler-proposal.md:1308-1308`; target: `node:VCP3`; text SHA-256: `09e07ae2894fd4ec4d052e119d715183b79fabc1c512d45019a6085bab985565`.

~~~~markdown
**Forbidden:** cloning the structural tree into a full VDOM AST without evidence, output-only tests, or delaying known correctness defects to later targets.
~~~~

### SRC-COMP-L1310-966C4B1BA5D5

- Kind: `deletion`; source: `compiler-proposal.md:1310-1310`; target: `node:VCP3`; text SHA-256: `966c4b1ba5d5560bb301cb2cab2e4e4d29afc598354d569e3aa818f458a4758e`.

~~~~markdown
**Deletion/abort:** delete the old VDOM path atomically only at `VCP6`/`VCP7`; retain adapters until then.
~~~~

### SRC-COMP-L1312-F52D711103D5

- Kind: `context`; source: `compiler-proposal.md:1312-1312`; target: `node:VCP3`; text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`.

~~~~markdown
---
~~~~
