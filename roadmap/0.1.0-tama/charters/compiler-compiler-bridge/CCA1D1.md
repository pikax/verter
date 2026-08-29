<!-- unified-charter-v2
id=CCA1D1
name=Registered frontend parse-route convergence
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=migration
semantic_role=delivery
class=compiler
predecessors=CCA1B,CCA1C
owner=compiler.compiler-bridge:registered CarrierFrontend parse execution route
conflict_domains=carrier_parser,compiler_execution
resource_class=rust-mixed
review_profile=architecture-3
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
charter=charters/compiler-compiler-bridge/CCA1D1.md
max_production_loc=650
max_production_files=6
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1D1 — Registered frontend parse-route convergence

## Independently acceptable outcome and owners

All registered Vue/Svelte parse execution outside the publication store selects one immutable-catalog `CarrierFrontend`. Current ownership is split between `CarrierCompiler` calls and closed adapter switches; final ownership is the catalog-selected frontend. Reverting this node restores those parse selectors while leaving both frontend backends installed.

## Concrete surfaces and boundary

- Production/tool surfaces are `crates/verter_compiler/src/framework_common/registered_carrier_projection.rs`, `crates/verter_compiler/src/bin/parse_corpus_probe.rs`, and registered parse lookup in `crates/verter_session/src/parse.rs`; bounded export/registration adjustments may touch `crates/verter_compiler/src/framework_common/mod.rs`, `crates/verter_session/src/framework/registry.rs`, and `crates/verter_session/src/framework/descriptor.rs`.
- The named boundary is `CarrierFrontend::parse` producing the same unregistered parse artifact, diagnostics, parse key, syntax profile, adapter identity, and registered geometry input.
- Publication, election, freshness, and store adoption are excluded and remain unchanged for CCA1D2.

## Exact predecessor contracts

- **CCA1B:** the Vue `CarrierFrontend` backend and catalog registration are implemented.
- **CCA1C:** the Svelte `CarrierFrontend` backend and catalog registration are implemented.

## Migration, invariants, and proof

- Replace each named direct `CarrierCompiler::parse` or closed combined-registry parse selector with one catalog lookup and one frontend call; no dual parse or fallback is permitted.
- Preserve source bytes, canonical adapter/language identity, strictness, diagnostics, recovery, parse keys, SFC-absolute Rust spans, and serialized UTF-16 boundary behavior.
- Acceptance proves the probe and non-store parse routes contain no combined-trait parse call, reject unknown frameworks, and add zero parse/copy pass in cold, warm, incremental, cancellation, and refusal cases.

## Deletions, budget, and aborts

- Delete only displaced non-publication parse selectors and imports; retain publication-store lookup and combined compatibility definitions for later owners.
- Ceiling: 650 production LOC, 6 production/tooling files, 2 related crates. Abort on a seventh production surface, publication mutation, or parse/parity/performance divergence.

## Verification and consumer

Use existing compiler parse/probe and session parse suites, targeted structural inspection, and `targeted-domain`. CCA1D2 consumes the resulting frontend-only parse route.
