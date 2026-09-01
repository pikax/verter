<!-- unified-charter-v2
id=CCA1O2I
name=Generated native host-request TypeScript mirror
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=implementation
semantic_role=delivery
class=compiler
predecessors=CCA1O1A,CCA1O2
owner=compiler.compiler-bridge:generated and byte-pinned native host compile-request TypeScript declarations
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
charter=charters/compiler-compiler-bridge/CCA1O2I.md
max_production_loc=650
max_production_files=6
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1O2I — Generated native host-request TypeScript mirror

## Independently acceptable outcome and rollback boundary

The durable problem: the native TypeScript declaration of the host compile request is hand-written beside the Rust schema that actually decodes it. It is aligned today only by inspection, and nothing structurally binds a nested Rust variant, field, optionality, or string union to it. A change on either side can leave the published declaration silently wrong, so a caller type-checks against a shape the decoder refuses, or trusts a slot the schema no longer accepts, with no failing check anywhere.

Outcome: the host compile-request portion of the native TypeScript surface is generated from the Rust declarations and byte-pinned, adopting the repository's existing generated-and-pinned pattern, so Rust/TypeScript drift fails the owning gate instead of shipping. Reverting deletes the generator, the generated file, and the guard, and restores the hand-written section.

## Concrete surfaces and APIs

- `crates/verter_napi/src/host_compile_request_ts.rs`: the renderer and the structural-metadata owner.
- `crates/verter_napi/src/bin/generate_host_compile_request_ts.rs`: the generator binary that writes the committed output. Generation is a command, not a test; no test regenerates on disk.
- `packages/native/host-compile-request.generated.ts`: the committed generated output.
- `crates/verter_napi/tests/cases/host_compile_request_ts_freshness.rs`: the byte-pin guard, registered in the existing NAPI integration-test harness. It renders in memory and compares exact bytes with the committed file.
- `packages/native/host-types.ts`: re-exports the generated request types in place of the hand-written declarations that currently begin at its host compile-request section.
- `packages/native/package.json`: publishes both the generated source and its emitted declaration artifact.
- Generation reads `ts-rs`-style derives and structural metadata on the actual native request and the nested protocol DTO declarations. It must never recover declarations by scanning Rust source names, paths, or tokens.
- The generation boundary is the host compile-request section only. The rest of `host-types.ts` is unrelated hand-written host and session API: it is neither generated nor byte-pinned. The browser package declarations, the FFI schema itself, and consumer migration are excluded.

## Exact predecessor contract

- **CCA1O1A:** implemented ledger row for “Canonical Svelte custom-element prop-type admission”; the Svelte custom-element prop-type slot has reached its final wire shape — a forwarded string with no closed wire enum — so generation cannot mint a superseded closed union.
- **CCA1O2:** implemented ledger row for “NAPI typed host-request adapter”; the tagged native request and the native TypeScript request union are owned at this boundary.

## Acceptance and evidence

- Every published host compile-request type originates in the generated file, the hand-written duplicates are gone, and the existing published module re-exports them under their current names.
- The guard fails when a Rust field, variant, optionality, or string-union member changes without regeneration, and passes on the committed bytes.
- The guard's discrimination is proven: a deliberate structural change to a Rust declaration reddens it, and the mutation is shown to be present, unique, and new before the red run is trusted.
- Generation derives declarations from structural metadata; no source-name, path, or token scan exists in the generator.
- The generated declarations describe the same shapes the decoder accepts and refuses, including closed objects, required slots, optional slots, and closed string unions.
- Unrelated declarations in the same published module are untouched and unpinned, and the published package contains both the generated source and its emitted declaration.

## Deletions, budgets, and aborts

- Delete the hand-written host compile-request declaration section from the published module. Delete nothing else.
- Planning guidance: roughly 650 LOC including generated output, across 6 files in 2 related crates/packages. These figures are guidance, not a cap; rescope only under the program's mandatory thresholds or when generation is asked to cover another surface.
- Abort on a name-keyed source scanner, on generation extended beyond the request section, on a guard that pins the whole hand-written file, or on a generated shape that disagrees with the decoder.

## Verification and review

Regenerate and commit the output, run the NAPI integration harness, the native package type tests, and `targeted-domain`. Apply `public-3`; add only CCA1O2I's ledger row.
