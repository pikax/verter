# Exact operative source-clause attachment — EPR4

Schema: 1. Node: `EPR4`. Clause count: 6. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-LEGACY-EPR-RESOLVE-001

- Kind: `requirement`; source: `legacy-arch-reconciliation.md:173-179`; target: `node:EPR4`; text SHA-256: `727a179b0b2a52120e095587dd4aa561d14f30d5a62de3ce7d508deb7f172243`.

~~~~markdown
### EPR-RESOLVE-001 — Deterministic validated selection

- Resolution enumerates only authorized source adapters.
- Every locator is validated before comparison.
- Comparator is explicit, deterministic, versioned, and independent of enumeration order.
- Resolution performs no network or spawn.
- Targets: `EPR4`.
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

### SRC-LEGACY-EXISTING-CACHE-001

- Kind: `requirement`; source: `legacy-arch-reconciliation.md:199-201`; target: `node:G1`; text SHA-256: `ecf95ce060d2ba6503880cd6ec607e851edb63871b62779606fd5ef981cdc27e`.

~~~~markdown
### EXISTING-CACHE-001

Fact/query/result caches use exact structural identity, read-set validation, complete-only admission, singleflight, cancellation, and reclaimable storage. Targets: `G1`, `G2`, `E4`, `H1`, all successor caches. Related source: `docs/arch/fact-based-cache.md`, blob `1f97d9be730193400629485e8c86415b35834f27`.
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
