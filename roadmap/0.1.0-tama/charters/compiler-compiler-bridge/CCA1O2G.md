<!-- unified-charter-v2
id=CCA1O2G
name=TypeScript-plugin native-signature migration
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=migration
semantic_role=delivery
class=compiler
predecessors=CCA1O2,CCA1O2H,CCA1O2I
owner=compiler.compiler-bridge:typescript-plugin typed native IDE request route
conflict_domains=host_service_graph,public_protocol
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
charter=charters/compiler-compiler-bridge/CCA1O2G.md
max_production_loc=300
max_production_files=1
max_related_packages=1
rescope_loc=700
rescope_files=4
rescope_unrelated_packages=2
-->

# CCA1O2G — TypeScript-plugin native-signature migration

## Independently acceptable outcome and owners

Move the production TypeScript-plugin mirror host from positional native IDE profiles to CCA1O2's typed framework-discriminated IDE request while both native signatures remain available. Current ownership is the local structural `CarrierCodegenHost` profile shape; final ownership is the typed native request. Reverting changes only this plugin consumer.

## Exact source population and API boundary

- The sole production surface is `packages/typescript-plugin/src/tsc/mirrorHost.ts`; focused evidence is `mirrorHost.spec.ts` and `spike.spec.ts`.
- Own the `CarrierCodegenHost` signatures, the dynamic `@verter/native` constructor typing, and the `ensureIdeCompiled`/`getIde` call pair used for Vue and Svelte IDE carriers.
- Construct one typed request from each source's framework, canonical ID, IDE product, source-map intent, and existing options; `getPublicApi`, mirroring, declaration generation, and TypeScript project orchestration are excluded.

## Exact predecessor contract

- **CCA1O2:** NAPI/native exposes the typed `HostCompileRequest` route beside the legacy profile route.
- **CCA1O2H:** implemented ledger row for “NAPI own-property closedness repair”; the native decode refuses an own unknown or cross-framework key whatever its value, so the typed route this consumer moves onto is closed as declared.
- **CCA1O2I:** implemented ledger row for “Generated native host-request TypeScript mirror”; the request declarations this consumer type-checks against are generated from the Rust schema and byte-pinned, so they cannot drift from the decoder.

## Invariants and acceptance

- Preserve lazy native loading, injected fake-host compatibility, pure cached `getIde`, explicit materialization before read, carrier source ordering, output/map bytes, diagnostics, and missing/refused behavior.
- The production file contains no positional `{ target, sourceMap }` IDE profile signature/call and type-checks against the real native host without a cast that hides signature incompatibility.
- One source still performs at most one ensure and one cached read; no extra native compile or source copy is introduced.

## Deletions, budget, and verification

Delete only the plugin-local legacy signatures and call objects; delete no native type or binding converter. Ceiling: 300 production LOC, 1 production file, 1 package; focused existing tests do not enlarge that production budget. Abort if another package consumer enters. Run TypeScript-plugin build/type/unit tests and `targeted-domain`. CCA1O4D requires this row before native profile deletion.
