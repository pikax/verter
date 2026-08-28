# Exact operative source-clause attachment — VST0

Schema: 1. Node: `VST0`. Clause count: 17. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-COMP-L1229-35A955C02589

- Kind: `context`; source: `compiler-proposal.md:1229-1229`; target: `node:VST0`; text SHA-256: `35a955c025890f1a7b7aad756226b2da9c1c91a1317646563c3f85c396f6f5cc`.

~~~~markdown
## `VST0.md` — Vue framework style semantics and scope plan
~~~~

### SRC-COMP-L1231-4DB1265D90FB

- Kind: `context`; source: `compiler-proposal.md:1231-1231`; target: `node:VST0`; text SHA-256: `4db1265d90fb803cfe532f3298e941c4ad2eb5de45b31f8b75768f9427655229`.

~~~~markdown
**Intent:** consume J-owned style products and produce canonical Vue-specific style facts once.
~~~~

### SRC-COMP-L1233-0E7C29E8DC29

- Kind: `context`; source: `compiler-proposal.md:1233-1233`; target: `node:VST0`; text SHA-256: `0e7c29e8dc29a8fd9a0c60753ae51cb63c22f74e1c79134d42a8601b30a67e93`.

~~~~markdown
**Problem:** Vue style meaning can be extracted inside compiler/session code, external processing stages can be ambiguous, and template/style scope identity can diverge.
~~~~

### SRC-COMP-L1235-56832D9ECFE1

- Kind: `context`; source: `compiler-proposal.md:1235-1235`; target: `node:VST0`; text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`.

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L1237-E915EDE74014

- Kind: `context`; source: `compiler-proposal.md:1237-1237`; target: `node:VST0`; text SHA-256: `e915ede7401433aaf8d0b5588f95a72cf048883a7157f635c9a7ad786a3d155d`.

~~~~markdown
- consume `StyleSyntaxIr` and J neutral facts only;
~~~~

### SRC-COMP-L1238-C39AA606E1C1

- Kind: `requirement`; source: `compiler-proposal.md:1238-1238`; target: `node:VST0`; text SHA-256: `c39aa606e1c118c9cd1975ee815fb5e2feb496fa9cff002b8445d9f4261f7ecb`.

~~~~markdown
- own Vue meaning for `v-bind()`, `:deep`, `:global`, `:slotted`, scoped selectors/keyframes, CSS Modules semantic exposure, and framework diagnostics;
~~~~

### SRC-COMP-L1239-9E4597B1CE1E

- Kind: `requirement`; source: `compiler-proposal.md:1239-1239`; target: `node:VST0`; text SHA-256: `9e4597b1ce1e0a8fde07732e6deb85f2e3478bc219e2ccb1319d1e9c0ed441ba`.

~~~~markdown
- convert style expressions to source-backed `ExprId`/binding/dependency facts through the canonical Vue semantic authority;
~~~~

### SRC-COMP-L1240-861040E632F6

- Kind: `context`; source: `compiler-proposal.md:1240-1240`; target: `node:VST0`; text SHA-256: `861040e632f6046690dccd7b3241bbe2b9f219a92d3a89a895b319742c5a53ab`.

~~~~markdown
- create one `VueComponentScopePlan` consumed by template, style, SSR and metadata paths;
~~~~

### SRC-COMP-L1241-BB1002931352

- Kind: `requirement`; source: `compiler-proposal.md:1241-1241`; target: `node:VST0`; text SHA-256: `bb10029313520cc3992fb0d4153788cf4c970f591bb5ee0e142a2aa672725b50`.

~~~~markdown
- consume exact stage-qualified external preprocessor/PostCSS results and compose maps;
~~~~

### SRC-COMP-L1242-C3C4997A5FF0

- Kind: `context`; source: `compiler-proposal.md:1242-1242`; target: `node:VST0`; text SHA-256: `c3c4997a5ff0ed9782693771de512fefe7693d662dac0c36dca494bcf489e2eb`.

~~~~markdown
- perform no native Sass/Less/Stylus execution;
~~~~

### SRC-COMP-L1243-CC96299BA45B

- Kind: `context`; source: `compiler-proposal.md:1243-1243`; target: `node:VST0`; text SHA-256: `cc96299ba45b316ecf1f2bd7b7ed6527a82f0c34975e6579dd6bf63cd5eff454`.

~~~~markdown
- do not implement selector-to-template matching in this block.
~~~~

### SRC-COMP-L1245-21BEDA004DA0

- Kind: `context`; source: `compiler-proposal.md:1245-1245`; target: `node:VST0`; text SHA-256: `21beda004da0eceadc30037990e9e697609cce20fa9ab17c7ac98c89da679849`.

~~~~markdown
**Suggested predecessor:** `VCP1`.
~~~~

### SRC-COMP-L1247-1F80BE81BFE2

- Kind: `context`; source: `compiler-proposal.md:1247-1247`; target: `node:VST0`; text SHA-256: `1f80be81bfe2400c5e81bd17b8be110923d8345f888badd9707febaf6c53c8a2`.

~~~~markdown
**Suggested subblocks:** J integration, Vue selector/directive facts, CSS-variable expressions, scope/keyframe plan, CSS Modules semantic facts, external-stage/map integration.
~~~~

### SRC-COMP-L1249-4061804FCDD9

- Kind: `acceptance`; source: `compiler-proposal.md:1249-1249`; target: `node:VST0`; text SHA-256: `4061804fcdd91996cb53db556cc65c197aa9a3c29511b0f29fe4b8fb2351800f`.

~~~~markdown
**Acceptance:** no compiler/session raw CSS scan remains for migrated facts; template/style scope identity cannot disagree; preprocess-dependent work is exact `NeedInputs`; maps compose across all admitted stages; no second CSS grammar exists.
~~~~

### SRC-COMP-L1251-DC4F16CB30E5

- Kind: `forbidden`; source: `compiler-proposal.md:1251-1251`; target: `node:VST0`; text SHA-256: `dc4f16cb30e54aeeb82bf5e1d369b618d566de246cc57c2e73678963fdeb72bf`.

~~~~markdown
**Forbidden:** CSS reparsing, compiler-owned style semantics, opaque “processed CSS” strings, native preprocessors, or selector pruning.
~~~~

### SRC-COMP-L1253-B1B7689A8677

- Kind: `deletion`; source: `compiler-proposal.md:1253-1253`; target: `node:VST0`; text SHA-256: `b1b7689a8677e2539ea99ace46edddb23196f4c2a7abc575f1f3edc5c455eccc`.

~~~~markdown
**Deletion/abort:** delete replaced Vue style scanners/extractors after parity; stop if stage ordering cannot be proven.
~~~~

### SRC-COMP-L1255-F52D711103D5

- Kind: `context`; source: `compiler-proposal.md:1255-1255`; target: `node:VST0`; text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`.

~~~~markdown
---
~~~~
