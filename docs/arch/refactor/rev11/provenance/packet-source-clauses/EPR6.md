# Exact operative source-clause attachment — EPR6

Schema: 1. Node: `EPR6`. Clause count: 5. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-LEGACY-EPR-ACTIVATE-001

- Kind: `requirement`; source: `legacy-arch-reconciliation.md:181-186`; target: `node:EPR5`; text SHA-256: `8ccd7625df8c8294d21aeb4f157e3c5860e4ea6c4c42c2f0cb6670dde7f25bc5`.

~~~~markdown
### EPR-ACTIVATE-001 — Healthy applied binding is availability

- Activation revalidates the selection handoff, spawns/attaches under bounded control, performs version/protocol/capability handshake, and atomically publishes a project-scoped ProviderEpoch.
- Process existence or configured mode is not availability.
- Swap/restart/crash/rollback is stale-safe and invalidates old handles.
- Targets: `EPR5`, `EPR6`.
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
