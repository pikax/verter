# Rev11 live authority

This directory is the single live Rev11 authority root. The former `unified/` package was promoted here at the trusted-local ORC0 cutover. The prior Rev11 tree is retained byte-for-byte under `backup/` as non-authoritative, read-only history; live tools and discovery never read it. Pre-cutover live inputs are bound to their original Git objects in `provenance/live-source-lock.toml`.

ORC0 provides honest-operator local consistency and auditability: one repo-global evidence transaction mutex/anchor across external runtime roots, immutable candidate/review targets, deterministic per-role effort, risk-scaled fresh provider-neutral harness tasks, current-round acceptance, and explicit recovery. The mutex grants no work ownership, conflict, capacity, or scheduling right; the maintainer coordinates those concerns. It does not claim malicious-owner resistance, cryptographic harness authenticity, or independent anti-rollback. Runtime evidence stays external, under the stable `verter-rev11-unified-runtime` namespace.

Static authority lives in `authority/`, `charters/`, `contracts/`, `catalogs/`, `schemas/`, `templates/`, and `provenance/`. Historical preactivation r3-r6 review bytes remain digest-valid but audit-only. The rejected preactivation ORC0 R1 is preserved append-only and does not consume a live review/fix cycle.

Core commands, from the repository root:

```text
node docs/arch/refactor/rev11/tools/programctl.mjs phase --runtime-root PATH
node docs/arch/refactor/rev11/tools/programctl.mjs frontier --runtime-root PATH
node docs/arch/refactor/rev11/tools/programctl.mjs explain ID --runtime-root PATH
node docs/arch/refactor/rev11/tools/programctl.mjs admit ID --holder NAME --candidate-ref refs/heads/BRANCH --runtime-root PATH
node docs/arch/refactor/rev11/tools/programctl.mjs dispatch ID --holder NAME --lease-id LEASE_ID --runtime-root PATH
node docs/arch/refactor/rev11/tools/programctl.mjs candidate-finalize ID --holder NAME --lease-id LEASE_ID --runtime-root PATH
node docs/arch/refactor/rev11/tools/programctl.mjs harness-record --role ROLE --round-id ROUND --lease-id LEASE --holder NAME --task TASK --provider PROVIDER --model MODEL --effort TIER --prompt FILE --report FILE --runtime-root PATH
node docs/arch/refactor/rev11/tools/programctl.mjs round-accept ROUND --holder NAME --runtime-root PATH
```

Admission emits the packet and implementation/review/verification/confirmation brief paths in one machine-readable result. The legacy `lease_id` field in these commands is only an opaque round handle. Rebase and squash before freezing a candidate, and fast-forward the exact reviewed commit. Do not automatically launch another train after frontier changes unless the maintainer's launch prompt explicitly authorized continuous ordering. See `APPLICATION.md`, `contracts/orchestration.md`, and `/multi-agent-orchestration`.
