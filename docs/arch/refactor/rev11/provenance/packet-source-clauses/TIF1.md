# Exact operative source-clause attachment — TIF1

Schema: 1. Node: `TIF1`. Clause count: 18. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

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

### SRC-LEGACY-EXISTING-TYPEINFO-001

- Kind: `requirement`; source: `legacy-arch-reconciliation.md:207-209`; target: `node:E1`; text SHA-256: `80907890ae0cdb9344c99bc73988697f7cbf0e12a81bb168db6b3940ec07dc06`.

~~~~markdown
### EXISTING-TYPEINFO-001

TypeInfo semantic value/query/public graph contracts remain owned by E/TCM/UAO/PUB authority; the checker and language service do not create a second TypeInfo engine. Related source: `docs/arch/native-typeinfo-parity.md`, blob `2041fbfbd635086ec718a84e314a53f89d1566ac` and child plans.
~~~~

### SRC-LEGACY-TRANSFER-04C59BB04C8E

- Kind: `requirement`; source: `legacy-architecture-transfers.md:530-535`; target: `node:CM1`; text SHA-256: `eb9e6cbd8cf511dd85ec354b0dfa242d7d2253310bfbefccb6f7dfb47e126ce0`.

~~~~markdown
### LEGACY-TRANSFER-04C59BB04C8E

- Original path: `docs/arch/scanners-replacement-typed-feature-facts.md`; Git blob: `04c59bb04c8ed361ab3203e62459be38ea10eeed`; exact source SHA-256: `3cca02f52a537f5567a13c2964ae8be4d40a5fa7388caa56220cc0038903e4a4`.
- Exact retained source: `sources/legacy-architecture-transfers/scanners-replacement-typed-feature-facts.md`.
- Applicable authority: `CM1`, `TIF1`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
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

### SRC-LEGACY-TRANSFER-2C07EE214251

- Kind: `requirement`; source: `legacy-architecture-transfers.md:488-493`; target: `node:CM1`; text SHA-256: `e07cd13a89367b660ede03174fc4ca7a3bf7bcdea265c2794dd5ab57bd04d0e7`.

~~~~markdown
### LEGACY-TRANSFER-2C07EE214251

- Original path: `docs/arch/scanners-replacement-capability-ledger.json`; Git blob: `2c07ee21425126369a9c0d592716d5b1b5ffa54f`; exact source SHA-256: `d27401a4f9b8040fe25b98d754f8685894a2927c6e55b9a38af2f1741612acf9`.
- Exact retained source: `sources/legacy-architecture-transfers/scanners-replacement-capability-ledger.json`.
- Applicable authority: `CM1`, `TIF1`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-3970AE56B544

- Kind: `requirement`; source: `legacy-architecture-transfers.md:523-528`; target: `node:CM1`; text SHA-256: `4139895ac8da06e621f2e0f50158457b46892c75927fcbbdb1d27074bfc2611b`.

~~~~markdown
### LEGACY-TRANSFER-3970AE56B544

- Original path: `docs/arch/scanners-replacement-type-authority.md`; Git blob: `3970ae56b544b0121b4dadd310860e76c00ea9fa`; exact source SHA-256: `b5ad534163f45ecaadde20b2b066d6d62d4eccf57a80b0f2411697770a75c67c`.
- Exact retained source: `sources/legacy-architecture-transfers/scanners-replacement-type-authority.md`.
- Applicable authority: `CM1`, `TIF1`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-39DC88E896A4

- Kind: `requirement`; source: `legacy-architecture-transfers.md:334-339`; target: `node:TCM3`; text SHA-256: `1ce1f4e3213eaf5e0a79e27de9a289863d057100ca4a431542527de72b9dc4f0`.

~~~~markdown
### LEGACY-TRANSFER-39DC88E896A4

- Original path: `docs/arch/native-typeinfo-parity-adapters-final-lift.md`; Git blob: `39dc88e896a462763b1957a68576046517f4f642`; exact source SHA-256: `d4d1092e46eb1f05224f00a96758458ec70860804285a5556cf4835317129ff9`.
- Exact retained source: `sources/legacy-architecture-transfers/native-typeinfo-parity-adapters-final-lift.md`.
- Applicable authority: `TCM3`, `TCM4`, `TIF0`, `TIF1`, `PUB0`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-3A55613B2843

