# Exact operative source-clause attachment — CCA1

Schema: 1. Node: `CCA1`. Clause count: 35. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-COMP-L662-D484DA845654

- Kind: `context`; source: `compiler-proposal.md:662-662`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `d484da845654c11ff55391c9fb769e6e24b252647a5f06264f41d3df2c7d79c8`.

~~~~markdown
**Suggested subblocks:**
~~~~

### SRC-COMP-L664-45CAEB179AE6

- Kind: `context`; source: `compiler-proposal.md:664-664`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `45caeb179ae6364ea45a86c3b7610ed14f0f19e7449ba5e6be63e69cd10e06ef`.

~~~~markdown
1. **CCA0-A — Current authority inventory.** Map every carrier/compiler/projection/semantic/module-assembly/style/host caller to one final owner; identify duplicate analyses and cross-framework option fields.
~~~~

### SRC-COMP-L665-AF4C9F734061

- Kind: `context`; source: `compiler-proposal.md:665-665`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `af4c9f7340613b936f621b6823d64b0263e1fe89c8f08e6bbaadd9235a98a64e`.

~~~~markdown
2. **CCA0-B — Policy and compatibility contract.** Define `CompilePolicy`, `DefaultCompilationContractId`, equivalence matrix, intentional-divergence records, and truthful unsupported `Optimized` capability.
~~~~

### SRC-COMP-L666-9B4826E04F4F

- Kind: `context`; source: `compiler-proposal.md:666-666`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `9b4826e04f4fb755c0183cb5cdda483a8dc9e07530fcc6b227eb1ff1dd81907c`.

~~~~markdown
3. **CCA0-C — Demand and admission contract.** Define the finite demand universe, reason edges, resumption basis, and the three admission tokens.
~~~~

### SRC-COMP-L667-27ED23C4140C

- Kind: `context`; source: `compiler-proposal.md:667-667`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `27ed23c4140c0ebbc90f4792bdbe97171484335336c5edea071870d20616cd41`.

~~~~markdown
4. **CCA0-D — Semantic authority contract.** Define per-framework authority namespaces and the `type_info` versus framework-interpretation boundary.
~~~~

### SRC-COMP-L668-16BECB3748A5

- Kind: `context`; source: `compiler-proposal.md:668-668`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `16becb3748a51732bf5b435ef55e4285d41c4ff50d3094e0291768dc52e9277a`.

~~~~markdown
5. **CCA0-E — Identity and representation laws.** Lock dense IDs, source anchors, optional lineage, lossless-sidecar exclusion, and optional physical materialization.
~~~~

### SRC-COMP-L669-7B4B7513DBF4

- Kind: `context`; source: `compiler-proposal.md:669-669`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `7b4b7513dbf4923ad482d591be4dc254142377e423abbb23a2b90191fa6ca41e`.

~~~~markdown
6. **CCA0-F — Architecture guards and exact-candidate review.** Add compile-time/dependency tests proving the generic compiler layer cannot import framework semantic types and the runtime compiler cannot own a second analyzer.
~~~~

### SRC-COMP-L671-B02F896E55EB

- Kind: `acceptance`; source: `compiler-proposal.md:671-671`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `b02f896e55eb5db520160599ba6e952d4b4e1611bf95c10167de247d59f18a3b`.

~~~~markdown
**Acceptance:**
~~~~

### SRC-COMP-L673-97A880BAA513

- Kind: `context`; source: `compiler-proposal.md:673-673`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `97a880baa5138a27ac5fd1c2eb4a5953f42eed5f7bcf0dcb3739a6f399edfe28`.

~~~~markdown
- every current method/caller has exactly one final authority;
~~~~

### SRC-COMP-L674-2A08833B4977

- Kind: `context`; source: `compiler-proposal.md:674-674`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `2a08833b49774c55d324e18e85838a212b30da7188fe3b8e91b82ffa1d3a94ab`.

~~~~markdown
- `Default` has a versioned behavior contract and can admit a planted cheap local alias-proven reactivity case without project I/O;
~~~~

