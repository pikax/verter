<!-- unified-charter-v2
id=CCA1O2E
name=Native transport-probe host-request migration
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=migration
semantic_role=delivery
class=compiler
predecessors=CCA1O2,CCA1O2H,CCA1O2I
owner=compiler.compiler-bridge:native transport-surface probe typed host requests
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
charter=charters/compiler-compiler-bridge/CCA1O2E.md
max_production_loc=250
max_production_files=1
max_related_packages=1
rescope_loc=600
rescope_files=3
rescope_unrelated_packages=2
-->

# CCA1O2E — Native transport-probe host-request migration

## Independently acceptable outcome and rollback boundary

Move the native binding's direct transport-surface probe to CCA1O2's typed request while both NAPI routes remain available. Reverting restores only the probe's legacy request objects.

## Concrete surfaces and APIs

- The sole production/tooling surface is `packages/native/scripts/probe-transport-surface.mjs`.
- Owns every profile-bearing `getVirtualFile`, `getIde`, `ensureIdeCompiled`, and runtime-render `compileMany` probe case in that file, including success, refusal, optional-product, ordering, and audit variants.
- Surface enumeration, result normalization, refusal-vs-missing distinctions, audit attribution, canonical IDs, and UTF-16/public span encoding remain unchanged.

## Exact predecessor contract

- **CCA1O2:** implemented ledger row for “NAPI typed host-request adapter”.
- **CCA1O2H:** implemented ledger row for “NAPI own-property closedness repair”; the native decode refuses an own unknown or cross-framework key whatever its value, so the typed route this caller moves onto is closed as declared.
- **CCA1O2I:** implemented ledger row for “Generated native host-request TypeScript mirror”; the request declarations this caller is written against are generated from the Rust schema and byte-pinned, so they cannot drift from the decoder.

## Acceptance and evidence

- The probe contains no legacy general or render compile-profile object and exercises the typed Vue/Svelte request variants through the same number of binding calls.
- Probe output keys, ordering, output/map/refusal classification, diagnostics, audit fields, canonical IDs, and serialized offsets are equivalent.

## Deletions, budgets, and aborts

- Delete no NAPI/native type, converter, probe case, output key, or audit field.
- Ceiling: 250 production/tooling LOC, 1 production/tooling file, 1 related package; rescope above 600 LOC, 3 files, 2 unrelated packages, or if another consumer enters.
- Abort on a deleted probe axis, duplicate binding call, changed normalization, or transport divergence.

## Verification and review

Use the existing transport probe/conversion suites plus `node --check` and `targeted-domain`. Add only CCA1O2E's ledger row and apply `public-3`.
