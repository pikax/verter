# Exact operative source-clause attachment — CLI3

Schema: 1. Node: `CLI3`. Clause count: 54. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-EXP-L1556-739A0A78C831

- Kind: `context`; source: `successor-expansion.md:1556-1556`; target: `node:CLI3`; text SHA-256: `739a0a78c831ae87d18b64e2ad4e41ac834d4b5aa05ba89570070c48c848317f`.

~~~~markdown
### `CLI3.md` — Aggregate `check` and transactional `fix` commands
~~~~

### SRC-EXP-L1558-FD0A8F2A8568

- Kind: `forbidden`; source: `successor-expansion.md:1558-1563`; target: `node:CLI3`; text SHA-256: `fd0a8f2a8568863270c0cbe0f9003634b0f018b7c59ed34be867390f9b53d8b2`.

~~~~markdown
**Intent:** compose typecheck, formatter-check, and lint into one non-mutating check while keeping transactional fixes limited to lint+format engines.
**Predecessors:** `CLI5`, `CLIF0`, `CLIL0`.
**Subblocks:** (1) non-mutating aggregate `check = typecheck + lint + fmt --check`; (2) explicit fix plan/order excluding typecheck mutation; (3) combine safe lint edits and formatting against one exact revision; (4) conflict/stale/rollback behavior; (5) atomic multi-file commit; (6) solely build, hash, sign, republish, and clean-install-test a new aggregate artifact through the platform/package matrix and process established by `CLI5`, with watch/report/performance tests and exact-candidate review.
**Acceptance:** the aggregate artifact records verified lineage to the accepted `CLI5` base artifact and packaging process but has its own identity/version reflecting the new command registry; `check` never writes and preserves each service’s result/provenance; `fix` previews and validates one transaction; partial failure leaves authored files unchanged.
**Forbidden:** implicit fixes during check, formatter-as-lint, arbitrary external edit auto-application, or per-file commits that leave a half-updated project.
**Deletion/abort:** `CLI3` is the sole aggregate republish/version owner; remove duplicate aggregate command adapters only after parity; abort aggregate mutation without recoverable atomicity or any attempt to present the changed binary as byte-identical to `CLI5`.
~~~~

### SRC-EXP-L1669-C23D0133ECFE

- Kind: `context`; source: `successor-expansion.md:1669-1669`; target: `contract:contracts/orchestration.md`; text SHA-256: `c23d0133ecfe03940f6d37decc04e5139e73e386336bec82f68c387bd92b0f1d`.

~~~~markdown
### 15.6 Non-active horizontal semantics ledger
~~~~

### SRC-EXP-L1671-281415DAE3E4

- Kind: `context`; source: `successor-expansion.md:1671-1671`; target: `contract:contracts/orchestration.md`; text SHA-256: `281415dae3e4d81ecd68f9846b5e1a4330e50919516ed505961cd1d5654f6d6f`.

~~~~markdown
After the architecture and one full new vertical are proven, prioritization should compare new framework work against horizontal semantics that benefit several verticals at once:
~~~~

### SRC-EXP-L1673-1541AD7A4CCE

- Kind: `context`; source: `successor-expansion.md:1673-1673`; target: `contract:contracts/orchestration.md`; text SHA-256: `1541ad7a4cce8fdd170b77ec1f6215cda11583ad9a67298a02120df0de1ddd7e`.

~~~~markdown
- CSS Modules, Sass/SCSS/Less semantic references, custom properties, and later evidence-gated utility-framework semantics;
~~~~

### SRC-EXP-L1674-EEA0DCA83B30

- Kind: `context`; source: `successor-expansion.md:1674-1674`; target: `contract:contracts/orchestration.md`; text SHA-256: `eea0dca83b30c883b81bf6a8637fdcea11ea36719ec23b0d7ec86585559d6533`.

~~~~markdown
- Vite/source-module facts such as aliases, assets, query imports, `import.meta.glob`, and environment typing, without bundler/HMR ownership;
~~~~

### SRC-EXP-L1675-D89AB7C50DC9

- Kind: `context`; source: `successor-expansion.md:1675-1675`; target: `contract:contracts/orchestration.md`; text SHA-256: `d89ab7c50dc98ce6f3e8f7ff9aa42823b6dedfbd05719868b19c2c8f341fe9c6`.

