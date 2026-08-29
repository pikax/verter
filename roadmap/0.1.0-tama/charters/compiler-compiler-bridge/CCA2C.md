<!-- unified-charter-v2
id=CCA2C
name=Staged host artifact handoff
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=migration
semantic_role=delivery
class=compiler
predecessors=CCA2B
owner=compiler.compiler-bridge:CompileArtifactSet host handoff lifecycle and publication contract
conflict_domains=compiler_execution,host_service_graph
resource_class=rust-mixed
review_profile=concurrency-3
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
charter=charters/compiler-compiler-bridge/CCA2C.md
max_production_loc=750
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA2C — Staged host artifact handoff

## Independently acceptable outcome and owners

Pass one complete `CompileArtifactSet` from the CCA1N-owned framework host backend into session lifecycle/publication code without reconstructing framework topology. Current handoff is a neutral runtime bundle plus session assembly; final handoff is the staged artifact set. Reverting restores only the host adapter.

## Exact production population

- `crates/verter_session/src/host_compile.rs`, `host_executor.rs`, and `host_resolve/virtual_file_pipeline.rs` own request lifecycle, cancellation, supersession, and complete-only publication.
- `crates/verter_compiler/src/assembly/publish.rs`, `assembly/mod.rs`, `standalone.rs`, and bounded host-integration backend surfaces own construction/transport of the staged payload.
- `crates/verter_session/src/types.rs` may carry the session-local handoff wrapper; public NAPI/WASM DTOs do not expose compiler-internal artifacts.

## Exact predecessor contract

- **CCA2B:** its CCA2BV/CCA2BS ancestors separately establish Vue and Svelte compiler-owned semantic module assembly; the zero-production join proves both emit the CCA2A schema through behavior-preserving adapters.

## Invariants and acceptance

- Identity binds canonical source, framework, request/products, source-unit/map basis, options, and revision. Changed inputs restart; cancelled/stale/partial/refused results publish nothing and warm nothing.
- One request crosses the handoff once and performs no duplicate parse, semantic, projection, compile, assembly, serialization, or source copy.
- Host/bundler/HMR/virtual/manifest outputs, diagnostics, maps, lifecycle, and ordering remain equivalent.

## Deletions, budget, and verification

Delete only displaced neutral-bundle handoff and session-side reconstruction after all named consumers use the staged payload; retain CCA2F-owned public/facade adapters. Ceiling: 750 LOC, 8 files, 2 crates; any ninth surface requires rescope. Run host/virtual/batch/cancellation/publication suites and `targeted-domain`. CCA2F consumes this handoff.
