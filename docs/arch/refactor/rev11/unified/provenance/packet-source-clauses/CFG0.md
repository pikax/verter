# Exact operative source-clause attachment — CFG0

Schema: 1. Node: `CFG0`. Clause count: 2. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-EXP-L941-B5B0A0D1C8C8

- Kind: `context`; source: `successor-expansion.md:941-941`; target: `node:CFG0`; text SHA-256: `b5b0a0d1c8c800105458c14bc4e481778319bedbe3db7480e0d422ff602a5e7f`.

~~~~markdown
### `CFG0.md` — Declarative Verter and captured ecosystem configuration
~~~~

### SRC-EXP-L943-7B6DAF3DA386

- Kind: `forbidden`; source: `successor-expansion.md:943-948`; target: `node:CFG0`; text SHA-256: `7b6daf3da3869fb80859a6a847b32a87d95b4f8a54cd9055e07310c4b759d1a4`.

~~~~markdown
**Intent:** establish the hermetic base configuration/read-set authority without depending on downstream lint-rule or formatter-option schemas.
**Predecessors:** `CAT0`.
**Subblocks:** (1) versioned `verter.config.jsonc` envelope; (2) root/extends/override/profile precedence and provenance; (3) typed opaque product-config sections whose schemas remain downstream-owned; (4) unknown top-level/cycle/trust/NeedInputs outcomes; (5) config read sets and invalidation; (6) NAPI/WASM prepared-input contracts.
**Acceptance:** precedence is deterministic across monorepo/nested configs; unknown framework release and top-level fields fail closed; product payloads retain exact source/provenance for later translators; changing irrelevant config does not invalidate unrelated profiles.
**Forbidden:** arbitrary JS execution in core, ambient home/global config, one flat framework section, silent option dropping, or conflating config translation with external tool execution.
**Deletion/abort:** migrate only base/profile readers; product readers are deleted by their downstream translator cutovers; rescope executable ecosystem configuration behind the separately trusted host boundary.
~~~~
