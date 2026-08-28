# Exact operative source-clause attachment — SKL3

Schema: 1. Node: `SKL3`. Clause count: 2. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-EXP-L1085-C7DE94B69FDB

- Kind: `context`; source: `successor-expansion.md:1085-1085`; target: `node:SKL3`; text SHA-256: `c7de94b69fdbd3c27ffab4864ace128e23a8cea7d8f5218ed8b6396e45905c67`.

~~~~markdown
### `SKL3.md` — Maintainer-ratified atomic workflow activation
~~~~

### SRC-EXP-L1087-92E645D3EA33

- Kind: `forbidden`; source: `successor-expansion.md:1087-1092`; target: `node:SKL3`; text SHA-256: `92e645d3ea333022641dfe58fef8147951bf8ab88a6974645ef3216e243b7626`.

~~~~markdown
**Intent:** switch repository routing to the reviewed skills atomically, with no interval containing zero or two active integration workflows.
**Predecessors:** `SKL2`.
**Subblocks:** (1) verify the `SKL2` semantic/test receipt; (2) stage the complete skills+AGENTS+discovery+old-workflow-retirement cutover candidate; (3) run fresh routing/negative tests and independent Codex Architect review on that exact tree; (4) obtain explicit maintainer adoption over the reviewed digest; (5) land one equivalent atomic commit; (6) verify landing equivalence and rollback restoration.
**Acceptance:** exactly one lifecycle-paired workflow is active before and after cutover; review and adoption both bind the complete cutover tree; any fix invalidates both receipts; rollback restores the old routing atomically.
**Forbidden:** self-ratification, activation before review, deletion before replacement, two competing active entry points, or manual post-landing edits.
**Deletion/abort:** retire only the old invocable entry point and duplicate routing after zero-consumer proof; abort and keep the old workflow active on any digest/routing mismatch.
~~~~
