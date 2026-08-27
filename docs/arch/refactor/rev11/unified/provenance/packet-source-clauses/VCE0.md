# Exact operative source-clause attachment — VCE0

Schema: 1. Node: `VCE0`. Clause count: 2. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-EXP-L1186-D14AAE5B09BA

- Kind: `context`; source: `successor-expansion.md:1186-1186`; target: `node:VCE0`; text SHA-256: `d14aae5b09ba9baea3410f40fb79fc02598adf83c342f08f59e4f98517a0795f`.

~~~~markdown
### `VCE0.md` — Vue Custom Element producer and consumer retrofit
~~~~

### SRC-EXP-L1188-A842A59B60C6

- Kind: `forbidden`; source: `successor-expansion.md:1188-1193`; target: `node:VCE0`; text SHA-256: `a842a59b60c6d8bd37760797cf3fa29b2f29f221ee8c91bcdb3e390bf7296ecd`.

~~~~markdown
**Intent:** make the accepted Vue release an explicit CE producer and consumer rather than a generic component approximation.
**Predecessors:** `HWC3`, `EAK1`, `SKL3`.
**Subblocks:** (1) prove `defineCustomElement`/`defineSSRCustomElement` roles; (2) treat `.ce.vue` and captured plugin config as mode candidates, not tag identity; (3) model CE-specific prop/attribute/event/slot/style/root behavior; (4) associate explicit registrations; (5) contribute Vue-owned evidence to HWC3, which solely projects standards facts and CEM output conforming to `CEF0`, then test TypeInfo/ComponentInfo/CEM results; (6) add template/TS IDE, diagnostic/action, source-map, and performance fixtures.
**Acceptance:** ordinary Vue component and CE build variants remain distinct; alias/re-export activation works; filename-only and userland same-spelling cases fail closed; Vue consumer `isCustomElement` policy is captured and invalidated correctly.
**Forbidden:** deriving registration from compile output, claiming runtime registration, treating `.ce.vue` as a tag declaration, vertical-owned CEM serialization, or implementing a private formatter. CE mode does not change formatter semantics; `.ce.vue` is covered by ordinary Vue syntax fixtures in `FMTV0`.
**Deletion/abort:** delete only named Vue profile rows/adapters after zero-consumer proof; shared schema/registry deletion belongs to `CEC0`; abort if the exact Vue release oracle differs from locked mode semantics.
~~~~
