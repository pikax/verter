# Exact operative source-clause attachment — SCP0

Schema: 1. Node: `SCP0`. Clause count: 17. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-COMP-L1424-9DA10B85DD60

- Kind: `context`; source: `compiler-proposal.md:1424-1424`; target: `contract:contracts/sizing.md`; text SHA-256: `9da10b85dd6018a1273dec50858434e2d74771a83cd67b336feb1e8cc92f6d80`.

~~~~markdown
# 9. Svelte Default compiler train
~~~~

### SRC-COMP-L1426-CC0C62C38EA6

- Kind: `requirement`; source: `compiler-proposal.md:1426-1426`; target: `node:SCP0`; text SHA-256: `cc0c62c38ea6eb81f17fa5ca075f4ef9b45aa41bad81b66461aac5c608445c88`.

~~~~markdown
## `SCP0.md` — Exact Svelte Default compiler lock
~~~~

### SRC-COMP-L1428-E3CC9136BA9B

- Kind: `requirement`; source: `compiler-proposal.md:1428-1428`; target: `node:SCP0`; text SHA-256: `e3cc9136ba9b37c65852ac72abce2daf41d312be560042bf10feed45b0f80284`.

~~~~markdown
**Intent:** freeze one exact Svelte semantic epoch, target contracts, style semantics, module compilation, corpora and performance gates.
~~~~

### SRC-COMP-L1430-35A936533B02

- Kind: `acceptance`; source: `compiler-proposal.md:1430-1430`; target: `node:SCP0`; text SHA-256: `35a936533b029e5d28605dacaffc5961f8c432a05238919a06f283fb271a97b7`.

~~~~markdown
**Problem:** the current experimental compiler cannot define its own acceptance after implementation, and default behavior must distinguish source-language semantics from output cosmetics.
~~~~

### SRC-COMP-L1432-56832D9ECFE1

- Kind: `context`; source: `compiler-proposal.md:1432-1432`; target: `node:SCP0`; text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`.

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L1434-EE05C498DD11

- Kind: `requirement`; source: `compiler-proposal.md:1434-1434`; target: `node:SCP0`; text SHA-256: `ee05c498dd118cd0b6d001560447be634bc173912ff0d0af50992488e3206567`.

~~~~markdown
- pin exact release/semantic epoch and `DefaultCompilationContractId` for client, server and module targets;
~~~~

### SRC-COMP-L1435-76A2DFC6065B

- Kind: `context`; source: `compiler-proposal.md:1435-1435`; target: `node:SCP0`; text SHA-256: `76a2dfc6065bd1cb1f1875a3b0e3184ebf686959774372e55807ffc44ffa7667`.

~~~~markdown
- lock runes/legacy, hydration, diagnostics, CSS pruning/scoping, maps, module surface, and unsupported cells;
~~~~

### SRC-COMP-L1436-8534E145BF11

- Kind: `context`; source: `compiler-proposal.md:1436-1436`; target: `node:SCP0`; text SHA-256: `8534e145bf11e50f16c8190fe7c6c52b98acb69989c68395c7c800d68078a057`.

~~~~markdown
- lock `Default` component-local facts and no workspace loading;
~~~~

### SRC-COMP-L1437-D66B66F946E3

- Kind: `context`; source: `compiler-proposal.md:1437-1437`; target: `node:SCP0`; text SHA-256: `d66b66f946e3214fc25d658e735f6609d83a5786c1f09402ca40704693a8bb1d`.

~~~~markdown
- lock official/reference differential, runtime/hydration validators and independent comparator use;
~~~~

### SRC-COMP-L1438-EE23035BDC7C

- Kind: `context`; source: `compiler-proposal.md:1438-1438`; target: `node:SCP0`; text SHA-256: `ee23035bdc7c93a30af09224f617a80c3d15ebbf8a9cb598ab1b5669df4839aa`.

~~~~markdown
- lock equivalent-work/RSS gates;
~~~~

### SRC-COMP-L1439-6E1EF5EDB6EC

- Kind: `deletion`; source: `compiler-proposal.md:1439-1439`; target: `node:SCP0`; text SHA-256: `6e1ef5edb6ec0156bbb94b3d25eb290aaef97d4e43234c1bd1f1b37bc04c46b5`.

~~~~markdown
- lock deletion scope of the experimental compiler.
~~~~

### SRC-COMP-L1441-5F57552C6CA5

- Kind: `context`; source: `compiler-proposal.md:1441-1441`; target: `node:SCP0`; text SHA-256: `5f57552c6ca5d7f886bf237de4d0b55692ed5ce22d73bc3acb98e3b5bc163688`.

~~~~markdown
**Suggested predecessor:** `CMP5`.
~~~~

### SRC-COMP-L1443-4E34E0B3173E

- Kind: `context`; source: `compiler-proposal.md:1443-1443`; target: `node:SCP0`; text SHA-256: `4e34e0b3173ee8b7450b2fe32e122544c7b41247c0d62c813ead74ec757040eb`.

~~~~markdown
**Suggested subblocks:** release/oracle dossier, behavior matrix, CSS/hydration/module corpus, current-baseline capture, performance lock, independent review.
~~~~

### SRC-COMP-L1445-17C7FD96FB55

- Kind: `acceptance`; source: `compiler-proposal.md:1445-1445`; target: `node:SCP0`; text SHA-256: `17c7fd96fb5554d23303cbab0d5a6ff6fc5341a052a158ec0c26ea9350bc3031`.

~~~~markdown
**Acceptance:** every target/style/diagnostic/map cell has a preimplementation pass rule; unsupported behavior is fail-closed and named.
~~~~

### SRC-COMP-L1447-65BD2D32E2FC

- Kind: `forbidden`; source: `compiler-proposal.md:1447-1447`; target: `node:SCP0`; text SHA-256: `65bd2d32e2fc0390cc3d99047b5d13e6fb48bec707eae26cfb3e0484d358ea2b`.

~~~~markdown
**Forbidden:** preserving an experimental representation solely because it exists, parser-speed-only goals, or criteria chosen from produced output.
~~~~

### SRC-COMP-L1449-D808961E0562

- Kind: `deletion`; source: `compiler-proposal.md:1449-1449`; target: `node:SCP0`; text SHA-256: `d808961e0562c51fa80e6cc0334e6775037a2de9951d13da83bc068fde68c9ec`.

~~~~markdown
**Deletion/abort:** no code; rescope rather than silently approximate unsupported semantics.
~~~~

### SRC-COMP-L1451-F52D711103D5

- Kind: `context`; source: `compiler-proposal.md:1451-1451`; target: `node:SCP0`; text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`.

~~~~markdown
---
~~~~
