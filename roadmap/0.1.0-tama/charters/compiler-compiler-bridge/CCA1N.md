<!-- unified-charter-v2
id=CCA1N
name=Native host-integration route convergence join
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=convergence
semantic_role=convergence
class=compiler
predecessors=CCA1N3,CCA1N4
owner=compiler.compiler-bridge:native host-route convergence proof
conflict_domains=compiler_execution,host_service_graph
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
charter=charters/compiler-compiler-bridge/CCA1N.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1N — Native host-integration route convergence join

## Independently acceptable outcome

Confirm both former generic session bundle selectors are gone through separate landings: CCA1N3 owns host-backed multi-product selection and CCA1N4 owns fixed-Vue runtime-render selection. This join changes no production file.

## Exact predecessor contracts

- **CCA1N3:** the host-backed `compile_entry` call and its complete eight-file route-record population have converged.
- **CCA1N4:** the independent `compile_entry_runtime_render` call and render-specific route evidence have converged.

## Acceptance and ownership

- Repository-wide structural evidence proves both production session `CarrierCompiler::compile_bundle` calls retained by CCA1M are absent and no third route was absorbed.
- Vue/Svelte host backends remain sole topology owners; generic session orchestration retains lifecycle, cancellation, refusal atomicity, and complete-only publication.
- Public request DTOs, unplugin/playground consumers, and staged artifacts remain excluded.

## Budget and verification

Production budget is 0 LOC, 0 files, 0 packages. Run bounded structural inspection and `docs-domain`; add only CCA1N's ledger row. CCA1O1 and downstream J4 consume this convergence fact.