~~~~markdown
- JSON/JSONC/YAML and statically captured configuration projections, without executable configuration in Rust/WASM;
~~~~

### SRC-EXP-L1676-2A0D33DCADDB

- Kind: `context`; source: `successor-expansion.md:1676-1676`; target: `contract:contracts/orchestration.md`; text SHA-256: `2a0d33dcaddb84b6601f525fc063a94ac156549dfce8791403fc75b88b697502`.

~~~~markdown
- package exports/imports/workspaces and monorepo cross-package component relationships.
~~~~

### SRC-EXP-L1678-BF3A33A10B21

- Kind: `context`; source: `successor-expansion.md:1678-1678`; target: `contract:contracts/orchestration.md`; text SHA-256: `bf3a33a10b21a90d8ec8149d689b9607300c6b85162d6c1142b9b5aca549f2b0`.

~~~~markdown
These are portfolio records, not active DAG nodes or hidden vertical prerequisites. Each needs its own authority/reuse dossier and may be selected ahead of a lower-value framework when measured cross-vertical unlock exceeds the next vertical score.
~~~~

### SRC-EXP-L1680-673A040B209B

- Kind: `deletion`; source: `successor-expansion.md:1680-1680`; target: `contract:contracts/amendments.md`; text SHA-256: `673a040b209b6ed80f6fae71b9c85bfe0bbf57164cddd1d6604bc260110ef0f4`.

~~~~markdown
## 16. Superseded-proposal disposition
~~~~

### SRC-EXP-L1682-9146E12385C7

- Kind: `context`; source: `successor-expansion.md:1682-1682`; target: `contract:contracts/amendments.md`; text SHA-256: `9146e12385c7e28111d952c7985e6540e53f58b184e68817c5196eead95c5f85`.

~~~~markdown
Useful architecture from revision 3 is migrated rather than lost:
~~~~

### SRC-EXP-L1684-067F3DBC0974

- Kind: `context`; source: `successor-expansion.md:1684-1684`; target: `contract:contracts/amendments.md`; text SHA-256: `067f3dbc09741c766d0e4b0509718f5c433653c2cc2b68b8eba9f2c17e78755b`.

~~~~markdown
| Revision-3 area | Revision-4 disposition |
~~~~

### SRC-EXP-L1685-AC92D1B091B2

- Kind: `context`; source: `successor-expansion.md:1685-1685`; target: `contract:contracts/amendments.md`; text SHA-256: `ac92d1b091b25afced4265ab8cc2f40af0ad8ffa13d3ba560710a1caa14bfa06`.

~~~~markdown
|---|---|
~~~~

### SRC-EXP-L1686-7EA8347D5D4F

- Kind: `deletion`; source: `successor-expansion.md:1686-1686`; target: `contract:contracts/amendments.md`; text SHA-256: `7ea8347d5d4f19da9226035e01b3ccd50d36dcb81c64da0c7164b2be66f2355b`.

~~~~markdown
| Global `EXT0`, `TVG0`, `PJG0`, `X1` | Superseded by `UAK1`, independent terminals, and continuous soak suites |
~~~~

### SRC-EXP-L1687-797595A30D5A

- Kind: `requirement`; source: `successor-expansion.md:1687-1687`; target: `contract:contracts/amendments.md`; text SHA-256: `797595a30d5a24d59c78b272672e0b1c0dda07d34404e0bceb81f2b382d44468`.

~~~~markdown
| `KX` catalog rename | Replace with `VID0/CAT0`; preserve one snapshot/owner and avoid cosmetic rename |
~~~~

### SRC-EXP-L1688-5FE19351AD53

- Kind: `context`; source: `successor-expansion.md:1688-1688`; target: `contract:contracts/amendments.md`; text SHA-256: `5fe19351ad530b6c7f9f19bb85a60e069340daee3f6c1bbd1010e1fe53c9db69`.

~~~~markdown
| `CDX0` activation | Split and strengthen in `VID0`, `DEM0`, `EAK0`, `COX0` |
~~~~

### SRC-EXP-L1689-3C97CB0B0CC7

