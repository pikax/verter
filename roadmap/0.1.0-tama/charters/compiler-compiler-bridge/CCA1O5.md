<!-- unified-charter-v2
id=CCA1O5
name=WASM playground host-request migration
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=migration
semantic_role=delivery
class=compiler
predecessors=CCA1O3,CCA1O1A,CCA1O3C
owner=compiler.compiler-bridge:playground typed WASM host request route
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
charter=charters/compiler-compiler-bridge/CCA1O5.md
max_production_loc=500
max_production_files=2
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1O5 — WASM playground host-request migration

## Independently acceptable outcome and owners

Move the playground runtime consumer to CCA1O3's typed WASM request while all WASM-local legacy DTOs and Rust decoders remain available. Current request construction is playground-local profile vocabulary; final consumer ownership is the typed WASM adapter. Reverting changes only playground request construction.

## Concrete surfaces and boundary

- The sole production surface is `packages/playground/src/core/compiler.ts`; focused evidence is `packages/playground/src/core/compiler.spec.ts`.
- Own Vue/Svelte framework/product request construction for upsert, virtual products, IDE materialization/read, SSR/client modes, source maps, and runtime requests.
- WASM package declarations, Rust binding decoders, fixture capture, transport probe, and all profile deletion are excluded.

## Exact predecessor contract

- **CCA1O3:** typed WASM adapter and the complete WASM-local compatibility DTO family coexist with the legacy route.
- **CCA1O1A:** implemented ledger row for “Canonical Svelte custom-element prop-type admission”; the Svelte custom-element prop-type slot has its final shape, so playground request construction encodes no superseded closed vocabulary.
- **CCA1O3C:** implemented ledger row for “Execution-proven WASM JS-boundary gate”; the browser boundary refusals this runtime consumer depends on are proven by execution rather than by compilation alone.

## Invariants and acceptance

- Preserve output/map/diagnostic bytes, cache and cancellation behavior, canonical IDs, product ordering, SFC-absolute Rust spans, and JavaScript UTF-16 offsets.
- Each profile-bearing playground call becomes one typed WASM call with no duplicate compile/source copy.
- The playground production file contains no local `HostCompileProfile` interface or legacy request field while the binding-local legacy route remains intact for tools.

## Deletions, budget, and verification

Delete only playground-local legacy request construction/type vocabulary. Ceiling: 500 production LOC, 2 production/test files, 1 package; abort if binding or tool mutation enters. Run playground/WASM interop suites and `targeted-domain`. CCA1O5D consumes this migration fact.
