# Exact operative source-clause attachment — CCA2

Schema: 1. Node: `CCA2`. Clause count: 32. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-COMP-L711-85E3399AC19F

- Kind: `context`; source: `compiler-proposal.md:711-711`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `85e3399ac19fa4a37ba71b1fd7b8d62b1a8a2e9a844d01dd52598c204a24c061`.

~~~~markdown
1. **CCA1-A — Type and registry skeleton.** Land typed traits/tables and compile-time capability truth with no route cutover.
~~~~

### SRC-COMP-L712-222AEC5287B1

- Kind: `context`; source: `compiler-proposal.md:712-712`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `222aec5287b1571a6f55505e9c10ea848045334051c2aa157bd2e60d83e2d6ec`.

~~~~markdown
2. **CCA1-B — Frontend and semantic migration.** Move parse/source-unit/fact routes while preserving bytes, recovery, identities, and caches.
~~~~

### SRC-COMP-L713-425D12D1034D

- Kind: `context`; source: `compiler-proposal.md:713-713`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `425d12d1034dd8593764583486a8d5542fbe590cdf8d282f3ee483d30483e987`.

~~~~markdown
3. **CCA1-C — Projection migration.** Move IDE/checkable projection into `ProjectionBackend`; prove no runtime compiler dependency.
~~~~

### SRC-COMP-L714-F9C4CE521860

- Kind: `requirement`; source: `compiler-proposal.md:714-714`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `f9c4ce52186022d297760e52b000c814ae74d5d218fd6b19e9d6ec00b9737db9`.

~~~~markdown
4. **CCA1-D — Runtime compiler migration.** Move Vue/Svelte compile routes and owner-local typed requests; preserve direct/prepared/managed behavior.
~~~~

### SRC-COMP-L715-4D24476EA00F

- Kind: `context`; source: `compiler-proposal.md:715-715`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `4d24476ea00f773e356b8622310f7c2ccc1e9c7624f577b49713dc67e9d6a94e`.

~~~~markdown
5. **CCA1-E — Host-integration migration.** Move existing framework-host behavior behind the explicit integration authority without changing semantics.
~~~~

### SRC-COMP-L716-C2B8A2397FA6

- Kind: `deletion`; source: `compiler-proposal.md:716-716`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `c2b8a2397fa625cbc4cab9b6583c7c01045f1ef8509a02da759350955d1bbe1f`.

~~~~markdown
6. **CCA1-F — Atomic deletion and parity.** Delete combined traits/registries/options and generated guards only after all consumers move.
~~~~

### SRC-COMP-L718-B02F896E55EB

- Kind: `acceptance`; source: `compiler-proposal.md:718-718`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `b02f896e55eb5db520160599ba6e952d4b4e1611bf95c10167de247d59f18a3b`.

~~~~markdown
**Acceptance:**
~~~~

### SRC-COMP-L720-0EB00A06EFC7

- Kind: `requirement`; source: `compiler-proposal.md:720-720`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `0eb00a06efc76adb33d1bbcd983b314332e171dd9d5333c8eadcea32997fc768`.

~~~~markdown
- tooling-only test carriers compile without runtime-backend stubs;
~~~~

### SRC-COMP-L721-91CD57F30B0A

- Kind: `context`; source: `compiler-proposal.md:721-721`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `91cd57f30b0ac22ce9807a2a80e79150b566d7ef64f7c7bf05683c73142513f3`.

~~~~markdown
- Vue/Svelte parse, projection, compile, maps, cache, diagnostics, and public outputs remain equivalent on pinned corpora;
~~~~

### SRC-COMP-L722-E72AEE902592

- Kind: `context`; source: `compiler-proposal.md:722-722`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `e72aee902592739effc5f14d240a7c614aed295736a6eddbada1805908b2c944`.

~~~~markdown
- one framework can request multiple targets while sharing prerequisites;
~~~~

### SRC-COMP-L723-86344A77B3F8

- Kind: `context`; source: `compiler-proposal.md:723-723`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `86344a77b3f8d21f3aaddee0a8939e1c85d2fb218006da985f0c891885c3f0df`.

~~~~markdown
- target dispatch occurs outside per-node loops;
~~~~

### SRC-COMP-L724-4FA786D70ED1

- Kind: `context`; source: `compiler-proposal.md:724-724`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `4fa786d70ed1ba5e7484ffad81a500df994c49d836f079b9c354cc73ccfd1626`.

~~~~markdown
- zero combined-registry/combined-options consumers remain.
~~~~

### SRC-COMP-L726-48E056C24C2E

- Kind: `forbidden`; source: `compiler-proposal.md:726-726`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `48e056c24c2e3d050ee2f8c13c99ca22cffdacb022091ebc481bd342271b61e2`.

~~~~markdown
**Forbidden:** dual-running registries, erased `Any` artifacts, one backend per target that duplicates framework prerequisites, public compatibility aliases that remain authorities, or framework branches in the generic session.
~~~~

### SRC-COMP-L728-E7A8460E45EC

- Kind: `deletion`; source: `compiler-proposal.md:728-728`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `e7a8460e45ec0609cf22bc9c03e66cb94850232b160b0800676ae8fa39a568e6`.

~~~~markdown
**Deletion/abort:** delete the old combined trait/registry and mixed option types atomically; abort on unexplained output/map/performance divergence.
~~~~

### SRC-COMP-L730-F52D711103D5