- Kind: `requirement`; source: `successor-expansion.md:1689-1689`; target: `contract:contracts/amendments.md`; text SHA-256: `3c97cb0b0cc72b24dc8faaf816c3ec083e0aea2434a4bb9c70ff77e29b8a59aa`.

~~~~markdown
| `EMB0` | Preserved as the sole embedded codec/authored-map-chain authority, consuming repaired `SourceUnitId` plus stable `AttachmentId`/`RegionId`; defines no independent embedded identity |
~~~~

### SRC-EXP-L1690-8D3A1B813597

- Kind: `context`; source: `successor-expansion.md:1690-1690`; target: `contract:contracts/amendments.md`; text SHA-256: `8d3a1b813597da4b5eb05e127fdf358675bc260ea14022ecc97fcd0d3dc30259`.

~~~~markdown
| `CMX0/CMX1` | Type-bearing envelope rejected; useful presentation compatibility moves to `TIF1` |
~~~~

### SRC-EXP-L1691-FF2C900A2196

- Kind: `context`; source: `successor-expansion.md:1691-1691`; target: `contract:contracts/amendments.md`; text SHA-256: `ff2c900a21966926125016b24de6c1c77913cdaf6191334432b1363eb2707b31`.

~~~~markdown
| `SGX0A/B` | Retained conceptually under `IDX0` with TypeInfo/index authority correction |
~~~~

### SRC-EXP-L1692-13931427FB7A

- Kind: `context`; source: `successor-expansion.md:1692-1692`; target: `contract:contracts/amendments.md`; text SHA-256: `13931427fb7ab79c764ba123d9b0a3604663210d992036958a01d93e4a284e6d`.

~~~~markdown
| `PJX0` | Projection admission/maps go to carrier owners + TCM1/TCM2; formatter/action maps stay distinct |
~~~~

### SRC-EXP-L1693-56EB01B7B786

- Kind: `context`; source: `successor-expansion.md:1693-1693`; target: `contract:contracts/amendments.md`; text SHA-256: `56eb01b7b786291b1a09c63f99de6e58be7ab1b2f16c6b6ed3e5a86aa925650b`.

~~~~markdown
| `ACT0` | Retained as authored transaction substrate consumed by `LRA0`, formatter, CLI fix/moves |
~~~~

### SRC-EXP-L1694-B8EDB2324B17

- Kind: `context`; source: `successor-expansion.md:1694-1694`; target: `contract:contracts/amendments.md`; text SHA-256: `b8edb2324b17d0fb0108891564b46f9b5b9f5324a0d8511a3e05844ec39f0bc9`.

~~~~markdown
| `OBS0/SEL0` | Retained as captured-input/selection concepts consumed by `CFG0/DEM0/CLI1` |
~~~~

### SRC-EXP-L1695-0471C5995C03

- Kind: `requirement`; source: `successor-expansion.md:1695-1695`; target: `contract:contracts/amendments.md`; text SHA-256: `0471c5995c0372ab5e2264c0ae0db36555ddb2b5dc293dfceb3f190dbdd42617`.

~~~~markdown
| `RFX0/AIX0` | Retained as downstream refactor/auto-import consumers of `IDX0`, TypeInfo, and exact actions |
~~~~

### SRC-EXP-L1696-2F6949C81A78

- Kind: `context`; source: `successor-expansion.md:1696-1696`; target: `contract:contracts/amendments.md`; text SHA-256: `2f6949c81a78cabddb2a8c513b867f36c1e77fd539449c0a05530eb6d2197674`.

~~~~markdown
| `FCX0` | Replaced by explicit optional `CarrierCompilerBackend` and per-vertical compiler disposition |
~~~~

### SRC-EXP-L1697-77E1521EB45A

- Kind: `requirement`; source: `successor-expansion.md:1697-1697`; target: `contract:contracts/amendments.md`; text SHA-256: `77e1521eb45ada607f1f2f12957117563ea4a4b0d6e9b87c007aa2067c39cf05`.

~~~~markdown
| `VWC*/SWC*` | Consumer-only framing replaced by `VCE0/SCE0` producer + consumer retrofits |
~~~~

### SRC-EXP-L1698-D62414024760

