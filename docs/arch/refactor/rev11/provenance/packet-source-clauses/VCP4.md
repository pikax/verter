# Exact operative source-clause attachment — VCP4

Schema: 1. Node: `VCP4`. Clause count: 15. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-COMP-L1314-F937B7DDA893

- Kind: `context`; source: `compiler-proposal.md:1314-1314`; target: `node:VCP4`; text SHA-256: `f937b7dda893f3ae26d6e966fa734bb2f32e345220b0b3a8bbeb7cecfee0e767`.

~~~~markdown
## `VCP4.md` — Vue SSR Default compiler
~~~~

### SRC-COMP-L1316-E21DBCA2CACE

- Kind: `context`; source: `compiler-proposal.md:1316-1316`; target: `node:VCP4`; text SHA-256: `e21dbca2cace388a608b7d7d093f31384224decba9aef15d722c14c9c198e81e`.

~~~~markdown
**Intent:** implement server compilation as a distinct target that shares prerequisites but performs zero client-effect planning.
~~~~

### SRC-COMP-L1318-D8663C045D34

- Kind: `context`; source: `compiler-proposal.md:1318-1318`; target: `node:VCP4`; text SHA-256: `d8663c045d34a90625557d6d17fcb3de3c6dc07fcc168a47b01ce48e4f1a4403`.

~~~~markdown
**Problem:** server targets can accidentally inherit client/Vapor structures and unnecessary target materialization.
~~~~

### SRC-COMP-L1320-56832D9ECFE1

- Kind: `context`; source: `compiler-proposal.md:1320-1320`; target: `node:VCP4`; text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`.

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L1322-65F60B27FE50

- Kind: `context`; source: `compiler-proposal.md:1322-1322`; target: `node:VCP4`; text SHA-256: `65f60b27fe50aae74e14fa0de85a071e743b890fdd488d7a85e4627666efd6d5`.

~~~~markdown
- monomorphic Vue+SSR executor;
~~~~

### SRC-COMP-L1323-284B6038B5D2

- Kind: `context`; source: `compiler-proposal.md:1323-1323`; target: `node:VCP4`; text SHA-256: `284b6038b5d277475b5ef7591e98b198da4aa031ca1a806d536251ccefc42e6c`.

~~~~markdown
- consume structural regions, escaping/staticness facts and style/scope relations;
~~~~

### SRC-COMP-L1324-CFE720820194

- Kind: `requirement`; source: `compiler-proposal.md:1324-1324`; target: `node:VCP4`; text SHA-256: `cfe720820194995011b8afc45ea04122c3fd2961e02ae930ac4f83cefdd58030`.

~~~~markdown
- segment-oriented server emission; materialize an SSR plan only where it avoids rediscovery;
~~~~

### SRC-COMP-L1325-E7DB9F09040E

- Kind: `context`; source: `compiler-proposal.md:1325-1325`; target: `node:VCP4`; text SHA-256: `e7db9f09040efc0335dd7fd4b266a5a316f5c6233acbb33435d5cb4f3a4a75d0`.

~~~~markdown
- zero VDOM patch planning, zero Vapor dependency/effect graph, zero VST1 query work;
~~~~

### SRC-COMP-L1326-F07C076B5CD0

- Kind: `context`; source: `compiler-proposal.md:1326-1326`; target: `node:VCP4`; text SHA-256: `f07c076b5cd0a43bf43705c93848d02a0dab26208fb9770eb09e330a9aae018c`.

~~~~markdown
- share parse/semantic/structure with VDOM/Vapor in multi-target requests.
~~~~

### SRC-COMP-L1328-1E4BF1D3A01D

- Kind: `context`; source: `compiler-proposal.md:1328-1328`; target: `node:VCP4`; text SHA-256: `1e4bf1d3a01d79737e78ee77dc804362fd1e31beac1e93e294a66d41aa363639`.

~~~~markdown
**Suggested predecessors:** `VCP2`, `VST0`.
~~~~

### SRC-COMP-L1330-B94AEBF49DAF

- Kind: `context`; source: `compiler-proposal.md:1330-1330`; target: `node:VCP4`; text SHA-256: `b94aebf49dafe16299fe2a47e75263326dc89404b5dab4cc8abc2c7d08c18fa0`.

~~~~markdown
**Suggested subblocks:** text/escaping/static segments, elements/components/slots, control flow, SSR helpers/module surface, maps, multi-target/performance proof.
~~~~

### SRC-COMP-L1332-894F02A9EF9D

- Kind: `acceptance`; source: `compiler-proposal.md:1332-1332`; target: `node:VCP4`; text SHA-256: `894f02a9ef9d56e1d9a8a0fdff983e3f7f0de7a3fca2b7f2a162cb3cdbeeee88`.

~~~~markdown
**Acceptance:** locked SSR behavior/maps pass; client-plan counters are zero; VDOM+SSR shares prerequisites and branches at the locked point; output remains deterministic across direct/prepared/managed paths.
~~~~

### SRC-COMP-L1334-D22C4920A312

- Kind: `forbidden`; source: `compiler-proposal.md:1334-1334`; target: `node:VCP4`; text SHA-256: `d22c4920a31229faf9ec753b9a81bd60825394810c223c0156d66839212215f8`.

~~~~markdown
**Forbidden:** reusing client target state merely for symmetry, client effect graph, or whole-tree server IR without measured need.
~~~~

### SRC-COMP-L1336-B51294648A2F

- Kind: `deletion`; source: `compiler-proposal.md:1336-1336`; target: `node:VCP4`; text SHA-256: `b51294648a2f9049196903cdd7b205f4ed32f2e83d2b7d098e19e7c1e24596e6`.

~~~~markdown
**Deletion/abort:** old SSR path deleted at framework cutover after parity.
~~~~

### SRC-COMP-L1338-F52D711103D5

- Kind: `context`; source: `compiler-proposal.md:1338-1338`; target: `node:VCP4`; text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`.

~~~~markdown
---
~~~~
