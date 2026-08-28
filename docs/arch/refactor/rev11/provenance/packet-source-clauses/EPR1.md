# Exact operative source-clause attachment — EPR1

Schema: 1. Node: `EPR1`. Clause count: 5. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-LEGACY-EPR-ATOMIC-INSTALL-001

- Kind: `requirement`; source: `legacy-arch-reconciliation.md:159-164`; target: `node:EPR1`; text SHA-256: `d2853c24330b2281a826c27e2a1d4238c9259f5099a139e991483ca2259d3a83`.

~~~~markdown
### EPR-ATOMIC-INSTALL-001 — Safe managed installation

- Managed install uses private no-follow temp roots, bounded download/extraction, verification before extraction/execution, cross-process locking, atomic rename, and READY written last.
- Partial/corrupt/symlink/reparse/insecure entries are never candidates.
- Targets: `EPR1`, optional `EPR2`.
- Source: `docs/arch/future/engine-provisioning-download-tier.md`, blob `cd6618efb8e1a586caa6842874a1ce5b128469af`.
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

### SRC-LEGACY-TRANSFER-8FDD6D881DB7

- Kind: `requirement`; source: `legacy-architecture-transfers.md:68-73`; target: `node:EPR0`; text SHA-256: `7a95fcce3e5b3b258d1207ddd262c50b579888081ca6831479bf3f685fc38290`.

~~~~markdown
### LEGACY-TRANSFER-8FDD6D881DB7

- Original path: `docs/arch/future/engine-provisioning-bundled-sidecar-and-shipping-channel.md`; Git blob: `8fdd6d881db77615e09b31f51318fc0254bb27dd`; exact source SHA-256: `e9c51872088b2637bc69a0e1c45a49f907dede39c3322acfb1857771be8a42d9`.
- Exact retained source: `sources/legacy-architecture-transfers/future/engine-provisioning-bundled-sidecar-and-shipping-channel.md`.
- Applicable authority: `EPR0`, `EPR1`, `EPR3`, `EPR4`, `EPR6`.
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
