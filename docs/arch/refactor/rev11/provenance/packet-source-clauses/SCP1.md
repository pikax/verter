# Exact operative source-clause attachment — SCP1

Schema: 1. Node: `SCP1`. Clause count: 17. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-COMP-L1453-8A81B511151A

- Kind: `context`; source: `compiler-proposal.md:1453-1453`; target: `node:SCP1`; text SHA-256: `8a81b511151a02b7857069db9bfc1b099a1b1e6efbd54bef6b85db337334389d`.

~~~~markdown
## `SCP1.md` — Canonical Svelte semantic authority convergence
~~~~

### SRC-COMP-L1455-5695E173AE5E

- Kind: `context`; source: `compiler-proposal.md:1455-1455`; target: `node:SCP1`; text SHA-256: `5695e173ae5e93acf855f235523f8687e3d19433e1e8092cba0b9160a1d251cf`.

~~~~markdown
**Intent:** make one Svelte semantic authority own all target-independent framework meaning.
~~~~

### SRC-COMP-L1457-B6C3392648E9

- Kind: `context`; source: `compiler-proposal.md:1457-1457`; target: `node:SCP1`; text SHA-256: `b6c3392648e91921a7f6f998face6b1b1910c2c956652ec13bb3742058401b7f`.

~~~~markdown
**Problem:** client/server/style/compiler paths can duplicate runes, stores, scope, dependency, mutation and template analyses.
~~~~

### SRC-COMP-L1459-56832D9ECFE1

- Kind: `context`; source: `compiler-proposal.md:1459-1459`; target: `node:SCP1`; text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`.

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L1461-EB7E957BF2F5

- Kind: `requirement`; source: `compiler-proposal.md:1461-1461`; target: `node:SCP1`; text SHA-256: `eb7e957bf2f51b17c3c9434e46b4f995efa7bf08a944d1ce36376ad05fa63146`.

~~~~markdown
- one authority for runes/legacy mode, scopes, bindings, references, mutations, stores, runes, read/write/dependency sets, purity/staticness, template scopes, components/elements, actions/transitions/animations/bindings and style cross-language facts;
~~~~

### SRC-COMP-L1462-00098E725F00

- Kind: `context`; source: `compiler-proposal.md:1462-1462`; target: `node:SCP1`; text SHA-256: `00098e725f00c1ba4b3028c10f93abc9ead2982fce45d1e4f3cf9d1730447866`.

~~~~markdown
- shared `verter_analysis`/`type_info` machinery, framework-owned interpretation;
~~~~

### SRC-COMP-L1463-FB8D1528B370

- Kind: `context`; source: `compiler-proposal.md:1463-1463`; target: `node:SCP1`; text SHA-256: `fb8d1528b37085e21f3213237e4c8e5caadfd963ead7b54dd7e941a6240e1384`.

~~~~markdown
- compact dense hot facts and sparse explanations;
~~~~

### SRC-COMP-L1464-01D655EEEBA3

- Kind: `context`; source: `compiler-proposal.md:1464-1464`; target: `node:SCP1`; text SHA-256: `01d655eeeba33ce1042c6a7658048466bd62b6405b712758ca403938f393ffce`.

~~~~markdown
- one authoritative script/expression parse and import analysis;
~~~~

### SRC-COMP-L1465-677819916B6A

- Kind: `forbidden`; source: `compiler-proposal.md:1465-1465`; target: `node:SCP1`; text SHA-256: `677819916b6af9a9f294fae565df7d4b66e896078d0ed54b3f9618dc798444e4`.

~~~~markdown
- client/server/module/style consumers use policy-restricted views, never duplicate analysis;
~~~~

### SRC-COMP-L1466-50E579A0FF07

- Kind: `requirement`; source: `compiler-proposal.md:1466-1466`; target: `node:SCP1`; text SHA-256: `50e579a0ff07a7be7345a041903319a35f6d4b66a7b59134ccb23a7741868ffb`.

~~~~markdown
- `Default` performs all required component-local semantics and no project-wide investigation.
~~~~

### SRC-COMP-L1468-2D4DEA423422

- Kind: `context`; source: `compiler-proposal.md:1468-1468`; target: `node:SCP1`; text SHA-256: `2d4dea423422ad163b14b5b9646dee0dd2a4925a2ce1777e2a9e040be31cb922`.

~~~~markdown
**Suggested predecessor:** `SCP0`.
~~~~

### SRC-COMP-L1470-67EB3F726DC5

- Kind: `deletion`; source: `compiler-proposal.md:1470-1470`; target: `node:SCP1`; text SHA-256: `67eb3f726dc55455791ba0863975683a263ec701eea6f0a2944b1df124725fa6`.

~~~~markdown
**Suggested subblocks:** script/rune/store facts, scopes/bindings/dependencies, template/component/directive facts, style cross-language hooks, compact storage, duplicate-analysis deletion.
~~~~

### SRC-COMP-L1472-18257E86C6F5

- Kind: `acceptance`; source: `compiler-proposal.md:1472-1472`; target: `node:SCP1`; text SHA-256: `18257e86c6f5164c66f6c5af60dd9dd7002213cfee40600108a229c9266fb029`.

~~~~markdown
**Acceptance:** client/server/style agree on every shared fact; no raw source semantic searches or downstream reparses remain; unknown dynamic cases fail open/conservative; work ledger shows one fact production.
~~~~

### SRC-COMP-L1474-43D76DF3ECDC

- Kind: `forbidden`; source: `compiler-proposal.md:1474-1474`; target: `node:SCP1`; text SHA-256: `43d76df3ecdc917152ad0852c082b991b040b8ff48e1bccbb520957580a32dd8`.

~~~~markdown
**Forbidden:** compiler-owned Svelte semantics, universal reactivity schema, source-string structural scanning, or project optimization.
~~~~

### SRC-COMP-L1476-8E1A64E44A24

- Kind: `deletion`; source: `compiler-proposal.md:1476-1476`; target: `node:SCP1`; text SHA-256: `8e1a64e44a24937afa7732f9f528a9d71514e6132567c853e499d1461fcf77c1`.

~~~~markdown
**Deletion/abort:** delete duplicate facts only after cross-consumer parity.
~~~~

### SRC-COMP-L1478-F52D711103D5

- Kind: `context`; source: `compiler-proposal.md:1478-1478`; target: `node:SCP1`; text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`.

~~~~markdown
---
~~~~

### SRC-LEGACY-TRANSFER-1B11EAEACCFE

- Kind: `requirement`; source: `legacy-architecture-transfers.md:558-563`; target: `node:SCP0`; text SHA-256: `196151f1cc3d11ad89808f4d1c20e806d32733cbfefc5ae47875e2d2aded0d98`.

~~~~markdown
### LEGACY-TRANSFER-1B11EAEACCFE

- Original path: `docs/arch/svelte-native-compiler-plan.md`; Git blob: `1b11eaeaccfea6baaad3684710026923b734bb88`; exact source SHA-256: `e96ca99c36787fbb0d9d29300601c3a58d653a0fb57f89a560a24080662dd7ad`.
- Exact retained source: `sources/legacy-architecture-transfers/svelte-native-compiler-plan.md`.
- Applicable authority: `SCP0`, `SCP1`, `SCP2`, `SCP3`, `SCP4`, `SCP5`, `SCP6`, `SCP7`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~
