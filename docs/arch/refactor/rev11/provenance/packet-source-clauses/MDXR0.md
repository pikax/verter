# Exact operative source-clause attachment — MDXR0

Schema: 1. Node: `MDXR0`. Clause count: 2. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-EXP-L1262-43D3DF1D51A1

- Kind: `context`; source: `successor-expansion.md:1262-1262`; target: `node:MDXR0`; text SHA-256: `43d3df1d51a1792e5b3484cb6ed96f83aee289bf42a3d7cea533f744d6354fb5`.

~~~~markdown
### `MDXR0.md` — React-specific MDX component-provider proof
~~~~

### SRC-EXP-L1264-F7BCEC6104A7

- Kind: `forbidden`; source: `successor-expansion.md:1264-1269`; target: `node:MDXR0`; text SHA-256: `f7bcec6104a7b91b9af0d2335ff62d01e17cf00c7350b6e9d53503470bd2e229`.

~~~~markdown
**Intent:** prove React-component auto-import and navigation in MDX only after a bounded React semantic provider exists.
**Predecessors:** `RCTP`, `MDXP`, `IDX0`.
**Subblocks:** (1) define the bounded React `ComponentInfo` provider contract; (2) join MDX JSX uses with proven React candidates; (3) rank auto-imports from exact package/project/export provenance; (4) produce import edits and definition/navigation maps; (5) reject Solid/Preact/plain-JSX/userland ambiguities; (6) test cancellation, index budgets, stale bases, and zero work.
**Acceptance:** React auto-import appears only for proven React profile/project candidates; generic MDX functionality remains available without React; no full React vertical or CLI is required.
**Forbidden:** capitalized-name heuristics, assuming all JSX is React, unbounded workspace scans, duplicate TS programs, or MDX-owned React semantics.
**Deletion/abort:** all proof-local provider/join code is deleted or remains unreachable after the experiment; no production terminal may depend on `MDXR0`. Its evidence may seed the separately ratified bounded React-provider production train described in §15.4.
~~~~
