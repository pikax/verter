<!-- unified-charter-v2
id=SM0
name=Static source-module fact and provenance contract
predecessors=PM2,CFG0
phase=expansion
train=expansion.source-modules
product=source_modules
kind=contract
semantic_role=delivery
class=successor
owner=expansion.source-modules:static source-module facts, provenance, read sets, and membership authority
conflict_domains=source_module_facts
resource_class=docs-light
gate_profile=docs-domain
review_profile=architecture-3
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=high
verification_effort_default=high
confirmation_effort_min=high
confirmation_effort_default=high
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/expansion-source-modules/SM0.md
size=M
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# SM0 — Static source-module fact and provenance contract

## Independently acceptable outcome and owners

Define the sole authority for statically captured build/source-module facts consumed by semantics and products. Current knowledge is duplicated in Vite/unplugin, LSP aliases, TypeScript-plugin resolution, and feature-local heuristics. Final ownership is typed `SourceModuleFactSet`; bundling and HMR remain BND ownership and package resolution remains PM ownership.

## Surfaces, APIs, and predecessor contracts

Expected surfaces: `verter_protocol`, `verter_workspace`, `verter_session`, `packages/unplugin`, and `packages/typescript-plugin`. APIs consume a kernel-owned source-module-environment identity and define `SourceModuleFactSet`, `SourceModuleFactProvenance`, `SourceModuleResolutionContribution`, `ModuleQueryKind`, `GlobPattern`, `GlobMembership`, and `EnvironmentBinding`. `PM2` supplies canonical package/module resolution and its typed contribution input; `CFG0` supplies typed captured config and read sets. A missing identity type is a VID0/CAT0 amendment, not SM scope.

## Binding architecture and subblocks

1. Define aliases, asset/query modules, URL/worker forms, glob facts, environment bindings, virtual provenance, and exact unsupported outcomes.
2. Separate source-visible semantic facts from bundler execution/artifacts.
3. Define JS-host capture: `packages/*` may run host hooks and translate results, while Rust/WASM never executes arbitrary user config.
4. Freeze identity/read-set/invalidation and consumer ownership matrices.

SM owns generic captured host aliases and source-module forms. A later project profile may supply a `ProjectProfileResolutionContribution`; SM may transport/normalize it into the PM contribution boundary but may not interpret Nuxt/SvelteKit meaning or claim the contribution as an SM-authored fact.

See `contracts/product-surface-expansion.md` for shared cache, migration, proof, and performance rules.

## Migration, deletions, forbidden designs, and acceptance

This contract changes no production code and deletes nothing. It forbids a second package resolver, executing Vite plugins in core, treating arbitrary virtual-module bytes as semantic truth, regex extraction from executable config, and storing unversioned env/glob results.

- **SM0-AC1:** every live alias/query/asset/glob/env heuristic maps to SM1–SM3 or an explicit external owner.
- **SM0-AC2:** planted unproven virtual fact and config-without-read-set are rejected.
- **SM0-AC3:** Vue, Svelte, Nuxt, and SvelteKit fixtures fit without framework fields in the generic fact schema.
- Abort if one fact family requires independent execution authority or a public security boundary.
- Verify strict DAG validation, docs build, and `architecture-3`; production LOC is zero.

SM1 and SM2 consume this contract. Ledger presence is the completion fact.
