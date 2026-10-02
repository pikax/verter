---
name: type-resolution
description: "Cross-file type resolution: type solver, ShallowFileState, ExternalTypeFrontier, canonical cache rules, macro traversal, prepared declarations"
---

# Type Resolution

Read the references below on demand for the mechanism being changed. The shared
semantic query path, shallow indexing, canonical dependency cache and lazy
projection contracts remain the authority for type resolution.

Each owner updates only its pre-registered reference file. Corrections to another
owner's text go in the correcting owner's file as superseding notes; L4's
documentation closure folds them in. The index is complete and stays unchanged
as those owners land.

| Reference | Topic | Owning task |
| --- | --- | --- |
| [test-and-guard-layout.md](references/test-and-guard-layout.md) | Frontier tests and guard layout | SKR-ENGINE-TESTS |
| [class-flow.md](references/class-flow.md) | Prepared class surfaces | SKR-CLASS-1 |
| [class-flow-solve.md](references/class-flow-solve.md) | Source-ordered class evaluation effects | SKR-CLASS-3 |
| [relation-ownership.md](references/relation-ownership.md) | Semantic dispatch and relation ownership | SKR-OWN-1 |
| [snapshot-freshness.md](references/snapshot-freshness.md) | Reader-owned snapshot freshness | SKR-RET-F |
| [declaration-headers.md](references/declaration-headers.md) | Indexed declarations and headers | SKR-P-HEADERS |
| [inference-owner.md](references/inference-owner.md) | Reverse recovery and inference ownership | SKR-INFER-1 |
| [continuation-runtime.md](references/continuation-runtime.md) | Query identity and continuation runtime | SKR-RUNTIME |
| [conditional-decisions.md](references/conditional-decisions.md) | Conditional decisions and path projection | SKR-COND |
| [semantic-observe.md](references/semantic-observe.md) | Derivation, provenance and observability | SKR-OBS |
| [parallel-execution.md](references/parallel-execution.md) | Independent semantic demand execution | SKR-PARALLEL |
| [request-budget.md](references/request-budget.md) | Request budgets and safety fuses | SKR-FUSE |
| [limits-and-retention.md](references/limits-and-retention.md) | Cache population, limits and retention | SKR-LIMITS |
| [cache-authority.md](references/cache-authority.md) | Project-Global Cache Authority (post-rewrite) | TRS0 |
| [canonical-dependency-cache.md](references/canonical-dependency-cache.md) | Canonical Dependency Cache Rule | TRS0 |
| [semantic-heuristic-prevention.md](references/semantic-heuristic-prevention.md) | Semantic Heuristic Prevention (CRITICAL) | TRS0 |
| [typed-degradation.md](references/typed-degradation.md) | Typed Degradation And Completeness Contract (CRITICAL) | TRS0 |
| [query-contracts.md](references/query-contracts.md) | Query Mode Contract | TRS0 |
| [navigator-boundary.md](references/navigator-boundary.md) | Navigator Boundary Contract | TRS0 |
| [generic-navigation-and-expansion.md](references/generic-navigation-and-expansion.md) | Generic Navigation And Expansion Contract | TRS0 |
| [worked-examples.md](references/worked-examples.md) | Worked Examples | TRS0 |
| [shallow-file-state-and-frontier.md](references/shallow-file-state-and-frontier.md) | Shallow File State and Frontier Engine | TRS0 |
| [retained-carriers.md](references/retained-carriers.md) | Retired solver surface and retained carriers | TRS0 |
| [declaration-merging.md](references/declaration-merging.md) | Declaration Merging (CRITICAL) | TRS0 |
| [declaration-augmentation.md](references/declaration-augmentation.md) | Declaration Augmentation (CRITICAL) | TRS0 |
| [global-names.md](references/global-names.md) | Global Names (one resolution path) | TRS0 |
| [cross-file-compiler-integration.md](references/cross-file-compiler-integration.md) | Cross-File Type Resolution (Compiler Integration) | TRS0 |
| [macro-type-traversal.md](references/macro-type-traversal.md) | Macro Type Traversal Rule | TRS0 |
| [typed-ir-resolver.md](references/typed-ir-resolver.md) | Typed-IR-Only Resolver Rule (CRITICAL) | TRS0 |
| [carrier-contracts.md](references/carrier-contracts.md) | PARSELOWER Carrier Contracts (handle-migration foundation) | TRS0 |
| [flow-return-substrate.md](references/flow-return-substrate.md) | Flow-Return Substrate (U6) | TRS0 |
| [tagged-component-publication.md](references/tagged-component-publication.md) | Tagged-Component Publication: Root Admission Commits, Members Backfill | TRS0 |
| [template-class-facts.md](references/template-class-facts.md) | Template class fact demand | TRS0 |
| [reactive-wrapper-demand.md](references/reactive-wrapper-demand.md) | Reactive-wrapper demand (shared vocabulary) | TRS0 |
| [signature-discovery.md](references/signature-discovery.md) | Signature discovery (`SignaturesOfType` / `ReadSignatureResult`) | TRS0 |