- Kind: `context`; source: `compiler-proposal.md:730-730`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`.

~~~~markdown
---
~~~~

### SRC-COMP-L732-274AD70ED8D1

- Kind: `context`; source: `compiler-proposal.md:732-732`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `274ad70ed8d1efb1c84b9ca1b8bedfa1797d5b7543836c93b0fb605781d30360`.

~~~~markdown
## `CCA2.md` — Compiler artifact, assembly, style-stage, and host boundary
~~~~

### SRC-COMP-L734-43DCCB8EC555

- Kind: `context`; source: `compiler-proposal.md:734-734`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `43dccb8ec5554ce3ed2bd2aa78150fb13b210c2ef24837c1ab63453bfad9e792`.

~~~~markdown
**Intent:** establish the stable staged-compile outputs consumed by C2 and later compiler implementations without implementing Compiler V2.
~~~~

### SRC-COMP-L736-4014C3AB804C

- Kind: `context`; source: `compiler-proposal.md:736-736`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `4014c3ab804caf27953a87f4138666e3a1d0683ee5624b84e1617a60bc1caaea`.

~~~~markdown
**Problem:** SFC-shaped generic outputs, session-owned framework assembly, opaque CSS preprocessing callbacks, and underspecified custom-block records would freeze the wrong long-term boundary.
~~~~

### SRC-COMP-L738-56832D9ECFE1

- Kind: `context`; source: `compiler-proposal.md:738-738`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`.

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L740-9FEEDF596D6E

- Kind: `context`; source: `compiler-proposal.md:740-740`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `9feedf596d6e213366649d9846c688e2de92ebd681c8fbc9efd47ae7ab44a40f`.

~~~~markdown
- define `CompileArtifactSet` with root artifact, artifacts, qualified maps, provenance, and typed relations;
~~~~

### SRC-COMP-L741-877AFD52BB6F

- Kind: `requirement`; source: `compiler-proposal.md:741-741`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `877afd52bb6fe8acd932941559a65565232d8fbf39a3574848f71ae14014dc56`.

~~~~markdown
- keep framework-local strongly typed results internally and convert only at the shared product boundary;
~~~~

### SRC-COMP-L742-FAB402C23B9B

- Kind: `context`; source: `compiler-proposal.md:742-742`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `fab402c23b9b5cda3b8b65b97e17589a78e1b37d9015c4c5aac6113e30d38154`.

~~~~markdown
- make framework compilers own semantic module assembly;
~~~~

### SRC-COMP-L743-2569DC35A4AD

- Kind: `context`; source: `compiler-proposal.md:743-743`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `2569dc35a4add6dbbd1e6c70dad8b9007d5724c7818337d0feb5c60d9e94760b`.

~~~~markdown
- make `FrameworkHostIntegrationBackend` own bundler/HMR/virtual-module/manifest policy;
~~~~

### SRC-COMP-L744-297D7AE1D3EC

- Kind: `context`; source: `compiler-proposal.md:744-744`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `297d7ae1d3ec35e072792d3afdcf72692295957805dca71961e36810ba482287`.

~~~~markdown
- define a stage-qualified external style continuation compatible with the J-owned boundary; do not create a second preprocessor authority;
~~~~

### SRC-COMP-L745-085AB67D4E0C

- Kind: `requirement`; source: `compiler-proposal.md:745-745`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `085ab67d4e0c9ce3039056978dff9259b7c80da08c4da22aa62dda12b7f778c7`.

~~~~markdown
- preserve custom blocks through a source-backed `CustomBlockDescriptor` separating role/tag name from `lang`, source reference, attributes, order, region, and content availability;
~~~~

### SRC-COMP-L746-50F0416AF195

- Kind: `context`; source: `compiler-proposal.md:746-746`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `50f0416af1950f7ab2039d63897a9226b2d07e3e8baee077af869bdf8a71ad52`.

~~~~markdown
- unknown custom blocks remain opaque and perform zero semantic/runtime work by default;
~~~~

### SRC-COMP-L747-E5E49A00E348

- Kind: `context`; source: `compiler-proposal.md:747-747`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `e5e49a00e348627bdbd6a12f136132e74eeb6faed5706030c744bbfcc2e6fd37`.

~~~~markdown
- keep OXC internal and stable artifacts text/bytes based;
~~~~

### SRC-COMP-L748-A797686DE1F4

- Kind: `deletion`; source: `compiler-proposal.md:748-748`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `a797686de1f424178d7ac582084b94e950bdb01b7a9914a09cf0a251670250ad`.

~~~~markdown
- install temporary behavior-preserving adapters for current runtime outputs with explicit deletion ownership.
~~~~

### SRC-COMP-L750-747E4FACD742

- Kind: `context`; source: `compiler-proposal.md:750-750`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `747e4facd7428598907cf6ce2ade650a3a71cd381a1a702729a654a62cc54e9c`.

~~~~markdown
**Suggested predecessor:** `CCA1`.
~~~~

### SRC-COMP-L752-D484DA845654

- Kind: `context`; source: `compiler-proposal.md:752-752`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `d484da845654c11ff55391c9fb769e6e24b252647a5f06264f41d3df2c7d79c8`.

~~~~markdown
**Suggested subblocks:**
~~~~

### SRC-COMP-L754-0169B86EC62F

- Kind: `context`; source: `compiler-proposal.md:754-754`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `0169b86ec62f1889c462bff000682e94a4ff71ebf5107be7c036150623673ec6`.

~~~~markdown
1. **CCA2-A — Artifact schema and map qualification.** Define artifact IDs, roles, languages, relations, map families, provenance, and terminal serialization.
~~~~

### SRC-COMP-L755-6A4BCA657B8F

- Kind: `context`; source: `compiler-proposal.md:755-755`; target: `contract:contracts/compiler-architecture.md`; text SHA-256: `6a4bca657b8fff5d3025d7ed0cd0f4288f6f0ce8580d1d9cea5bdfdbeec5e99e`.

~~~~markdown
2. **CCA2-B — Framework assembly boundary.** Move or wrap Vue/Svelte semantic module assembly behind the runtime compiler authority; keep behavior unchanged.
~~~~
