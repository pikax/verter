# Exact operative source-clause attachment — VIM1

Schema: 1. Node: `VIM1`. Clause count: 2. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-EXP-L995-57BBEF470724

- Kind: `context`; source: `successor-expansion.md:995-995`; target: `node:VIM1`; text SHA-256: `57bbef470724c4c2bfe2b5649aeb71f7f98310d330219a085b211b7384214c8b`.

~~~~markdown
### `VIM1.md` — Deterministic manifest compiler and conformance generator
~~~~

### SRC-EXP-L997-62B859F666AF

- Kind: `forbidden`; source: `successor-expansion.md:997-1002`; target: `node:VIM1`; text SHA-256: `62b859f666af52d2741baca22a0c1dc3613ddc2f2bc8e69d818e19c7f3907dd7`.

~~~~markdown
**Intent:** make CI and agents enforce the same vertical rules through repository-owned tooling.
**Predecessors:** `VIM0`, `CEF0`, `COX0`, `LRA0`, `FMK0`, `PUB0`, `PER0`.
**Subblocks:** (1) `cargo xtask vertical new`; (2) `check`; (3) `matrix`; (4) `charters`; (5) `test-plan`; (6) generated descriptor/client/capability/test registration checks; (7) deterministic output and forbidden-dependency closure.
**Acceptance:** two clean runs are byte-identical; malformed/negative manifests fail for semantic reasons rather than keyword grep; generated charters contain all required cells but no semantic implementation; CI invokes the same validator API used by skills.
**Forbidden:** skill-local validation authority, source rewriting outside declared generated files, auto-ratification, or generating framework algorithms.
**Deletion/abort:** remove hand-maintained mirrors only after freshness guards prove replacement; abort if generation would require executing vertical code.
~~~~
