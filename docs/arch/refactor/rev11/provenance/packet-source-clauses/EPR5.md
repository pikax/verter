# Exact operative source-clause attachment — EPR5

Schema: 1. Node: `EPR5`. Clause count: 6. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-LEGACY-EPR-ACTIVATE-001

- Kind: `requirement`; source: `legacy-arch-reconciliation.md:181-186`; target: `node:EPR5`; text SHA-256: `8ccd7625df8c8294d21aeb4f157e3c5860e4ea6c4c42c2f0cb6670dde7f25bc5`.

~~~~markdown
### EPR-ACTIVATE-001 — Healthy applied binding is availability

- Activation revalidates the selection handoff, spawns/attaches under bounded control, performs version/protocol/capability handshake, and atomically publishes a project-scoped ProviderEpoch.
- Process existence or configured mode is not availability.
- Swap/restart/crash/rollback is stale-safe and invalidates old handles.
- Targets: `EPR5`, `EPR6`.
~~~~

### SRC-LEGACY-EPR-VALIDATE-001

- Kind: `requirement`; source: `legacy-arch-reconciliation.md:152-157`; target: `node:EPR1`; text SHA-256: `72820552573f62dc1574ec5b1655d958344860bba18fcf45e3a7d5846dcc015e`.

~~~~markdown
### EPR-VALIDATE-001 — Artifact identity before execution

- Path is a locator, not identity.
- Every candidate binds exact engine/version/flavor/platform/build/origin/content/integrity/compatibility/revocation evidence.
- Integrity/trust/revocation failure is loud and cannot become not-found fallback.
- Targets: `EPR1`, `EPR4`, `EPR5`.
~~~~

### SRC-LEGACY-EXISTING-SCHED-001

- Kind: `requirement`; source: `legacy-arch-reconciliation.md:211-213`; target: `node:G3`; text SHA-256: `26e21c0fe99be20d73d2c51603bdd12a2f57d6f6118f17d18064ad8ab0f3e667`.

~~~~markdown
### EXISTING-SCHED-001

Bounded CPU/I/O/provider execution, cancellation, owner-affine lifecycle commands, and stale-safe publication remain owned by G3/G5/H2/H3. Related source: `docs/arch/scheduler-lifecycle-unification-plan.md`, blob `872f72b6cdad3e3d078f6f589b2d6e6670271c54`.
~~~~

### SRC-LEGACY-TRANSFER-872F72B6CDAD

- Kind: `requirement`; source: `legacy-architecture-transfers.md:537-542`; target: `node:G3`; text SHA-256: `424dad8d9f85dccb98402e48dca0c7a8872e953c2d576ddaf809eaecf01c3df6`.

~~~~markdown
### LEGACY-TRANSFER-872F72B6CDAD

- Original path: `docs/arch/scheduler-lifecycle-unification-plan.md`; Git blob: `872f72b6cdad3e3d078f6f589b2d6e6670271c54`; exact source SHA-256: `2383e9c29d6a33960ff2e41766246311dfd9d2c2adfd92f2ce435ae3aeb197bc`.
- Exact retained source: `sources/legacy-architecture-transfers/scheduler-lifecycle-unification-plan.md`.
- Applicable authority: `G3`, `G5`, `H2`, `H3`, `EPR2`, `EPR5`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-D7A2F005EB35

- Kind: `requirement`; source: `legacy-architecture-transfers.md:180-185`; target: `node:H3`; text SHA-256: `28b7dd6fc108023454ab5724627d48adff85f3c4fccf1a426fbe3dd1166adf3b`.

~~~~markdown
### LEGACY-TRANSFER-D7A2F005EB35

- Original path: `docs/arch/future/shared-tsgo-speculative-carrier-publication.md`; Git blob: `d7a2f005eb35f5fca42465ad7775f52ed4902f30`; exact source SHA-256: `0e6b7bace9f23fc8b62a859b4931bf9e52b9c86c1f033ad1e8ebc5de051c480d`.
- Exact retained source: `sources/legacy-architecture-transfers/future/shared-tsgo-speculative-carrier-publication.md`.
- Applicable authority: `H3`, `EPR5`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-SUCCESSOR-DAG-AMENDMENT

- Kind: `context`; source: `successor-dag-amendment.md:1-1`; target: `node:NCK0`; text SHA-256: `9413cba2563db3ebfda5614b0ecd45ba6757581a4f7a20da7341ed2b3dc1d128`.

~~~~markdown
# Rev11 legacy-architecture reconciliation and successor charter pack
~~~~
