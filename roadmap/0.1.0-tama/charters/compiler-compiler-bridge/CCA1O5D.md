<!-- unified-charter-v2
id=CCA1O5D
name=WASM legacy compile-profile deletion
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=cutover
semantic_role=delivery
class=compiler
predecessors=CCA1O3A,CCA1O3B,CCA1O5
owner=compiler.compiler-bridge:WASM-local HostCompileProfile and WASM FfiCompileProfile decode deletion
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
size=M
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/compiler-compiler-bridge/CCA1O5D.md
max_production_loc=650
max_production_files=4
max_related_packages=2
rescope_loc=1500
rescope_files=8
rescope_unrelated_packages=3
-->

# CCA1O5D — WASM legacy compile-profile deletion

## Independently acceptable outcome and owners

Delete the now-unused WASM-local legacy profile DTO family and Rust `FfiCompileProfile` decode paths after playground and both direct tooling populations have migrated. Current compatibility ownership is WASM-local; final request ownership is the typed adapter. Reverting restores only unused binding-local compatibility.

## Exact production surface

- `crates/verter_wasm/src/lib.rs` — positional/profile decoders for IDE, virtual-file, block-override, and related legacy routes.
- `packages/wasm/src/index.ts` — local `HostCompileProfile`, `HostBlockOverrideRequest`, `HostVirtualQuery`, legacy signatures, and exports.
- `packages/playground/src/core/compiler.ts` — zero-consumer proof and any terminal import cleanup only; request migration belongs to CCA1O5.
- `docs/api/wasm.md` — final typed request documentation.
- Existing WASM/package/playground tests may change without adding production ownership.

## Exact predecessor contracts

- **CCA1O3A:** the direct WASM fixture producer uses the typed request and preserves fixture bytes/coordinate semantics.
- **CCA1O3B:** the direct WASM transport probe uses the typed request and preserves comparison axes.
- **CCA1O5:** the playground runtime population uses the typed request and no longer owns a legacy profile.

## Invariants and acceptance

- Structural evidence proves no WASM/package/playground/tool consumer of the local profile or composite legacy DTOs remains before deletion.
- Delete every `FfiCompileProfile` decode/reference in `verter_wasm` but retain shared protocol/FFI schema for CCA1O.
- Preserve public typed request behavior, output/map/diagnostic bytes, canonical IDs, SFC-absolute Rust spans, JavaScript UTF-16 offsets, cancellation, and ordering.

## Budget, aborts, and verification

Ceiling: 650 production/documentation LOC, 4 production/documentation files, 2 related crate/packages; abort on a fifth production surface or remaining consumer. Run WASM/package/playground tests, fixture freshness, transport comparison, generated declarations, and `targeted-domain`. CCA1O consumes this deletion fact.
