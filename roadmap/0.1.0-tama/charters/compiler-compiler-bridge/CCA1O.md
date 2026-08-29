<!-- unified-charter-v2
id=CCA1O
name=Shared legacy compile-profile deletion
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=cutover
semantic_role=delivery
class=compiler
predecessors=CCA1O4D,CCA1O5D
owner=compiler.compiler-bridge:FfiCompileProfile and shared profile-converter deletion
conflict_domains=compiler_execution,host_service_graph,public_protocol
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
size=S
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/compiler-compiler-bridge/CCA1O.md
max_production_loc=400
max_production_files=5
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1O — Shared legacy compile-profile deletion

## Independently acceptable outcome and rollback boundary

Delete the now-unreferenced shared `FfiCompileProfile` schema and converter after the native and WASM consumer populations have independently converged. Reverting restores only that unused shared compatibility DTO; all public bindings and consumers remain on typed host requests.

## Concrete surfaces and APIs

- Production surfaces are exactly `crates/verter_protocol/src/types.rs` and `crates/verter_ffi/src/convert/input.rs`; focused structural/conversion evidence is `crates/verter_ffi/src/convert/tests.rs` plus binding-local tests already updated by CCA1O4D/CCA1O5D.
- Owns only `FfiCompileProfile` and the shared legacy profile-to-host converter/test deletion.
- Typed protocol/FFI request schema remains; NAPI/native, WASM/package, unplugin, and playground routes already moved in predecessors and are excluded from mutation.

## Exact predecessor contract

- **CCA1O4D:** implemented ledger row for “Native legacy compile-profile deletion”; its CCA1O2A–CCA1O2G, CCA1O3, and CCA1O4 ancestors prove every native benchmark/tool/probe, TypeScript-plugin, unplugin, and session-vocabulary consumer moved and WASM declarations were fully localized before native deletion.
- **CCA1O5D:** implemented ledger row for “WASM legacy compile-profile deletion”; its CCA1O3A/CCA1O3B/CCA1O5 ancestors prove fixture-tool, transport-probe, and playground consumers moved before WASM-local deletion.

## Acceptance and evidence

- Structural evidence proves `FfiCompileProfile`, its converter, and every reference are absent while the typed request schema remains sole.
- Protocol/FFI/NAPI/WASM/native/unplugin/playground focused suites remain green without output or refusal changes.

## Deletions, budgets, and aborts

- Delete only `FfiCompileProfile`, its shared converter, and tests that solely validate that retired DTO.
- Ceiling: 400 LOC, 5 files, 2 crates; rescope if a binding or TypeScript consumer still needs mutation.
- Abort if any production reference remains or typed-request behavior diverges.

## Verification and review

Use TDD for the structural deletion boundary, run protocol/FFI plus downstream binding/consumer suites and `targeted-domain`. Apply `public-3`; add only CCA1O's ledger row.