- Kind: `deletion`; source: `successor-expansion.md:1698-1698`; target: `contract:contracts/amendments.md`; text SHA-256: `d62414024760ace48bf2b5aa1dac9c68365e451fd41eb321d2d04761537d726a`.

~~~~markdown
| Fifteen full vertical charter families | Removed from active DAG; regenerated one at a time after `UKS0` |
~~~~

### SRC-EXP-L1699-22E7B37D77EE

- Kind: `context`; source: `successor-expansion.md:1699-1699`; target: `contract:contracts/amendments.md`; text SHA-256: `22e7b37d77ee6b934f6d198819601c94da720022e56c0d49a2f01741eb8d801c`.

~~~~markdown
| Formatter/lint/CLI mega-terminal chain | Replaced by independent `FMT4`, `LNT3` plus rule packs, base `CLI5`, and optional aggregate `CLI3` promotions |
~~~~

### SRC-EXP-L1701-6C6F10E2BF3A

- Kind: `requirement`; source: `successor-expansion.md:1701-1701`; target: `contract:contracts/amendments.md`; text SHA-256: `6c6f10e2bf3a38a4d133562b1ed5ce457a294bd1f34a876c9354e57178eed007`.

~~~~markdown
No revision-3 charter identifier is silently treated as accepted repository authority. `UAK0` must disposition exact current producers/consumers after Rev11 finishes.
~~~~

### SRC-EXP-L1714-52BA65A7F12E

- Kind: `context`; source: `successor-expansion.md:1714-1714`; target: `contract:contracts/reviews.md`; text SHA-256: `52ba65a7f12e385cbcc661da10e31ca32730bdd5e75fb6191c9ed3b81ed4ba20`.

~~~~markdown
## 18. Evidence, review questions, and candid risks
~~~~

### SRC-EXP-L1730-5A16DD9444F4

- Kind: `requirement`; source: `successor-expansion.md:1730-1730`; target: `contract:contracts/reviews.md`; text SHA-256: `5a16dd9444f4976f89e86924af975cbf366e5601f7c6db0a2339e0055292ea40`.

~~~~markdown
### 18.2 Questions every architecture review must attack
~~~~

### SRC-EXP-L1732-3C091B351A8F

- Kind: `context`; source: `successor-expansion.md:1732-1732`; target: `contract:contracts/reviews.md`; text SHA-256: `3c091b351a8f29d18e142126fba30c18cfb64105ad0a2b4f28a34ee4909c1f83`.

~~~~markdown
1. Does any “shared” abstraction contain a hidden Vue, React, HTML, or Next semantic branch?
~~~~

### SRC-EXP-L1733-813F3361E0A1

- Kind: `context`; source: `successor-expansion.md:1733-1733`; target: `contract:contracts/reviews.md`; text SHA-256: `813f3361e0a1a0a9badd29adf08d3b09145649cb5b2abb33b1b539a05140dc8a`.

~~~~markdown
2. Can a post-snapshot TypeScript fact influence the transform that created that snapshot?
~~~~

### SRC-EXP-L1734-2C7570691961

- Kind: `context`; source: `successor-expansion.md:1734-1734`; target: `contract:contracts/reviews.md`; text SHA-256: `2c7570691961e029676e9fdf55296f9a0ae21e59a3d96ca141fc62aede64e092`.

~~~~markdown
3. Can two parser, type, config, map, cache, index, or public-schema authorities answer the same question?
~~~~

### SRC-EXP-L1735-AF9C10BDBC54

- Kind: `context`; source: `successor-expansion.md:1735-1735`; target: `contract:contracts/reviews.md`; text SHA-256: `af9c10bdbc54e7dafca22922921f27361964d759574cbd7a20d3dc6db94ae135`.

~~~~markdown
4. Can a disabled or selected-but-unrequested profile do observable work?
~~~~

### SRC-EXP-L1736-63CE88414980

- Kind: `context`; source: `successor-expansion.md:1736-1736`; target: `contract:contracts/reviews.md`; text SHA-256: `63ce8841498055a88cce80f8fed9c3fd93262a84d917de15f06e19df6f2ea4a5`.

~~~~markdown
5. Can two framework releases collide in activation, caches, rules, diagnostics, or metadata?
~~~~

### SRC-EXP-L1737-70E973307C78

