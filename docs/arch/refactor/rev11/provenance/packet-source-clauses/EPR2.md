# Exact operative source-clause attachment — EPR2

Schema: 1. Node: `EPR2`. Clause count: 6. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-LEGACY-EPR-ATOMIC-INSTALL-001

- Kind: `requirement`; source: `legacy-arch-reconciliation.md:159-164`; target: `node:EPR1`; text SHA-256: `d2853c24330b2281a826c27e2a1d4238c9259f5099a139e991483ca2259d3a83`.

~~~~markdown
### EPR-ATOMIC-INSTALL-001 — Safe managed installation

- Managed install uses private no-follow temp roots, bounded download/extraction, verification before extraction/execution, cross-process locking, atomic rename, and READY written last.
- Partial/corrupt/symlink/reparse/insecure entries are never candidates.
- Targets: `EPR1`, optional `EPR2`.
- Source: `docs/arch/future/engine-provisioning-download-tier.md`, blob `cd6618efb8e1a586caa6842874a1ce5b128469af`.
~~~~

### SRC-LEGACY-EPR-OFFLINE-001

- Kind: `requirement`; source: `legacy-arch-reconciliation.md:188-193`; target: `node:EPR0`; text SHA-256: `12a3f963d95695f053129b2ce845b2ca4c3cd78a8e69653fd73923ce716fbb05`.

~~~~markdown
### EPR-OFFLINE-001 — No hidden network and truthful status

- Forbidden/offline/air-gapped policy makes zero DNS/socket attempts.
- Proxy/custom CA inputs are explicit and secrets are not persisted/logged.
- No engine produces typed NeedInputs/unavailable capability rather than hidden fallback.
- Targets: `EPR0`, `EPR2`, `EPR6`.
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

### SRC-LEGACY-TRANSFER-CD6618EFB8E1

- Kind: `requirement`; source: `legacy-architecture-transfers.md:75-80`; target: `node:EPR0`; text SHA-256: `23d0a0a1d5c5e65e7e981f082e836dbd34814e659995a2b5b9aebd7fb8ca37f8`.

~~~~markdown
### LEGACY-TRANSFER-CD6618EFB8E1

- Original path: `docs/arch/future/engine-provisioning-download-tier.md`; Git blob: `cd6618efb8e1a586caa6842874a1ce5b128469af`; exact source SHA-256: `3c932ef124f15e4f45d66833b7548bf2dd7809c732416b69b1beb106acba41ab`.
- Exact retained source: `sources/legacy-architecture-transfers/future/engine-provisioning-download-tier.md`.
- Applicable authority: `EPR0`, `EPR1`, `EPR2`, `EPR4`, `EPR6`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-SUCCESSOR-DAG-AMENDMENT

- Kind: `context`; source: `successor-dag-amendment.md:1-1`; target: `node:NCK0`; text SHA-256: `9413cba2563db3ebfda5614b0ecd45ba6757581a4f7a20da7341ed2b3dc1d128`.

~~~~markdown
# Rev11 legacy-architecture reconciliation and successor charter pack
~~~~
