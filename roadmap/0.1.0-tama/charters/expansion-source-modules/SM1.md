<!-- unified-charter-v2
id=SM1
name=Vite aliases, assets, queries, URL, and environment facts
predecessors=SM0
phase=expansion
train=expansion.source-modules
product=source_modules
kind=implementation
semantic_role=delivery
class=successor
owner=expansion.source-modules:static source-module facts, provenance, read sets, and membership authority
conflict_domains=source_module_facts,project_model
resource_class=rust-mixed
gate_profile=canonical
review_profile=semantic-3
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
charter=charters/expansion-source-modules/SM1.md
size=M
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# SM1 — Vite aliases, assets, queries, URL, and environment facts

## Independently acceptable outcome and owners

Produce canonical static facts for Vite-compatible aliases, asset imports, `?raw`/`?url`/worker queries, `new URL(..., import.meta.url)`, `import.meta.env`, and host-provided virtual provenance. Current owners are JS/plugin and LSP/TS-plugin heuristics; final owner is the SM fact service with typed JS capture adapters.

## Surfaces, APIs, and predecessor contract

Expected surfaces span the source-module DTO/service in Rust plus capture adapters in `packages/unplugin` and `packages/typescript-plugin`. APIs: `HostAliasFact`, `SourceModuleResolutionContribution`, `AssetModuleFact`, `ModuleQueryFact`, `UrlAssetFact`, `WorkerModuleFact`, `EnvironmentBinding`, `VirtualModuleProvenance`. `SM0` supplies schema, trust boundary, and ownership matrix. `HostAliasFact` covers aliases captured from the admitted host; it is distinct from framework-owned Nuxt/SvelteKit alias contributions added later.

## Binding architecture and subblocks

1. Capture normalized Vite resolve/config facts with exact tool/plugin/config epochs and read sets.
2. Resolve only statically knowable literal and admitted pattern forms; dynamic forms return typed unsupported/NeedInputs.
3. Publish type/shape/provenance contributions without executing bundler work during semantic queries.
4. Integrate completion/navigation/checker/compiler consumers through the fact API, leaving HMR and emitted bytes to BND.

Fact identity includes project/source-module environment, importer, raw specifier/query, conditions, capture epoch, and relevant files/env keys. Irrelevant env/config changes do not invalidate. Shared laws apply.

## Migration, deletions, forbidden designs, and acceptance

Characterize each consumer, introduce facts, shadow for measurement only, switch, and delete matching aliases/query/env heuristics. Forbid ambient `process.env`, arbitrary plugin execution in Rust/WASM, query stripping before identity, and claiming dynamic expressions static.

- **SM1-AC1:** exact alias/asset/query/URL/worker/env positive and negative matrix passes in Vue and Svelte projects.
- **SM1-AC2:** planted query-loss, importer-loss, or stale capture epoch fails.
- **SM1-AC3:** incremental config/env/asset changes equal fresh; unsupported/partial facts never warm.
- **SM1-AC4:** warm semantic requests execute no bundler hook and perform bounded fact lookups.
- Abort if glob enumeration enters (SM2) or output/HMR behavior enters (BND).
- Verify focused Rust/TS source-module suites, canonical gate, and `semantic-3`.

SM2 consumes these facts. Ceiling: 800 LOC, 8 files, 2 packages; ledger presence records completion.