### SRC-COMP-L675-FA8CF7E9F457

- Kind: `requirement`; source: `compiler-proposal.md:675-675`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `fa8cf7e9f45739b4cdb4d5aa0a3f9ecd76c842e7e742d47993a81a74b2819962`.

~~~~markdown
- `Optimized` is present only as truthful future capability;
~~~~

### SRC-COMP-L676-8858CD3E4067

- Kind: `context`; source: `compiler-proposal.md:676-676`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `8858cd3e4067d319d6536a8a6b3a8c0604dbd3220ed83f669d94c460b9ea5766`.

~~~~markdown
- no global framework semantic authority or type-info-as-framework-authority exists;
~~~~

### SRC-COMP-L677-B88742C7EC5A

- Kind: `requirement`; source: `compiler-proposal.md:677-677`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `b88742c7ec5a9f82406a918fc14c66807126de9c3a8a0a5e08150d0b450e287a`.

~~~~markdown
- J ownership is preserved;
~~~~

### SRC-COMP-L678-04664E2E6287

- Kind: `context`; source: `compiler-proposal.md:678-678`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `04664e2e628715d1ef5da9aa4f7779fbd0fb91a3469f4afb7b1ab2c7648cc7ce`.

~~~~markdown
- no compiler hot-path contract contains tooling recovery/trivia;
~~~~

### SRC-COMP-L679-16C3E1E67D6B

- Kind: `context`; source: `compiler-proposal.md:679-679`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `16c3e1e67d6b5906a758dd7aeb83836ac1a91f8189a54d4fd5bef1462a388280`.

~~~~markdown
- all negative architecture fixtures fail structurally.
~~~~

### SRC-COMP-L681-64EB39B7C160

- Kind: `forbidden`; source: `compiler-proposal.md:681-681`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `64eb39b7c1607cd638df5c5ab907a536e6b953a4207b9795dec7d85bd3b1168b`.

~~~~markdown
**Forbidden:** implementation of Vue/Svelte V2, CSS matcher changes, native preprocessors, project-wide optimization, dynamic plugin/ABI design, or preserving the combined authority behind aliases.
~~~~

### SRC-COMP-L683-401D67185A3B

- Kind: `deletion`; source: `compiler-proposal.md:683-683`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `401d67185a3baa6953c59ed75bce21868fd3b5084415d35044f56218baf5db8b`.

~~~~markdown
**Deletion/abort:** no broad deletion; reject/rescope if the authority split requires two active semantic answers or changes accepted compiler output in this lock block.
~~~~

### SRC-COMP-L685-F52D711103D5

- Kind: `context`; source: `compiler-proposal.md:685-685`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`.

~~~~markdown
---
~~~~

### SRC-COMP-L687-5758B6290C10

- Kind: `context`; source: `compiler-proposal.md:687-687`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `5758b6290c1084424ef5574846079108041298feac48f696b8ed9ea7694a29a1`.

~~~~markdown
## `CCA1.md` — Five-way compiler capability and registration cutover
~~~~

### SRC-COMP-L689-FA35E8182D56

- Kind: `context`; source: `compiler-proposal.md:689-689`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `fa35e8182d567873c9300c09b8d719d37c9155d61f061d37f18e7e4f6dfd08cc`.

~~~~markdown
**Intent:** atomically install the authority split with behavior-preserving adapters so C2 builds on the final seam rather than the combined carrier compiler abstraction.
~~~~

### SRC-COMP-L691-F1FDF61D18D5

- Kind: `forbidden`; source: `compiler-proposal.md:691-691`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `f1fdf61d18d5fd0c815efbbd6ce15cf2e2be536dc52a8300a5c66e67d4b6e5da`.

~~~~markdown
**Problem:** a tooling-only carrier must not pretend to compile, IDE projection must not be a runtime compiler product, generic sessions must not understand framework module topology, and framework/target dispatch must not occur dynamically per node.
~~~~

### SRC-COMP-L693-56832D9ECFE1

- Kind: `context`; source: `compiler-proposal.md:693-693`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`.

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L695-BE5E03BA1D72

