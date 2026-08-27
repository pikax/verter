# Exact operative source-clause attachment — CMP1

Schema: 1. Node: `CMP1`. Clause count: 20. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-COMP-L956-7B9E25A055F5

- Kind: `context`; source: `compiler-proposal.md:956-956`; target: `node:CMP1`; text SHA-256: `7b9e25a055f59ee98469e983a12d76a52f2c48d4f681c099a763adddb9959506`.

~~~~markdown
## `CMP1.md` — Demand-refined semantic consumption and admissions
~~~~

### SRC-COMP-L958-139F3B75F069

- Kind: `requirement`; source: `compiler-proposal.md:958-958`; target: `node:CMP1`; text SHA-256: `139f3b75f0694a60897bc7cf1239e9baea9189fcfcf0443c038a9c577a70a2d5`.

~~~~markdown
**Intent:** ensure runtime compilation reuses the canonical framework analysis and computes only demanded fact families.
~~~~

### SRC-COMP-L960-F160EF349642

- Kind: `context`; source: `compiler-proposal.md:960-960`; target: `node:CMP1`; text SHA-256: `f160ef3496423a18bc641f6a4a805298a7eb9a9773d38b3c8bb9a4c7b33f31cf`.

~~~~markdown
**Problem:** compiler-local semantic analysis, repeated import/expression parsing, and a demand plan created after semantic work cause disagreement and unnecessary work.
~~~~

### SRC-COMP-L962-56832D9ECFE1

