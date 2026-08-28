# Exact operative source-clause attachment — HWC1

Schema: 1. Node: `HWC1`. Clause count: 2. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-EXP-L1105-95B62A580C73

- Kind: `context`; source: `successor-expansion.md:1105-1105`; target: `node:HWC1`; text SHA-256: `95b62a580c73ecd9ff960da34ae02973320342821f313e0826f52d0b0bb9ec7c`.

~~~~markdown
### `HWC1.md` — Independent neutral HTML parser and recovery corpus
~~~~

### SRC-EXP-L1107-345055327CE5

- Kind: `forbidden`; source: `successor-expansion.md:1107-1112`; target: `node:HWC1`; text SHA-256: `345055327ce54ce7c5ced4ee2bf2c7492b48a8da200c9d984055c640dcec427d`.

~~~~markdown
**Intent:** create an owned HTML syntax frontend by copying/specializing the closest proven parser, not by building an omni parser.
**Predecessors:** `HWC0`, `PAR0`, `ENC1`.
**Subblocks:** (1) fork exact Vue parser lineage into the locked owner; (2) remove Vue directives/interpolation/component assumptions; (3) implement admitted HTML tokenization, tree facts, entities, namespaces, raw-text, comments, malformed recovery, and stable IDs; (4) add WPT/differential/fuzz corpus; (5) add incremental/full parity and budgets; (6) prove no dependency back to Vue.
**Acceptance:** pinned standards cells and malformed corpus pass; a source revision is parsed once; Unicode spans are exact; allocations/latency meet prelocked gates.
**Forbidden:** parameterizing the Vue parser with `is_vue`, sharing semantic AST types, broad unsupported recovery success, or importing framework semantics.
**Deletion/abort:** delete copied Vue-only paths and names; abort if independent ownership cannot be obtained without changing Vue behavior.
~~~~
