<!-- unified-charter-v2
id=CCA1O4
name=Native unplugin host-request convergence
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=migration
semantic_role=delivery
class=compiler
predecessors=CCA1O2,CCA1O2H,CCA1O2I,CCA1O2J
owner=compiler.compiler-bridge:unplugin typed native host request route
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
charter=charters/compiler-compiler-bridge/CCA1O4.md
max_production_loc=500
max_production_files=4
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1O4 — Native unplugin host-request convergence

## Independently acceptable outcome and owners

Move unplugin's native host calls to CCA1O2's typed framework-discriminated request while the complete legacy native profile surface remains available. Current request construction is profile-shaped; final consumer ownership is typed and framework-specific. Reverting changes only unplugin request construction.

## Concrete surfaces and API boundary

- Production surfaces are `packages/unplugin/src/index.ts` and `packages/unplugin/src/core/compiler.ts`; focused evidence is their existing specs.
- Own framework/product request construction for the typed batch route replacing `compileMany`, for `getVirtualFile`, for the IDE path — one typed `compileRequest` in place of materialization followed by a cached read — and for conversion from the stable external unplugin option vocabulary into the typed native request.
- The public `compileProfile` plugin option may remain as a stable user option; it is not the deleted binding DTO and must be translated once at the native call boundary.
- Native/NAPI signatures, benchmark/tools, TypeScript plugin, WASM, and any profile deletion are excluded.

## Exact predecessor contract

- **CCA1O2:** typed NAPI/native request and exact converter coexist with the legacy signatures.
- **CCA1O2H:** implemented ledger row for “NAPI own-property closedness repair”; an own unknown or cross-framework key is refused whatever its value, so this consumer inherits the unqualified fail-closed rule.
- **CCA1O2I:** implemented ledger row for “Generated native host-request TypeScript mirror”; the typed request declarations are generated and byte-pinned against the Rust schema.
- **CCA1O2J:** implemented ledger row for “NAPI typed host-request callable route”; the native host object exposes callable typed compile and batch routes, so this consumer has a reachable typed route to move onto.

## Invariants and acceptance

- Preserve Vue/Svelte bundling, HMR, manifest, virtual modules, cache keys, cancellation, maps, diagnostics, output order, and stable external plugin options.
- Each old native call becomes one typed call — the IDE ensure-then-read pair included — with no source copied into a request and no duplicate compilation; malformed/cross-framework fields fail closed.
- Structural evidence proves unplugin has no native `HostCompileProfile` import or profile-bearing host call while all legacy native exports still exist.

## Deletions, budget, and verification

Delete only unplugin-local legacy host request construction/imports. Ceiling: 500 production LOC, 4 production/test files, 1 package; abort if native binding deletion or another consumer enters. Run unplugin type/unit/bundler suites and `targeted-domain`. CCA1O4D consumes this migration fact.
