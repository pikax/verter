<!-- unified-charter-v2
id=CCA1O2B
name=Native comparison-tool host-request migration
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=migration
semantic_role=delivery
class=compiler
predecessors=CCA1O2
owner=compiler.compiler-bridge:repository native comparison-tool typed host requests
conflict_domains=compiler_execution,host_service_graph,public_protocol
resource_class=ts-heavy
review_profile=public-3
gate_profile=targeted-domain
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=high
verification_effort_default=high
confirmation_effort_min=high
confirmation_effort_default=high
size=S
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/compiler-compiler-bridge/CCA1O2B.md
max_production_loc=300
max_production_files=3
max_related_packages=0
rescope_loc=700
rescope_files=5
rescope_unrelated_packages=1
-->

# CCA1O2B — Native comparison-tool host-request migration

## Independently acceptable outcome and rollback boundary

Move the three repository comparison tools that pass legacy native compile profiles to CCA1O2's typed request without changing the binding or the compared compiler behavior. Reverting restores only those tool call shapes.

## Concrete surfaces and APIs

- Tooling surfaces are exactly `scripts/compare-per-file.mjs`, `scripts/ssr-baseline/compare.mjs`, and `scripts/vue-behavior-compare/run.mjs`.
- Owns each named tool's profile-bearing `getVirtualFile` request and its framework/product/options construction. Corpus discovery, official-compiler baselines, normalization, reporting, and file traversal are excluded.
- Canonical input IDs, source bytes, output bytes/maps/diagnostics, and serialized offsets remain identical; one old native call becomes one typed native call.

## Exact predecessor contract

- **CCA1O2:** implemented ledger row for “NAPI typed host-request adapter”.

## Acceptance and evidence

- Targeted inspection proves no legacy `compileProfile` field remains in the three files and every replacement names the Vue framework and exact requested product set.
- Syntax checks and hermetic request-conversion fixtures cover shape/refusal behavior. Optional external corpora may provide supplemental parity evidence but are never a default-run prerequisite.

## Deletions, budgets, and aborts

- Delete no binding type, converter, tool mode, baseline, or report field.
- Ceiling: 300 production/tooling LOC, 3 tooling files, 0 related packages; rescope above 700 LOC, 5 files, 1 unrelated package, or if another tool family enters.
- Abort on a second native call, source copy, changed corpus selection, or output/map/diagnostic normalization change.

## Verification and review

Run `node --check` for all three scripts, applicable hermetic native conversion tests, and `targeted-domain`. External-corpus execution is supplemental only. Add only CCA1O2B's ledger row and apply `public-3`.