- Kind: `context`; source: `compiler-proposal.md:695-695`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `be5e03ba1d72af74484e78c72092316ec958e3875337af11ed6887c2de028e5b`.

~~~~markdown
- add typed catalog/registry tables for:
~~~~

### SRC-COMP-L696-7DC7B3D27F7C

- Kind: `context`; source: `compiler-proposal.md:696-696`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `7dc7b3d27f7c9d77fa19ee75afa7216be28eb096ad3614157cb36318ae8a504e`.

~~~~markdown
- carrier frontends;
~~~~

### SRC-COMP-L697-4399E57362BC

- Kind: `context`; source: `compiler-proposal.md:697-697`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `4399e57362bc25aeaf2b1357aada5e6b660a982b5bde43eac715b4e79937137d`.

~~~~markdown
- framework semantic authorities/profiles;
~~~~

### SRC-COMP-L698-8A2101E53AE5

- Kind: `context`; source: `compiler-proposal.md:698-698`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `8a2101e53ae5fbd83496935e8851cbb60dddf86f4ba557bc955168c188fb95e8`.

~~~~markdown
- projection backends;
~~~~

### SRC-COMP-L699-CABDD2F8EBB3

- Kind: `context`; source: `compiler-proposal.md:699-699`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `cabdd2f8ebb3a1f1d89f5a9742ffa3dfd149d6bd0e506311fa464acbe194d863`.

~~~~markdown
- optional runtime compilers;
~~~~

### SRC-COMP-L700-7136CB285C10

- Kind: `context`; source: `compiler-proposal.md:700-700`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `7136cb285c10a3b341855b55069473062fe18935c23b7449d3771d366b27d58d`.

~~~~markdown
- framework-host integrations;
~~~~

### SRC-COMP-L701-1EE1A1CF673F

- Kind: `context`; source: `compiler-proposal.md:701-701`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `1ee1a1cf673fd6f76e6cc58c632a67bac280742a0a60e6fa455ee62bfc4247fe`.

~~~~markdown
- migrate Vue and Svelte through behavior-preserving adapters;
~~~~

### SRC-COMP-L702-5815030ACA3C

- Kind: `context`; source: `compiler-proposal.md:702-702`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `5815030aca3c31c5f48e5b1e01e015c1a13d7162dcfd80bf2c2814a040d1ce45`.

~~~~markdown
- keep target selection coarse and static inside each framework runtime compiler;
~~~~

### SRC-COMP-L703-929BA79510B9

- Kind: `context`; source: `compiler-proposal.md:703-703`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `929ba79510b9ef8126d61bc2f58dde6c4c2e4306397f8cb52d7732011a6d2037`.

~~~~markdown
- keep multi-target prerequisite sharing inside one framework compiler cell;
~~~~

### SRC-COMP-L704-7425D2E62B5D

- Kind: `context`; source: `compiler-proposal.md:704-704`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `7425d2e62b5d3f60dedf8095582d6cbdd5c13c849b7963940d99a762ff82d120`.

~~~~markdown
- retain one immutable catalog construction authority;
~~~~

### SRC-COMP-L705-5A1F47809A8E

- Kind: `deletion`; source: `compiler-proposal.md:705-705`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `5a1f47809a8ed8fe9d2e347a951a0ec5a5ec1ffb5adaf7a595a329adbf45b18a`.

~~~~markdown
- delete the combined carrier compiler trait/registry and cross-framework option bucket in the atomic cutover.
~~~~

### SRC-COMP-L707-12F1570AF52A

- Kind: `context`; source: `compiler-proposal.md:707-707`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `12f1570af52aa94ead12261fc374a500f6a811f5af132474be5115dce2fdb70b`.

~~~~markdown
**Suggested predecessor:** `CCA0`.
~~~~

### SRC-COMP-L709-D484DA845654

- Kind: `context`; source: `compiler-proposal.md:709-709`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `d484da845654c11ff55391c9fb769e6e24b252647a5f06264f41d3df2c7d79c8`.

~~~~markdown
**Suggested subblocks:**
~~~~
