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

| Reference | Topic | Owner inventory |
| --- | --- | --- |
| [test-and-guard-layout.md](references/test-and-guard-layout.md) | Frontier tests and guard layout | SKR-ENGINE-TESTS |
| [class-flow.md](references/class-flow.md) | Prepared class surfaces | SKR-CLASS-1 |
| [class-flow-solve.md](references/class-flow-solve.md) | Source-ordered class evaluation effects | SKR-CLASS-3 |
| [relation-ownership.md](references/relation-ownership.md) | Semantic dispatch and relation ownership | relation-evidence.md |
| [counted-node-ownership.md](references/counted-node-ownership.md) | Counted node lifetime (the sole production authority), internal delivery, sealed public carriers and node capabilities, document close and sidecar cleanup | counted-ownership-substrate.md, public-semantic-carriers.md, lease-owned-graph-lifetime.md |
| [snapshot-freshness.md](references/snapshot-freshness.md) | Reader-owned snapshot freshness | SKR-RET-F |
| [declaration-headers.md](references/declaration-headers.md) | Indexed declarations and headers | SKR-P-HEADERS |
| [inference-owner.md](references/inference-owner.md) | Reverse recovery and inference ownership | inference-owner.md |
| [checker-diagnostics.md](references/checker-diagnostics.md) | Checker diagnostic recipes, occurrences and their binding | checker-diagnostic-recipes.md |
| [continuation-runtime.md](references/continuation-runtime.md) | Query identity and continuation runtime | continuation-runtime-polling.md |
| [conditional-decisions.md](references/conditional-decisions.md) | Conditional decisions and path projection | conditional-decisions.md |
| [semantic-observe.md](references/semantic-observe.md) | Derivation, provenance and observability | SKR-OBS |
| [parallel-execution.md](references/parallel-execution.md) | Independent semantic demand execution | SKR-PARALLEL |
| [request-budget.md](references/request-budget.md) | Request budgets and safety fuses | SKR-FUSE |
| [limits-and-retention.md](references/limits-and-retention.md) | Cache population, limits and retention | SKR-LIMITS |
| [cache-authority.md](references/cache-authority.md) | Project-Global Cache Authority (post-rewrite) | type-resolution-split |
| [canonical-dependency-cache.md](references/canonical-dependency-cache.md) | Canonical Dependency Cache Rule | type-resolution-split |
| [semantic-heuristic-prevention.md](references/semantic-heuristic-prevention.md) | Semantic Heuristic Prevention (CRITICAL) | type-resolution-split |
| [typed-degradation.md](references/typed-degradation.md) | Typed Degradation And Completeness Contract (CRITICAL) | type-resolution-split |
| [query-contracts.md](references/query-contracts.md) | Query Mode Contract | type-resolution-split |
| [navigator-boundary.md](references/navigator-boundary.md) | Navigator Boundary Contract | type-resolution-split |
| [generic-navigation-and-expansion.md](references/generic-navigation-and-expansion.md) | Generic Navigation And Expansion Contract | type-resolution-split |
| [worked-examples.md](references/worked-examples.md) | Worked Examples | type-resolution-split |
| [shallow-file-state-and-frontier.md](references/shallow-file-state-and-frontier.md) | Shallow File State and Frontier Engine | type-resolution-split |
| [retained-carriers.md](references/retained-carriers.md) | Retired solver surface and retained carriers | type-resolution-split |
| [declaration-merging.md](references/declaration-merging.md) | Declaration Merging (CRITICAL) | type-resolution-split |
| [declaration-augmentation.md](references/declaration-augmentation.md) | Declaration Augmentation (CRITICAL) | type-resolution-split |
| [global-names.md](references/global-names.md) | Global Names (one resolution path) | type-resolution-split |
| [cross-file-compiler-integration.md](references/cross-file-compiler-integration.md) | Cross-File Type Resolution (Compiler Integration) | type-resolution-split |
| [macro-type-traversal.md](references/macro-type-traversal.md) | Macro Type Traversal Rule | type-resolution-split |
| [typed-ir-resolver.md](references/typed-ir-resolver.md) | Typed-IR-Only Resolver Rule (CRITICAL) | type-resolution-split |
| [carrier-contracts.md](references/carrier-contracts.md) | PARSELOWER Carrier Contracts (handle-migration foundation) | type-resolution-split |
| [flow-return-substrate.md](references/flow-return-substrate.md) | Flow-Return Substrate (U6) | type-resolution-split |
| [tagged-component-publication.md](references/tagged-component-publication.md) | Tagged-Component Publication: Root Admission Commits, Members Backfill | type-resolution-split |
| [template-class-facts.md](references/template-class-facts.md) | Template class fact demand | type-resolution-split |
| [reactive-wrapper-demand.md](references/reactive-wrapper-demand.md) | Reactive-wrapper demand (shared vocabulary) | type-resolution-split |
| [signature-discovery.md](references/signature-discovery.md) | Signature discovery (`SignaturesOfType` / `ReadSignatureResult`) | type-resolution-split |
