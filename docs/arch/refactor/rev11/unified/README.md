# Rev11 unified execution DAG

ORC0 uses the superseding 2026-08-28 trusted-local directive: honest-operator local consistency and auditability, one repo-global lifecycle lock/anchor across external runtime roots, deterministic per-node effort tiers, and fresh provider-neutral harness task records. It makes no malicious-owner, cryptographic harness-authenticity, or independent anti-rollback claim. See `APPLICATION.md`.

This additive package remains fail-closed until the J1/ORC0 lifecycle is completed. It does not modify or supersede live Rev11 authority while dormant. C1 is imported from its immutable accepted receipt. J1 is represented separately as `LANDED_GRANDFATHERED` by an exact Git-verified landing receipt for commit `6a6c3c1a83709f7a58918e5b4e3d1eedcbd3ddac`; neither the legacy ledger nor unified v2 falsely calls J1 accepted.

Static authority lives in `authority/`, `charters/`, `contracts/`, `catalogs/`, `schemas/`, `templates/`, and `provenance/`. The three proposal sources remain byte-identical and 2,339 digest-bound context/requirement/acceptance/deletion/forbidden atoms transfer their obligations to real target charters/contracts. Fifty live Rev11 inputs are separately byte/SHA locked. Unverifiable Recovery A/B coverage claims were removed; recovery artifacts are not authority.

State is derived from real Git identities, immutable receipts, operator-attested harness evidence, the append-only amendment chain, and runtime leases stored outside the worktree. A receipt never overrides a blocker. Historical executable r3-r6 evidence is digest-valid but audit-only and cannot satisfy current acceptance. BR0 remains the source-canonical sole successor entry.

Use an external runtime directory:

```text
node tools/programctl.mjs phase --runtime-root PATH
node tools/programctl.mjs frontier --runtime-root PATH
node tools/programctl.mjs explain ID --runtime-root PATH
node tools/programctl.mjs admit ID --holder NAME --candidate-ref refs/heads/BRANCH --gate-runner NAME --reviewer LENS=NAME --runtime-root PATH
node tools/programctl.mjs packet ID --holder NAME --lease-id LEASE_ID --runtime-root PATH
node tools/programctl.mjs dispatch ID --holder NAME --lease-id LEASE_ID --runtime-root PATH
node tools/programctl.mjs candidate-finalize ID --holder NAME --lease-id LEASE_ID --runtime-root PATH
node tools/programctl.mjs gate-run ID --scope candidate --integration-sha SHA --holder NAME --lease-id LEASE_ID --runtime-root PATH
node tools/programctl.mjs review-run ID --lens LENS --custody-binding review-capability:SHA256 --holder NAME --lease-id LEASE_ID --runtime-root PATH
```

Repeat `--reviewer` once for every lens in the node's exact review profile. Every operational command runs phase-aware strict authority validation. `admit` atomically checks READY state, same-node exclusion, canonical static conflict domains, and resource capacity before acquiring a lease. Gate and review PASS evidence cannot be imported: it is created only by the canonical execution runners, and review acceptance remains blocked without externally ratified immutable capabilities. See `APPLICATION.md` for activation, lease, amendment, and acceptance workflows.
