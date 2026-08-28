# Exact operative source-clause attachment — CLI5

Schema: 1. Node: `CLI5`. Clause count: 2. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-EXP-L1529-3E7B56FCB3D0

- Kind: `context`; source: `successor-expansion.md:1529-1529`; target: `node:CLI5`; text SHA-256: `3e7b56fcb3d0578173273fc016e95dd77ff9944857639063765e4c70f4da3fb7`.

~~~~markdown
### `CLI5.md` — Base packaging, watch mode, compatibility wrappers, and promotion
~~~~

### SRC-EXP-L1531-32D48006AA6D

- Kind: `forbidden`; source: `successor-expansion.md:1531-1536`; target: `node:CLI5`; text SHA-256: `32d48006aa6d47838b5849eee1692825a88616525d33543027e69bf6af736160`.

~~~~markdown
**Intent:** package and independently promote the base executable without waiting for formatter, lint, or future verticals.
**Predecessors:** `CLI2`, `CLITS0`, `CLIC0`, `CLI4`, `PER0`.
**Subblocks:** (1) native platform package matrix and integrity/provenance; (2) npm `@verter/cli` install/dispatch; (3) bounded watch/incremental session reuse; (4) convert named old binaries to thin wrappers over the same executable/service registry; (5) retain wrappers for one explicitly named published release with telemetry/deprecation receipt; (6) cold/warm/RSS/cancellation/signal/CI tests, generated command matrix, docs, and exact-candidate reviews.
**Acceptance:** clean installs work on every locked platform; commands advertise only available services; watch equals repeated fresh results and plateaus memory; wrappers execute the same implementation; base CLI promotes independently of fmt/lint/Astro/Qwik/project profiles.
**Forbidden:** downloading unverified binaries, separate alias implementations, hidden daemon state, or withholding CLI release for incomplete Astro/Qwik/project profiles.
**Deletion/abort:** do not delete compatibility wrappers here; a later charter may delete them only after the named published-release receipt and zero-consumer/generated-reference proof. A failing platform remains explicitly unsupported rather than receiving an unverified fallback.
~~~~
