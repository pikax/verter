# Exact operative source-clause attachment — FCFG0

Schema: 1. Node: `FCFG0`. Clause count: 2. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-EXP-L1336-8A6345ECF97A

- Kind: `context`; source: `successor-expansion.md:1336-1336`; target: `node:FCFG0`; text SHA-256: `8a6345ecf97ac1e82cff98e4f43558aef5ece5ba8a417505453bfbe0c46bc401`.

~~~~markdown
### `FCFG0.md` — Prettier-compatible formatter configuration translator
~~~~

### SRC-EXP-L1338-E509B0D4A4CD

- Kind: `forbidden`; source: `successor-expansion.md:1338-1343`; target: `node:FCFG0`; text SHA-256: `e509b0d4a4cd40c45e63e6e4bd8dc562e5197dac642017b243a23e8175fd0e89`.

~~~~markdown
**Intent:** translate the captured `CFG0` payload into the exact `FMK0/FMT0` option vocabulary without making base configuration depend on formatter schemas.
**Predecessors:** `FMT0`, `FMK0`, `CFG0`.
**Subblocks:** (1) map pinned Prettier options; (2) define Verter-only formatter settings in separate namespace; (3) implement overrides/ignore/provenance; (4) classify unknown/inapplicable/unsupported values; (5) generate schema/docs/capability cells; (6) differential config and invalidation tests.
**Acceptance:** supported Prettier config resolves identically on locked fixtures; unknown or unsupported options fail truthfully; oxfmt contributes bug evidence only and no second option vocabulary.
**Forbidden:** arbitrary JS config execution in Rust, silent option dropping, formatter rules in `CFG0`, or external formatter invocation.
**Deletion/abort:** delete old formatter-specific config readers after zero-consumer proof; executable configs remain behind an explicit trusted-host input boundary.
~~~~
