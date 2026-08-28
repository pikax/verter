# Exact operative source-clause attachment — LNTCFG0

Schema: 1. Node: `LNTCFG0`. Clause count: 2. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-EXP-L1410-2160B340D728

- Kind: `context`; source: `successor-expansion.md:1410-1410`; target: `node:LNTCFG0`; text SHA-256: `2160b340d728ed65d6de1deb7a295488c48d7dc9d336e57cc09515dd4c8f5f16`.

~~~~markdown
### `LNTCFG0.md` — Verter lint configuration and ecosystem translators
~~~~

### SRC-EXP-L1412-D05896206E2C

- Kind: `forbidden`; source: `successor-expansion.md:1412-1417`; target: `node:LNTCFG0`; text SHA-256: `d05896206e2ccedf2f2fd74563782fc31b54d216cbe926dbb1cbf7e668a01ebd`.

~~~~markdown
**Intent:** own the Verter lint schema and translate captured ecosystem configuration after the exact rule vocabulary exists.
**Predecessors:** `LNT0`, `LRA0`, `CFG0`.
**Subblocks:** (1) versioned `lint` section in `verter.config.jsonc`; (2) exact per-language/per-framework rule namespaces and overrides; (3) static ESLint/TS-ESLint/Vue/Svelte/Stylelint translators; (4) suppression/severity/fix-policy provenance; (5) unknown/inapplicable/external-only/cycle/trust outcomes; (6) schema generation, invalidation, and differential config corpus.
**Acceptance:** Verter-only rules configure without pretending to be ecosystem rules; supported ecosystem configs translate deterministically; unknown rule/option fails closed; profile overrides do not leak across framework releases.
**Forbidden:** arbitrary JS config execution in Rust, silent fallback, a flat cross-framework rules map, translator logic in `CFG0`, or external plugin execution.
**Deletion/abort:** delete duplicate lint config readers only after all consumers move; executable config remains an explicit trusted-host input.
~~~~
