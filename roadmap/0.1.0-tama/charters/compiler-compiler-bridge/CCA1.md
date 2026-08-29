<!-- unified-charter-v2
id=CCA1
name=Compiler capability cutover convergence join
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=convergence
semantic_role=convergence
class=compiler
predecessors=CCA1T4
owner=compiler.compiler-bridge:typed capability cutover convergence proof
conflict_domains=compiler_execution,capability_catalog
resource_class=docs-light
review_profile=public-3
gate_profile=docs-domain
implementation_effort_min=medium
implementation_effort_default=medium
review_effort_min=high
review_effort_default=high
verification_effort_min=medium
verification_effort_default=medium
confirmation_effort_min=medium
confirmation_effort_default=medium
size=S
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/compiler-compiler-bridge/CCA1.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1 — Compiler capability cutover convergence join

## Independently acceptable outcome

Confirm the compiler bridge has converged on the immutable catalog and five typed capabilities after four distinct terminal deletion landings. This join changes no production or documentation file and owns no deletion.

## Exact predecessor contract

- **CCA1T4:** mixed option/output compatibility data and the final legacy module shell are absent; its CCA1T1–CCA1T3 ancestors separately deleted the dynamic registry, framework compatibility adapters, and combined trait.

## Acceptance and ownership proof

- Repository-wide structural/dependency evidence proves no production definition, export, trait object, implementation, lookup, call, option bucket, or compatibility adapter for `CarrierCompiler`/`CarrierCompilerRegistry` remains.
- Immutable catalog lookup over `CarrierFrontend`, `FrameworkSemanticAuthority`, `ProjectionBackend`, `RuntimeCompilerBackend`, and `FrameworkHostIntegrationBackend` is the sole compiler authority.
- The full parse, semantic, projection, runtime, host, NAPI/WASM, native/unplugin/playground, TypeScript-plugin, map, diagnostic, cache, cancellation, and publication suites remain equivalent with zero duplicate work.
- `CompileArtifactSet`, framework assembly migration, style continuation, custom-block descriptors, and downstream facade work remain exclusively CCA2A–CCA2F.

## Budget, aborts, and verification

Production budget is 0 LOC, 0 files, 0 packages. Any missing migration or deletion reopens its precise predecessor; this join may not absorb fixes. Run repository-wide structural inspection and `docs-domain`; add only CCA1's ledger row. CCA2A consumes this terminal fact.