- Kind: `context`; source: `successor-expansion.md:1737-1737`; target: `contract:contracts/reviews.md`; text SHA-256: `70e973307c78a61c9a5d560cb804c329f135ec45ab4d448cc75274ceea65c605`.

~~~~markdown
6. Can an untagged offset cross Rust, FFI, LSP, CLI, or a cache boundary?
~~~~

### SRC-EXP-L1738-335359831466

- Kind: `context`; source: `successor-expansion.md:1738-1738`; target: `contract:contracts/reviews.md`; text SHA-256: `3353598314666ddc350b7a09dda36f45a481dab58670851450c0c5ca35fb0f3f`.

~~~~markdown
7. Can cancellation, overflow, ambiguity, or missing input become an admitted empty success?
~~~~

### SRC-EXP-L1739-F31A4D7EB034

- Kind: `context`; source: `successor-expansion.md:1739-1739`; target: `contract:contracts/reviews.md`; text SHA-256: `f31a4d7eb034fe6dfcad4bb8d4cda2d99495fde7f0d13cf60353d01348f2364e`.

~~~~markdown
8. Can a Custom Element claim confuse declaration, registration, scope, framework component identity, and runtime reachability?
~~~~

### SRC-EXP-L1740-7BF519FC65E2

- Kind: `context`; source: `successor-expansion.md:1740-1740`; target: `contract:contracts/reviews.md`; text SHA-256: `7bf519fc65e29dfe8e8402d5e70ec3c9708cc24c97154d1f44ad69a3d10e2482`.

~~~~markdown
9. Can a project profile select/create a TypeScript program or overwrite framework/TypeScript authority?
~~~~

### SRC-EXP-L1741-E909CDA6178F

- Kind: `requirement`; source: `successor-expansion.md:1741-1741`; target: `contract:contracts/reviews.md`; text SHA-256: `e909cda6178fdc2c98f6d00676b31d7913472baf05277fc34f7c6c4e48c9063f`.

~~~~markdown
10. Can a skill generate or implement work without an exact accepted manifest, charter, authority digest, and independent review?
~~~~

### SRC-EXP-L1755-5C57282D8064

- Kind: `context`; source: `successor-expansion.md:1755-1755`; target: `contract:contracts/amendments.md`; text SHA-256: `5c57282d80645e4513eaf83a450795e0b0fb9ac6aa70cb93c81a455b45e7df8f`.

~~~~markdown
## 19. Ratification recommendation
~~~~

### SRC-EXP-L1757-F1C972D7446F

- Kind: `deletion`; source: `successor-expansion.md:1757-1757`; target: `contract:contracts/amendments.md`; text SHA-256: `f1c972d7446f0e984c3694e73b8087f08fc71f8f11bc7509818eeb2398a87279`.

~~~~markdown
Do **not** ratify the superseded 251-charter program. Do **not** dispatch any successor implementation under the current freeze.
~~~~

### SRC-EXP-L1759-7A035429582A

- Kind: `context`; source: `successor-expansion.md:1759-1759`; target: `contract:contracts/amendments.md`; text SHA-256: `7a035429582abb18c1a40a3fa44cc6b3ddde8cd9dfa76a0e89d4839eef0902df`.

~~~~markdown
Recommended decision sequence:
~~~~

### SRC-EXP-L1761-6BBA45E37635

- Kind: `requirement`; source: `successor-expansion.md:1761-1761`; target: `contract:contracts/amendments.md`; text SHA-256: `6bba45e37635773bc4385cc6d4b98d36f02a1be2379f96fc6acc5b1f06b0356e`.

~~~~markdown
1. obtain an explicit maintainer decision lifting the freeze only for the identified amendment/repair scope;
~~~~

### SRC-EXP-L1762-8B50957DF2F6

- Kind: `context`; source: `successor-expansion.md:1762-1762`; target: `contract:contracts/amendments.md`; text SHA-256: `8b50957df2f6f63007c88ac81efd950621f280128c778535f3bc14711b92efef`.

~~~~markdown
2. ratify and land `AMD-TCM-PRECONDITIONS` under that authority;
~~~~

### SRC-EXP-L1763-B7A3BBA43597