- Kind: `requirement`; source: `legacy-architecture-transfers.md:208-213`; target: `node:TIF1`; text SHA-256: `1e0661a31d1f4ed46194d5c3e85fd973f517c903fa449772c0efa9ab977cbd58`.

~~~~markdown
### LEGACY-TRANSFER-3A55613B2843

- Original path: `docs/arch/future/vue-public-instance-generic-bound-recursion.md`; Git blob: `3a55613b28433503fc4f284bbdbd043b413112a0`; exact source SHA-256: `684abc504366ec8c3c6268c39f3b1a02f29d3698086b26a39f83ab4e52bd1232`.
- Exact retained source: `sources/legacy-architecture-transfers/future/vue-public-instance-generic-bound-recursion.md`.
- Applicable authority: `TIF1`, `NCF-JF-VUE`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-5CE6EF3AD364

- Kind: `requirement`; source: `legacy-architecture-transfers.md:579-584`; target: `node:TIF0`; text SHA-256: `876f31646a347665cadfe2d45b1eb20aa06b9e600b09a1fc4f1335f39f727b1c`.

~~~~markdown
### LEGACY-TRANSFER-5CE6EF3AD364

- Original path: `docs/arch/typed-ir-cutover/compat-heuristic-mapping.md`; Git blob: `5ce6ef3ad3646b58b83c924384e9e23959414163`; exact source SHA-256: `0821e46260a13eed33e7419b1dc7fd8c176b72bb151e871f3a4a9efd02532b03`.
- Exact retained source: `sources/legacy-architecture-transfers/typed-ir-cutover/compat-heuristic-mapping.md`.
- Applicable authority: `TIF0`, `TIF1`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-67AD64B1A90E

- Kind: `requirement`; source: `legacy-architecture-transfers.md:586-591`; target: `node:TIF1`; text SHA-256: `2fb81fd8108c3512c04a140ecb92c0a1c1908047dadb86b721e66634e01fc76f`.

~~~~markdown
### LEGACY-TRANSFER-67AD64B1A90E

- Original path: `docs/arch/typeinfo-row-registry-counts.md`; Git blob: `67ad64b1a90e7f9ef4de515cf3933ba85393b211`; exact source SHA-256: `da82271a68bc59e74e499a874fed0901126656d007489357234133125295ea73`.
- Exact retained source: `sources/legacy-architecture-transfers/typeinfo-row-registry-counts.md`.
- Applicable authority: `TIF1`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-78E3C9D90645

- Kind: `requirement`; source: `legacy-architecture-transfers.md:509-514`; target: `node:CM1`; text SHA-256: `646d6d7568132f09da789e93b8d603f49e3534182209d786c99e9997f3762478`.

~~~~markdown
### LEGACY-TRANSFER-78E3C9D90645

- Original path: `docs/arch/scanners-replacement-preprocessor-interim.md`; Git blob: `78e3c9d90645a5be2472af2e13e3c0d439f045c8`; exact source SHA-256: `55e5063bef00253251f844e4554773a967f659f7505b1f67f588a0f650b9f3a1`.
- Exact retained source: `sources/legacy-architecture-transfers/scanners-replacement-preprocessor-interim.md`.
- Applicable authority: `CM1`, `TIF1`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-7ABDCA82CF3E

- Kind: `requirement`; source: `legacy-architecture-transfers.md:201-206`; target: `node:TIF1`; text SHA-256: `aa8d18d218cd8f8560e975f35fd04ca2bfbc9052c8805d6a559717a0cf75c9ff`.

~~~~markdown
### LEGACY-TRANSFER-7ABDCA82CF3E

- Original path: `docs/arch/future/unplugin-macro-type-hydration-speed-path.md`; Git blob: `7abdca82cf3e0219d391148303983e12ec30634a`; exact source SHA-256: `684d23a528099ebf0257aae560789bac1ddca1b91109419e96477067aec89a08`.
- Exact retained source: `sources/legacy-architecture-transfers/future/unplugin-macro-type-hydration-speed-path.md`.
- Applicable authority: `TIF1`, `CM1`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-7F86EABE42F8

