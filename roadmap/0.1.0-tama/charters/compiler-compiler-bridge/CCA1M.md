<!-- unified-charter-v2
id=CCA1M
name=Runtime compile route convergence join
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=convergence
semantic_role=convergence
class=compiler
predecessors=CCA1M1,CCA1M2,CCA1M3
owner=compiler.compiler-bridge:runtime route convergence proof with both temporary host bundle calls retained
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
charter=charters/compiler-compiler-bridge/CCA1M.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1M — Runtime compile route convergence join

## Independently acceptable outcome

Confirm that direct/prepared/batch runtime consumers use CCA1M1, the host-backed temporary adapter uses CCA1M2, and the runtime-render temporary adapter uses CCA1M3. This node changes no production file.

## Exact predecessor contracts

- **CCA1M1:** all compiler-local direct/prepared/batch routes use `RuntimeCompilerBackend`.
- **CCA1M2:** the host-backed adapter delegates runtime work but retains its outer `compile_bundle` call.
- **CCA1M3:** the runtime-render adapter delegates runtime work but retains its separate outer `compile_bundle` call.

## Acceptance and deletion ownership

- Repository-wide evidence proves exactly two production session `compile_bundle` calls remain: host-backed `compile_entry` and fixed-Vue `compile_entry_runtime_render`.
- CCA1N3 exclusively owns deletion of the first call; CCA1N4 exclusively owns deletion of the second.
- No route performs duplicate parse, semantic, projection, runtime, assembly, or copy work; public DTO and host-selector changes are forbidden here.

## Budget and verification

Production budget is 0 LOC, 0 files, 0 packages. Run bounded structural inspection and `docs-domain`; add only CCA1M's ledger row. CCA1N1 and CCA1N2 consume this convergence fact.
