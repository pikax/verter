<!-- unified-charter-v2
id=CCA1M1
name=Direct and batch runtime route convergence
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=migration
semantic_role=delivery
class=compiler
predecessors=CCA1K,CCA1L
owner=compiler.compiler-bridge:direct prepared and batch RuntimeCompilerBackend delegation
conflict_domains=compiler_execution
resource_class=rust-mixed
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
size=M
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/compiler-compiler-bridge/CCA1M1.md
max_production_loc=750
max_production_files=7
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1M1 — Direct and batch runtime route convergence

## Independently acceptable outcome and owners

Direct, prepared, managed, and batch compiler entrypoints construct one typed runtime request and delegate to the catalog-selected `RuntimeCompilerBackend`. Current ownership is split across combined-trait helpers; final ownership is the framework runtime backend. Reverting restores only these compiler-local consumers.

## Concrete surfaces and boundary

- Production surfaces are runtime entrypoints in `crates/verter_compiler/src/compile/mod.rs`, `crates/verter_compiler/src/standalone.rs`, `crates/verter_compiler/src/compile_request.rs`, `crates/verter_compiler/src/framework_common/vue_bridge.rs`, `crates/verter_compiler/src/svelte/carrier.rs`, and bounded exports in their module files.
- Owns direct/prepared/batch request construction, catalog selection, standalone-core delegation, and removal of combined-trait runtime calls in those routes.
- Both session `compile_bundle` calls are excluded and remain byte-for-byte present for CCA1M2/CCA1M3.

## Exact predecessor contracts

- **CCA1K:** Vue runtime backend is installed and behaviorally equivalent.
- **CCA1L:** Svelte runtime backend is installed and behaviorally equivalent.

## Invariants and acceptance

- Preserve outputs, maps, diagnostics, options, requested products, refusal, cancellation, fresh/incremental behavior, and SFC-absolute internal spans.
- One request performs one parse/semantic/plan/emit/assembly/copy sequence; no combined-trait fallback or dual execution remains.
- Structural proof shows compiler-local direct/prepared/batch routes use only `RuntimeCompilerBackend` and neither generic session selector changed.

## Deletions, budget, and verification

Delete only displaced compiler-local runtime consumer methods/adapters. Ceiling: 750 production LOC, 7 files, 1 crate; abort if a host selector or public DTO enters. Run direct/prepared/batch compiler suites and `targeted-domain`. CCA1M consumes this route fact.
