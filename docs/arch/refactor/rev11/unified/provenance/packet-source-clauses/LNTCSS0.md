# Exact operative source-clause attachment — LNTCSS0

Schema: 1. Node: `LNTCSS0`. Clause count: 2. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-EXP-L1455-73FDBB1ABC4C

- Kind: `context`; source: `successor-expansion.md:1455-1455`; target: `node:LNTCSS0`; text SHA-256: `73fdbb1abc4ccf4c5074c24130febe41436d83c6d2ec7917714d73983e4a9968`.

~~~~markdown
### `LNTCSS0.md` — CSS and Stylelint compatibility pack
~~~~

### SRC-EXP-L1457-AA8464B44DEF

- Kind: `forbidden`; source: `successor-expansion.md:1457-1462`; target: `node:LNTCSS0`; text SHA-256: `aa8464b44defd9f29688c0a48d39f6a89f1cf2ea9e7d51722c225dc432b9fac6`.

~~~~markdown
**Intent:** close admitted CSS-family/Stylelint rule cells independently from formatter and framework packs.
**Predecessors:** `LNT2`.
**Subblocks:** (1) CSS correctness cells; (2) accessibility/security/performance cells; (3) admitted SCSS/Less cells; (4) Stylelint config/suppression parity; (5) safe fixes/actions; (6) differential false-positive, zero-work, and performance corpus.
**Acceptance:** every rule states exact language applicability; unsupported dialect/plugin rules route only through explicit external policy; fixes remain separate from formatting.
**Forbidden:** invoking Stylelint in Rust, claiming all plugins, framework-selector semantics in base CSS, or format-as-fix.
**Deletion/abort:** delete only named CSS rule rows after parity; shared registry deletion belongs to `LNT3`; unsupported cells remain truthful and independent.
~~~~