- Kind: `context`; source: `compiler-proposal.md:962-962`; target: `node:CMP1`; text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`.

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L964-40F1F006AFEC

- Kind: `context`; source: `compiler-proposal.md:964-964`; target: `node:CMP1`; text SHA-256: `40f1f006afec724ea3b83b111475f5a6840ef2fe8e1d131020034d0079fb4c4f`.

~~~~markdown
- specialize successor `DEM0` into a finite compiler demand closure;
~~~~

### SRC-COMP-L965-F81248E9F3FE

- Kind: `requirement`; source: `compiler-proposal.md:965-965`; target: `node:CMP1`; text SHA-256: `f81248e9f3fe7402437863cf01e67eb7361b6c40fad0f7955519009b03a478e0`.

~~~~markdown
- create exact reason edges from target/product to required parse, semantic, style, map, planning and emission capabilities;
~~~~

### SRC-COMP-L966-856F04BE7F1A

- Kind: `context`; source: `compiler-proposal.md:966-966`; target: `node:CMP1`; text SHA-256: `856f04be7f1a727c301f97442f4ae60046b6097606f7389f912679a5e3d6ace9`.

~~~~markdown
- obtain `ParseAdmission` from each demanded frontend/region;
~~~~

### SRC-COMP-L967-910B6CDC47B5

- Kind: `requirement`; source: `compiler-proposal.md:967-967`; target: `node:CMP1`; text SHA-256: `910b6cdc47b5988a05f1938d1626218c2a7df1898eae91ca42ddea057239c0e9`.

~~~~markdown
- ask the exact framework semantic authority for demanded fact families;
~~~~

### SRC-COMP-L968-04EB3AB51E07

- Kind: `requirement`; source: `compiler-proposal.md:968-968`; target: `node:CMP1`; text SHA-256: `04eb3ab51e07d9e6aee845b6a0c581ab81600db6ff767a12a541aad7c49f36ab`.

~~~~markdown
- obtain `SemanticAdmission` with exact source/fact basis and coverage;
~~~~

### SRC-COMP-L969-9D93926C5DE6

- Kind: `context`; source: `compiler-proposal.md:969-969`; target: `node:CMP1`; text SHA-256: `9d93926c5de6390829f5b20c9c3dbeac101ffcd9bc3a120f514c6c7baedc2e06`.

~~~~markdown
- compose `CompileAdmission` without rerunning analysis;
~~~~

### SRC-COMP-L970-B25701236E50

- Kind: `requirement`; source: `compiler-proposal.md:970-970`; target: `node:CMP1`; text SHA-256: `b25701236e50805b397255155d8a40321ef1d63491ac02bae666c1e4b2d27522`.

~~~~markdown
- expose policy-restricted read-only compiler views over the same facts;
~~~~

### SRC-COMP-L971-EF22C6881636

- Kind: `context`; source: `compiler-proposal.md:971-971`; target: `node:CMP1`; text SHA-256: `ef22c6881636bd169755fa719d3c519dd831e0dc2277ce730352a03a7238b0a7`.

~~~~markdown
- allow `Default` component-local provenance through immutable aliases and literal canonical framework imports without loading external files;
~~~~

### SRC-COMP-L972-90D91DCE44C3

- Kind: `context`; source: `compiler-proposal.md:972-972`; target: `node:CMP1`; text SHA-256: `90d91dce44c3947506774062209643f560107cd0ffd91e74e57bae550aa1195f`.

~~~~markdown
- do not use ambient LSP/tsgo state;
~~~~

### SRC-COMP-L973-E7F8655452BA

- Kind: `requirement`; source: `compiler-proposal.md:973-973`; target: `node:CMP1`; text SHA-256: `e7f8655452bac6a575a3f574082335c69ff277bf535cdc0d11b9921df82b68e3`.

~~~~markdown
- return `NeedInputs` for genuinely required external style stages and resume on the same basis.
~~~~

### SRC-COMP-L975-0806E5E726F9

- Kind: `context`; source: `compiler-proposal.md:975-975`; target: `node:CMP1`; text SHA-256: `0806e5e726f9f096489f20004fd8db34179cbbca2061f6313b6106b92a99bb7b`.

~~~~markdown
**Suggested predecessors:** `CMP0`, `CPER1`.
~~~~

### SRC-COMP-L977-AEE3F0CA187C

- Kind: `deletion`; source: `compiler-proposal.md:977-977`; target: `node:CMP1`; text SHA-256: `aee3f0ca187cacee8aabcbd775fa2f865bd07078c611549fceb3228e2ddbe120`.

~~~~markdown
**Suggested subblocks:** demand universe, closure engine, parse admission, semantic admission/view, compile admission/resume, duplicate-analysis deletion.
~~~~

### SRC-COMP-L979-3E38B2129EAA

- Kind: `acceptance`; source: `compiler-proposal.md:979-979`; target: `node:CMP1`; text SHA-256: `3e38b2129eaac3c00ad0b46d64101f546014792addfbb84cc959191ae7810e0d`.

~~~~markdown
**Acceptance:** each exact expression region has one authoritative parsed representation after grammar selection; import/binding/reactivity/dependency facts have one framework owner; the compiler cannot call a second parser/analyzer; capabilities absent from closed demand have zero ledger work; alias-proven local reactivity reaches `Default` target planning.
~~~~

### SRC-COMP-L981-4C9DC1F4E15A

- Kind: `forbidden`; source: `compiler-proposal.md:981-981`; target: `node:CMP1`; text SHA-256: `4c9dc1f4e15a7d7856da1876379e8d9950a9fb8a8cb8c19a7470c52206c1c3de`.

~~~~markdown
**Forbidden:** per-node calls into external providers, field-wise fact merging, compiler-specific import scanning, late demand expansion after target execution begins, or a monolithic eager semantic snapshot.
~~~~

### SRC-COMP-L983-9E0B31C24B35

- Kind: `deletion`; source: `compiler-proposal.md:983-983`; target: `node:CMP1`; text SHA-256: `9e0b31c24b35abd19cc78f77f1beee727b1d9a14194a490ad7262d894be8e8b3`.

~~~~markdown
**Deletion/abort:** delete duplicate compiler-local analysis only with fact/output parity; rescope any semantic fact that lacks one framework owner.
~~~~

### SRC-COMP-L985-F52D711103D5

- Kind: `context`; source: `compiler-proposal.md:985-985`; target: `node:CMP1`; text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`.

~~~~markdown
---
~~~~
