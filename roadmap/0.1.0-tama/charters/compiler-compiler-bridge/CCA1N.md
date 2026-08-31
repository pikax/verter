<!-- unified-charter-v2
id=CCA1N
name=Native host-integration route convergence join
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=convergence
semantic_role=convergence
class=compiler
predecessors=CCA1N3,CCA1N4,CCA1N4A,CCA1N2G
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

Confirm both former generic session bundle selectors are gone through separate landings — CCA1N4 owns the bound runtime-render execution cutover and CCA1N3 owns the host-backed multi-product cutover — and that both lanes converge on the request-scoped binding model. This join changes no production file.

## Exact predecessor contracts

- **CCA1N4:** the runtime-render lane executes both Vue and Svelte through bound framework host backends; its outer bundle call and route evidence have converged.
- **CCA1N3:** the host-backed `compile_entry` call and its complete eight-file route-record population have converged.
- **CCA1N4A:** the runtime-render route record states only the verification its consumers actually perform.
- **CCA1N2G:** carrier grammar at source ingestion derives from immutable compiler-catalog identity; no Vue-else-Svelte grammar fallthrough remains.

## Acceptance and ownership

- Repository-wide structural evidence proves zero session `CarrierCompiler::compile_bundle` calls remain in the two migrated populations and no third route was absorbed.
- Exactly one framework binding occurs per immutable host request; neither the host-backed nor the runtime-render lane contains a framework selector.
- Vue/Svelte `FrameworkHostIntegrationBackend`s are the only framework request-topology owners and the sole issuers of `CompileAdmission` (one admission token type per backend; demand carried in the issued value); product backends consume host-issued admission and never mint it.
- No compatibility fallback remains on either migrated lane: bound execution yields typed unavailability and never switches compiler, framework, or lane. The outer last-known-good publication policy is separately owned by CCA2C and is excluded from this join.
- Generic session route and request orchestration retains only lifecycle, batching, ordering, panic isolation, supersession, cancellation, refusal atomicity, audit correlation, and complete-only publication. Excluded from this join: framework assembly already chartered to CCA2BV — the Vue main-module payload-shape assembly and the template virtual-file Vue import topology — and the outer stale-publication policy owned by CCA2C.
- Route and audit records describe the executed bound topology; public request DTOs, unplugin/playground consumers, and staged artifacts remain excluded.

## Budget and verification

Production budget is 0 LOC, 0 files, 0 packages. Run bounded structural inspection and `docs-domain`; add only CCA1N's ledger row. CCA1O1 and downstream J4 consume this convergence fact.
