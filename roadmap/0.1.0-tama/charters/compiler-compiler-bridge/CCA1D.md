<!-- unified-charter-v2
id=CCA1D
name=Registered frontend convergence join
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=convergence
semantic_role=convergence
class=compiler
predecessors=CCA1D2
owner=compiler.compiler-bridge:registered frontend route convergence proof
conflict_domains=carrier_parser,compiler_execution
resource_class=docs-light
review_profile=architecture-3
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
charter=charters/compiler-compiler-bridge/CCA1D.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1D — Registered frontend convergence join

## Independently acceptable outcome

Confirm that CCA1D1 owns every registered parse selector and CCA1D2 owns the complete-only publication route. This join changes no production or documentation file and introduces no compatibility authority.

## Exact predecessor contract

- **CCA1D2:** publication-store migration is implemented; its CCA1D1 ancestor proves frontend parse-route convergence.

## Acceptance and exclusions

- Repository-wide structural evidence resolves every former production parse/publication combined-registry call to CCA1D1 or CCA1D2.
- Parse bytes, keys, diagnostics, geometry, provenance, cancellation, persistence, and publication counters are green in the already-owned suites.
- Semantic, projection, runtime, host, FFI, and staged-artifact work is forbidden.

## Budget and verification

Production budget is 0 LOC, 0 files, 0 packages. Run bounded structural inspection and the `docs-domain` consistency gate; add only CCA1D's ledger row. CCA1E and CCA1F consume this convergence fact.
