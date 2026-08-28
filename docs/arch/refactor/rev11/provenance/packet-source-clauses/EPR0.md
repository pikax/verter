# Exact operative source-clause attachment — EPR0

Schema: 1. Node: `EPR0`. Clause count: 6. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-LEGACY-EPR-BUNDLE-001

- Kind: `requirement`; source: `legacy-arch-reconciliation.md:166-171`; target: `node:EPR0`; text SHA-256: `4053dc931f7487d7d6a804dac7636b9d3d0ff69e9d73d7ab89d21fbdd66c4d03`.

~~~~markdown
### EPR-BUNDLE-001 — Explicit shipping owner

- Bundled engine bytes must belong to one named package/platform matrix with pinned input, manifest, SBOM, license, provenance, installed-package validation, size/update/rollback policy, and negative rejection elsewhere.
- Existing “never package” guards are changed only through explicit authorization.
- Targets: `EPR0`, optional `EPR3`.
- Source: `docs/arch/future/engine-provisioning-bundled-sidecar-and-shipping-channel.md`, blob `8fdd6d881db77615e09b31f51318fc0254bb27dd`.
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

### SRC-LEGACY-EPR-POLICY-001

- Kind: `requirement`; source: `legacy-arch-reconciliation.md:145-150`; target: `node:EPR0`; text SHA-256: `205a061f7959820aa50934387fb03bf4975026688264a97f25f0e89cca310d93`.

~~~~markdown
### EPR-POLICY-001 — Acquisition and bundling are explicit policy

- Automatic network acquisition and bundled distribution are security/product decisions.
- A valid end state may forbid both.
- Source classes, order, authorization, trust, update, rollback, offline, proxy, enterprise, and privacy behavior are captured policy.
- Targets: `EPR0`.
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
