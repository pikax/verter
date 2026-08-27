# Exact operative source-clause attachment — HWC3

Schema: 1. Node: `HWC3`. Clause count: 2. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-EXP-L1123-C31FCA4D3E76

- Kind: `context`; source: `successor-expansion.md:1123-1123`; target: `node:HWC3`; text SHA-256: `c31fca4d3e7678f4252f61faf8bc0ba3fa9f7728fe4b68d1da8798de92156c23`.

~~~~markdown
### `HWC3.md` — Web Component standards model, registry analysis, and CEM
~~~~

### SRC-EXP-L1125-363850D1D58A

- Kind: `forbidden`; source: `successor-expansion.md:1125-1130`; target: `node:HWC3`; text SHA-256: `363850d1d58af5711089a55779375b5691c212c3285c0bae8e7b1599f5d5275f`.

~~~~markdown
**Intent:** solely implement standards-fact projection, registry analysis, and CEM import/export over TypeInfo and the workspace index, conforming to the `CEF0` contract.
**Predecessors:** `HWC2`, `CEF0`.
**Subblocks:** (1) consume the `CEF0` standards/CEM contract; (2) project custom-element declarations, registrations, registry scopes, properties/attributes/events/slots/methods/parts/CSS custom properties from neutral or vertical-owned evidence; (3) implement `customElements.define` and statically admitted registry analysis; (4) implement declaration↔registration↔consumer association; (5) implement CEM import/export with provenance; (6) ambiguity/scoped-registry/package fixtures.
**Acceptance:** Vue/Svelte/Lit/Stencil-owned evidence can be projected into HWC3-produced standards facts without HWC3 knowing framework semantics; consumers obtain exact/partial/ambiguous results honestly; CEM round-trip preserves admitted facts and provenance under `CEF0`.
**Forbidden:** runtime execution, global registry certainty, class-inheritance heuristics as authority, or CEM-owned types.
**Deletion/abort:** migrate only neutral standards rows/adapters; shared legacy WCP schema/registry deletion belongs solely to `CEC0`; abort static reachability claims that cannot survive scoped/dynamic registry counterexamples.
~~~~
