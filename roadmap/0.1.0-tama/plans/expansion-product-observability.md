# Plan: expansion.product-observability

Owner: `expansion.product-observability` — product observability, feature exposure and reproducibility. Constitution: `contracts/product-experience.md` (ratified by DX0). Product: `dx_product`.

This plan is the train map for the product-observability nodes. Each node's binding content is its charter under `charters/expansion-product-observability/`; this file records order, boundaries and status only. Implementation state is resolved solely by the implementation ledger (`authority/state/implemented.toml`), never by this document.

## Node map

| Node | Outcome | Predecessors | Status |
| --- | --- | --- | --- |
| DX0 | Cross-surface feature preservation and exposure constitution | ORC0, STP0 | implemented (this delivery) |
| DX1 | Exposure descriptor contract and promotion rule | DX0, PUB0 | pending |
| DX1G | Generated exposure bindings and coverage report | DX1 | pending |
| DX2 | Bounded inspection request and snapshot facade | DX1, H3, TIF1, IDX0 | pending |
| DX3 | Semantic and compiler evidence projections | DX2 | pending |
| DX3T | Query, dependency and compiler trace projection | DX2 | pending |
| DX4 | Authenticated local companion transport and trust boundary | DX1, CLI1, EPR1 | pending |
| DX5 | Native companion operation adapter and execution parity | DX4, DX2, CLI4 | pending |
| DX6 | Portable reproduction capsule, export limits and redaction | DX1, DX2 | pending |
| DX6R | Deterministic replay core | DX6, DX2 | pending |
| DX7 | Local doctor, support export and actionable failure explanations | DX1, DX6, EPR5 | pending |
| DX8 | Inspection and reproduction cross-surface terminal | DX3, DX3T, DX5, DX6, DX6R, DX7, DX1G | pending |

Cross-train consumers of DX0's products include BWH0, COX0, ED0, EPR0, JBT0, LSO0, LSPX0, PG0, PM0, PUB0, RFX0, SVI0, TIF1 and VSC0; they bind through the receiving amendments in `contracts/product-experience.md` §13 and their own charters, not by importing this train.

## Boundaries

- **DX0 → DX1.** DX0 ratifies the obligation model (FeatureExposureContract, HostExecutionClass, ProductReceiptBasis, rails, inventory). DX1 turns it into the exposure descriptor contract and the executable-promotion rule; nothing in DX0 pre-certifies DX1 code.
- **DX1 → DX2.** The bounded inspection facade (DX2) consumes registered descriptors; its future module homes are `crates/verter_protocol/src/inspection`, `crates/verter_session/src/inspection`, `packages/language-shared/src/inspection`, registered by DX2, not by DX0.
- **DX4/DX5** are the only sanctioned browser→native route: authenticated, explicitly selected local companion execution. The universal browser proxy and silent uploads stay rejected.
- **DX6/DX6R** own reproduction capsules and deterministic replay; the receipt basis ratified in DX0 is the basis a capsule must reproduce.
- **DX8** is the terminal that proves exposure end-to-end across surfaces; PG0's workbench and the playground consume its result.

## Rules inherited by every node in this train

- TypeScript stays the semantic-rail authority; inspection answers are labelled and never substitute semantic answers (DX0-AC3).
- Native-only operations in browser surfaces need explicit local-native execution or a registered blocked gap (DX0-AC2, DX0-AC-EXPOSURE).
- No second semantic/type/project engine inside a client; no hidden source uploads; no stale publication; completeness states stay distinct.
- Every operation this train exposes supplies its DX1 registration, executable adapter and replay case before promotion.
