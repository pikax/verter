# Exact operative source-clause attachment — LNT0

Schema: 1. Node: `LNT0`. Clause count: 3. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-EXP-L1399-73F7D9EB4E59

- Kind: `context`; source: `successor-expansion.md:1399-1399`; target: `contract:contracts/sizing.md`; text SHA-256: `73f7d9eb4e59fd0a41a536631b833da0aa210346eee561fe252eb0137b3dcd19`.

~~~~markdown
## 13. Native lint product train
~~~~

### SRC-EXP-L1401-D279D4260805

- Kind: `context`; source: `successor-expansion.md:1401-1401`; target: `node:LNT0`; text SHA-256: `d279d42608057d0c725be38465501b7fd5150b21f7e9d3ef8e36ff0b25ee124c`.

~~~~markdown
### `LNT0.md` — Native lint product and compatibility lock
~~~~

### SRC-EXP-L1403-50E84741BCC6

- Kind: `forbidden`; source: `successor-expansion.md:1403-1408`; target: `node:LNT0`; text SHA-256: `50e84741bcc68363f5b36c06513e30b8aab55646ac70ac9a9d2ddc75fd3747eb`.

~~~~markdown
**Intent:** freeze the native/equivalent/external rule universe and product claims without inventing another lint engine.
**Predecessors:** `LRA0`, `CFG0`.
**Subblocks:** (1) inventory current Verter rules and fixes; (2) pin ESLint, TypeScript-ESLint, eslint-plugin-vue, Svelte, Stylelint, and relevant framework rule versions; (3) classify NativeEquivalent/VerterOnly/ExternalOnly/Unsupported cells; (4) lock diagnostic/fix compatibility; (5) lock corpus/performance/zero-work gates; (6) ratify config and external-runner policy.
**Acceptance:** no blanket “ESLint compatible” claim; every rule ID has exact applicability, owner, fact demand, oracle, and fix safety.
**Forbidden:** running arbitrary plugins in core, claiming compatibility from similar names, or choosing easy rules after implementation.
**Deletion/abort:** no code; rescope incompatible semantic rules explicitly.
~~~~
