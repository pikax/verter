<!-- unified-charter-v2
id=CCA1O4D
name=Native legacy compile-profile deletion
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=cutover
semantic_role=delivery
class=compiler
predecessors=CCA1O2A,CCA1O2B,CCA1O2C,CCA1O2D,CCA1O2E,CCA1O2F,CCA1O2G,CCA1O3,CCA1O4
owner=compiler.compiler-bridge:NapiCompileProfile and native HostCompileProfile deletion
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
charter=charters/compiler-compiler-bridge/CCA1O4D.md
max_production_loc=650
max_production_files=5
max_related_packages=2
rescope_loc=1500
rescope_files=8
rescope_unrelated_packages=3
-->

# CCA1O4D — Native legacy compile-profile deletion

## Independently acceptable outcome and owners

Delete the now-unused NAPI/native legacy compile-profile types, converters, overloads, and documentation after every native consumer migration is independently landed. Current compatibility ownership is NAPI/native; final request ownership is the typed framework-discriminated adapter. Reverting restores only unused compatibility declarations.

## Exact production surface

- `crates/verter_napi/src/lib.rs` — `NapiCompileProfile`, its conversion, and legacy binding signatures.
- `packages/native/host-types.ts` — native `HostCompileProfile` and profile-bearing native request shapes.
- `packages/native/index.ts` — legacy exports/overloads and native wrapper declarations.
- `packages/native/README.md` and `docs/api/native.md` — binding API documentation.
- Focused NAPI/native/FFI conversion tests may change but do not add production ownership.

## Exact predecessor contracts

- **CCA1O2A–CCA1O2E:** every native benchmark, comparison/SSR tool, and transport probe profile-bearing call is migrated.
- **CCA1O2F:** session-facing explanatory vocabulary no longer names the native legacy type.
- **CCA1O2G:** the production TypeScript-plugin positional native IDE signatures/calls are migrated.
- **CCA1O4:** unplugin's native calls are migrated.
- **CCA1O3:** WASM owns local `HostCompileProfile`, `HostBlockOverrideRequest`, and `HostVirtualQuery` declarations with no direct or transitive native profile dependency, so this deletion cannot alter WASM declarations.

## Invariants and acceptance

- Structural and generated-declaration evidence proves zero native-binding consumer, export, overload, or transitive WASM reference remains before deletion.
- Delete no WASM-local DTO, `FfiCompileProfile`, shared converter, stable external unplugin option, or typed request route.
- Native/NAPI/unplugin/TypeScript-plugin output, map, diagnostic, cancellation, cache, and ordering suites remain equivalent.

## Budget, aborts, and verification

Ceiling: 650 production/documentation LOC, 5 production/documentation files, 2 related crate/packages; abort on a sixth production surface or any remaining consumer. Run NAPI/native declaration/conversion tests, TypeScript-plugin and unplugin type tests, WASM declaration isolation proof, and `targeted-domain`. CCA1O consumes this deletion fact.
