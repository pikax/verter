<!-- unified-charter-v2
id=CCA1D2
name=Registered frontend publication-store convergence
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=migration
semantic_role=delivery
class=compiler
predecessors=CCA1D1
owner=compiler.compiler-bridge:registered FrameworkParseArtifact publication route
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
charter=charters/compiler-compiler-bridge/CCA1D2.md
max_production_loc=700
max_production_files=7
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1D2 — Registered frontend publication-store convergence

## Independently acceptable outcome and owners

The registered publication store performs its elected parse through `CarrierFrontend` and remains the sole publisher of complete `FrameworkParseArtifact` values. Current parse execution is reached through the combined registry; final parse execution is the CCA1D1 frontend route while publication authority remains store-owned. Reverting restores only the store adapter.

## Concrete surfaces and boundary

- Production surfaces are `crates/verter_session/src/carrier_publication_store/mod.rs`, publication entrypoints in `crates/verter_session/src/parse.rs` and `crates/verter_session/src/host_executor.rs`, plus bounded contract vocabulary in `crates/verter_session/src/framework/descriptor.rs`, `crates/verter_session/src/framework/registry.rs`, and `crates/verter_session/src/types.rs`.
- The boundary is elected `AcceptedRegisteredCarrierSource` → one frontend parse → validated `FrameworkParseArtifact` → complete-only publication/adoption.
- Projection, runtime compilation, host integration, and public request DTOs are excluded.

## Exact predecessor contract

- **CCA1D1:** every selected Vue/Svelte parse executes through the immutable frontend catalog with equivalent artifacts and diagnostics.

## Migration, invariants, and proof

- Preserve parse-key/artifact identity, provenance, registered geometry, cancellation, supersession, fresh/persisted adoption, diagnostic ordering, and one-shot publication.
- Rejected, cancelled, stale, mismatched, or incomplete work cannot publish or warm; one elected publication performs exactly one parse.
- Acceptance removes every publication-store dependency on `CarrierCompiler`/`CarrierCompilerRegistry`, proves fresh/incremental equivalence, and checks parser/publication counters for duplicate work.

## Deletions, budget, and aborts

- Delete only displaced store-facing combined-registry adapters and stale contract vocabulary owned by these six surfaces.
- Ceiling: 700 production LOC, 7 production files, 2 related crates. Abort on an eighth production surface, a second publisher, or lifecycle/parity/performance divergence.

## Verification and consumer

Use existing publication-store, parse, persistence, cancellation, and supersession suites plus `targeted-domain`. CCA1D is the zero-production convergence proof consumed by semantic backend nodes.
