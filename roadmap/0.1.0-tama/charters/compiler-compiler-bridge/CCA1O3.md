<!-- unified-charter-v2
id=CCA1O3
name=WASM typed host-request adapter
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=implementation
semantic_role=delivery
class=compiler
predecessors=CCA1O1
owner=compiler.compiler-bridge:WASM typed host compile request and fully local legacy DTO boundary
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
charter=charters/compiler-compiler-bridge/CCA1O3.md
max_production_loc=650
max_production_files=6
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1O3 — WASM typed host-request adapter

## Independently acceptable outcome and owners

Add the typed WASM request adapter beside the legacy route and make the entire legacy WASM request surface binding-local. Current TypeScript ownership imports native profile-bearing DTOs; final compatibility ownership is `packages/wasm` alone. Reverting removes the typed adapter/local DTO family and restores the native imports.

## Concrete surfaces and named DTO boundary

- Surfaces are `crates/verter_wasm/src/lib.rs`, `packages/wasm/src/index.ts`, `docs/api/wasm.md`, and focused WASM/package tests.
- Add WASM-local exported `HostCompileProfile`, `HostBlockOverrideRequest`, and `HostVirtualQuery` definitions with their existing public names and wire shapes. The two composite DTOs reference the local profile, not `@verter/native` aliases.
- All wrapper signatures—`applyBlockOverrides`, `getVirtualFile`, `getIde`, and `ensureIdeCompiled`—use those local DTOs. Non-profile leaf types may remain imported from native only when their declaration graph contains no `HostCompileProfile` reference.
- Own WASM decode/validation of framework-discriminated `HostCompileRequest` and exact conversion to CCA1O1's FFI schema.

## Exact predecessor contract

- **CCA1O1:** protocol/FFI provides the typed framework-discriminated host request and fail-closed converter.

## Invariants and acceptance

- Unknown/cross-framework fields fail at deserialization; legacy behavior remains unchanged until CCA1O5D.
- Declaration/type evidence proves `packages/wasm` neither imports nor re-exports native `HostCompileProfile`, native `HostBlockOverrideRequest`, or native `HostVirtualQuery`, and no exported WASM declaration reaches native `HostCompileProfile` transitively.
- Existing public names, optionality, compile-profile wire shape, canonical IDs, output/map/diagnostic bytes, SFC-absolute Rust spans, and JavaScript UTF-16 offsets remain equivalent.
- No production playground/tool call moves and no profile schema is deleted.

## Deletions, budget, and verification

Delete no legacy route; replace only cross-package type aliases with binding-local equivalents. Ceiling: 650 production/documentation LOC, 6 files, 2 related crates/packages; abort if another consumer or binding enters. Run WASM conversion/declaration/package tests, a generated-declaration dependency inspection, and `targeted-domain`. O3A/O3B/O5 consume the local boundary; O4D may delete native types only after this row exists.