- Kind: `context`; source: `successor-expansion.md:1763-1763`; target: `contract:contracts/amendments.md`; text SHA-256: `b7a3bba43597e96f56cf6513a7c12f8817a9f1bd500d1f0314c4566b5232f0ee`.

~~~~markdown
3. complete TCM0 remediation, observation/coordinate/ADR corrections, pre-L4 `SourceUnitId` repair, TCM1–TCM4 activation, activated-tree K3/L1/L2 revalidation, and Rev11 L4;
~~~~

### SRC-EXP-L1764-ED031959C8BC

- Kind: `requirement`; source: `successor-expansion.md:1764-1764`; target: `contract:contracts/amendments.md`; text SHA-256: `ed031959c8bc0fe9e34d56d9d6de609667386a3ea6919b9256f67d533318619d`.

~~~~markdown
4. obtain a separate post-L4 maintainer decision authorizing `BR0` creation/ratification/dispatch and the exact successor scope;
~~~~

### SRC-EXP-L1765-1821B2F492E1

- Kind: `requirement`; source: `successor-expansion.md:1765-1765`; target: `contract:contracts/amendments.md`; text SHA-256: `1821b2f492e148921c0f1fa2ef29cbd2f5634cc2d61916799759bd276d3affc7`.

~~~~markdown
5. validate both authorities in `successor-genesis.toml` and create `BR0` from that exact accepted tree;
~~~~

### SRC-EXP-L1766-123AE5B845C0

- Kind: `requirement`; source: `successor-expansion.md:1766-1766`; target: `contract:contracts/amendments.md`; text SHA-256: `123ae5b845c0b096d2bebacd23f968b21ace3bc5f50e50d9929e9266668e8ccf`.

~~~~markdown
6. close scoped kernel contracts independently; begin skills from `UAM0`, formatter from `FMK0`, lint from `LRA0/CFG0`, and base CLI from `PUB0`, while `UAK2` remains a read-only architecture convergence claim;
~~~~

### SRC-EXP-L1767-E5D2F91D6B3D

- Kind: `context`; source: `successor-expansion.md:1767-1767`; target: `contract:contracts/amendments.md`; text SHA-256: `e5d2f91d6b3dfde8d08d2fe2d2f18f4440d56d0bfe22926d312b709fadf83ea0`.

~~~~markdown
7. implement neutral HTML + Custom Elements as the first architecture project, with Vue/Svelte CE terminals independently releasable;
~~~~

### SRC-EXP-L1768-D2FD3ED4CC4F

- Kind: `requirement`; source: `successor-expansion.md:1768-1768`; target: `contract:contracts/amendments.md`; text SHA-256: `d2fd3ed4cc4f88a56127c516e7e27a5ed6e6277022dc1cad63e1d0b73ebb8f66`.

~~~~markdown
8. run the sequential representative proof slices and accept non-release `UKS0` only after all findings are closed;
~~~~

### SRC-EXP-L1769-16EEC534F496

- Kind: `requirement`; source: `successor-expansion.md:1769-1769`; target: `contract:contracts/amendments.md`; text SHA-256: `16eec534f496d4e2453ac14b53f6be2d311067ace7df2d5474b60d39a5b05cb9`.

~~~~markdown
9. select and execute one full vertical at a time—MDX first by current evidence; treat `MDXR0` only as evidence and promote the separately locked bounded React provider before React-specific MDX intelligence;
~~~~

### SRC-EXP-L1770-6CE7D646C41E

- Kind: `requirement`; source: `successor-expansion.md:1770-1770`; target: `contract:contracts/amendments.md`; text SHA-256: `6ce7d646c41e43176bee6ac5af344f0c8b11fcf2f06a717eb3118dbf3a0782e8`.

~~~~markdown
10. open project-profile implementation only after the language/framework substrate proves itself.
~~~~

### SRC-EXP-L1772-3794825DA331

- Kind: `context`; source: `successor-expansion.md:1772-1772`; target: `contract:contracts/amendments.md`; text SHA-256: `3794825da331a428fd05a0d653c341df9dd7056b16d5a2c8db23a3cc378d2304`.

~~~~markdown
This ordering maximizes longevity and performance because it fixes the expensive authority seams first, forces the architecture to survive genuinely different geometries, and keeps every later vertical removable, reviewable, and independently releasable.
~~~~
