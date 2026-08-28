# Exact operative source-clause attachment — LSO4

Schema: 1. Node: `LSO4`. Clause count: 7. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-LEGACY-EXISTING-CACHE-001

- Kind: `requirement`; source: `legacy-arch-reconciliation.md:199-201`; target: `node:G1`; text SHA-256: `ecf95ce060d2ba6503880cd6ec607e851edb63871b62779606fd5ef981cdc27e`.

~~~~markdown
### EXISTING-CACHE-001

Fact/query/result caches use exact structural identity, read-set validation, complete-only admission, singleflight, cancellation, and reclaimable storage. Targets: `G1`, `G2`, `E4`, `H1`, all successor caches. Related source: `docs/arch/fact-based-cache.md`, blob `1f97d9be730193400629485e8c86415b35834f27`.
~~~~

### SRC-LEGACY-LSO-AUTHORED-001

- Kind: `requirement`; source: `legacy-arch-reconciliation.md:72-77`; target: `node:LSO0`; text SHA-256: `a1f1fa596f84a8e6d8d0ee8f97409e66df02aa16e466c983a13279fb859b4c25`.

~~~~markdown
### LSO-AUTHORED-001 — Authored-coordinate public boundary

- Core operations use authored source units, semantic subjects, exact profiles, and typed outcomes.
- LSP positions, generated paths, provider JSON, and final workspace edits are edge concerns.
- Approximate, nearest, `Range::default`, and `0:0` fallbacks are forbidden.
- Targets: `LSO0`, all LSO implementations.
~~~~

### SRC-LEGACY-LSO-OCCURRENCE-001

- Kind: `requirement`; source: `legacy-arch-reconciliation.md:103-108`; target: `node:LSO4`; text SHA-256: `d6428154331475f108564a8da2f0c1f9fc800420967372617e56192312308e3b`.

~~~~markdown
### LSO-OCCURRENCE-001 — Typed bounded occurrences

- References/hierarchy/rename use role-typed semantic occurrences.
- IDX0 narrows candidates but never establishes authority.
- Incomplete/budgeted/cancelled enumeration cannot cache a complete negative result.
- Targets: `LSO4`, `LSO5`.
~~~~

### SRC-LEGACY-LSO-TARGET-001

- Kind: `requirement`; source: `legacy-arch-reconciliation.md:79-86`; target: `node:LSO2`; text SHA-256: `184c1d120b6e481757de9cae3264643f7ba6310fe2877bc4924254bc69f381da`.

~~~~markdown
### LSO-TARGET-001 — One target/provenance graph

- Definition, type-definition, implementation, references, hierarchy, rename, hover links, and completion resolve share one canonical target identity and provenance graph.
- URI/range is rendering, not semantic identity.
- Every foreign target uses its own snapshot, line index, mapper, and analysis.
- Generated mapping requires exact provider/mapper snapshot equality.
- Targets: `LSO2`, `LSO3`, `LSO4`, `LSO5`, `LSO6`, `LSO7`.
- Source: `docs/arch/goto-definition-architecture-decision.md`, blob `9c48db563e0f411da1983d1b3cb5374b4f59b0ca`.
~~~~

### SRC-LEGACY-TRANSFER-9C48DB563E0F

- Kind: `requirement`; source: `legacy-architecture-transfers.md:250-255`; target: `node:LSO0`; text SHA-256: `7aa614bb693d3c19bad3164ffd380fea4a8a8e9dce959cafcdefa31f656e6564`.

~~~~markdown
### LEGACY-TRANSFER-9C48DB563E0F

- Original path: `docs/arch/goto-definition-architecture-decision.md`; Git blob: `9c48db563e0f411da1983d1b3cb5374b4f59b0ca`; exact source SHA-256: `7d706c49d70a317a9b218cdef55cc01ec7211ba4d0bdeadac60505e9e4a445c4`.
- Exact retained source: `sources/legacy-architecture-transfers/goto-definition-architecture-decision.md`.
- Applicable authority: `LSO0`, `LSO2`, `LSO3`, `LSO4`, `LSO5`, `LSO8`, `LSO9`, `LSO10`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-DE8DD6E47BDF

- Kind: `requirement`; source: `legacy-architecture-transfers.md:166-171`; target: `node:LSO4`; text SHA-256: `5dab874a35f6fad4983f5b3af4ae8681b03695b38a86f804090087acaed6a8e1`.

~~~~markdown
### LEGACY-TRANSFER-DE8DD6E47BDF

- Original path: `docs/arch/future/semantic-dispatch-connected-depth-budget-reset.md`; Git blob: `de8dd6e47bdf9f10d2e556d088f827bbf72f8ad9`; exact source SHA-256: `9e25db54f57b62dab17cba592334b0ddbc0f4af1aa70fd80394898ee0fcf85bd`.
- Exact retained source: `sources/legacy-architecture-transfers/future/semantic-dispatch-connected-depth-budget-reset.md`.
- Applicable authority: `LSO4`, `PER0`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-SUCCESSOR-DAG-AMENDMENT

- Kind: `context`; source: `successor-dag-amendment.md:1-1`; target: `node:NCK0`; text SHA-256: `9413cba2563db3ebfda5614b0ecd45ba6757581a4f7a20da7341ed2b3dc1d128`.

~~~~markdown
# Rev11 legacy-architecture reconciliation and successor charter pack
~~~~