- Kind: `requirement`; source: `legacy-architecture-transfers.md:516-521`; target: `node:CM1`; text SHA-256: `535207925b5b845e9f9327de86c3ec8fab440e7b3e07b73d43cce84e8ff0b62f`.

~~~~markdown
### LEGACY-TRANSFER-7F86EABE42F8

- Original path: `docs/arch/scanners-replacement-public-contract.md`; Git blob: `7f86eabe42f8c061f7372448e6f5a19426d800c4`; exact source SHA-256: `9721102ea74e9e643a5ed8fb5f70afefa236c2f82e1fd33508b578bbfb30cafb`.
- Exact retained source: `sources/legacy-architecture-transfers/scanners-replacement-public-contract.md`.
- Applicable authority: `CM1`, `TIF1`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-B3203BB016F3

- Kind: `requirement`; source: `legacy-architecture-transfers.md:495-500`; target: `node:CM1`; text SHA-256: `7f42d87e7dc3ba72f7c1ad5b4dd9a74f3a554b6de1405bf1deeedd383fd6e173`.

~~~~markdown
### LEGACY-TRANSFER-B3203BB016F3

- Original path: `docs/arch/scanners-replacement-compat-descriptor.md`; Git blob: `b3203bb016f3e5fb4d33acc361a1cd990bba3b9e`; exact source SHA-256: `7e8f70693f4e9d185df467b0f055f36b751d88f12a67ac90fc90a02aa7dd4ac6`.
- Exact retained source: `sources/legacy-architecture-transfers/scanners-replacement-compat-descriptor.md`.
- Applicable authority: `CM1`, `TIF1`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-C92C98813AE4

- Kind: `requirement`; source: `legacy-architecture-transfers.md:82-87`; target: `node:TIF1`; text SHA-256: `104c5cd01f28ca1b72f4d7c8062e96a2f386e138f7cd0142b50266c01c5ed4e1`.

~~~~markdown
### LEGACY-TRANSFER-C92C98813AE4

- Original path: `docs/arch/future/global-components-typing-and-fail-closed-diagnostics.md`; Git blob: `c92c98813ae4ec6b655add8e0b3ea7467eefb048`; exact source SHA-256: `c48fef63c27802968bcc5fc9a4570b15b6d5cb30eeb57b6e31474952c8008698`.
- Exact retained source: `sources/legacy-architecture-transfers/future/global-components-typing-and-fail-closed-diagnostics.md`.
- Applicable authority: `TIF1`, `LSO5`, `NCF-JF-VUE`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-D02185D5F77B

- Kind: `requirement`; source: `legacy-architecture-transfers.md:502-507`; target: `node:CM1`; text SHA-256: `c6a9d5650baf30164f4a52ed76e5e5151907112d4e00e21321ccc132d69509ba`.

~~~~markdown
### LEGACY-TRANSFER-D02185D5F77B

- Original path: `docs/arch/scanners-replacement-content-handoff.md`; Git blob: `d02185d5f77b21675d1eea6cc562aea4230e187f`; exact source SHA-256: `b2dbbaff4908a48ec75e1fb82c3708f9bd5586eb02bf096796bf03e105b35ccd`.
- Exact retained source: `sources/legacy-architecture-transfers/scanners-replacement-content-handoff.md`.
- Applicable authority: `CM1`, `TIF1`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-DDDCBF845DF7

- Kind: `requirement`; source: `legacy-architecture-transfers.md:481-486`; target: `node:B1`; text SHA-256: `7a4a040e39989bfa16e9a7097f0892bca777331be76e36401f198f7d7dba9185`.

~~~~markdown
### LEGACY-TRANSFER-DDDCBF845DF7

- Original path: `docs/arch/scanners-replacement-b1-capabilities.md`; Git blob: `dddcbf845df7eb8940f47771d2b6f1a62171e161`; exact source SHA-256: `cd572ab6e6706a0e4ce11ae712d5254187f984b8c2cca01e652bc05ce41fc967`.
- Exact retained source: `sources/legacy-architecture-transfers/scanners-replacement-b1-capabilities.md`.
- Applicable authority: `B1`, `CM1`, `TIF1`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~
