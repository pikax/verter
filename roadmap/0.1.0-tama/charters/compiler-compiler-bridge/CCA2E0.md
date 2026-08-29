<!-- unified-charter-v2
id=CCA2E0
name=Source-backed custom-block descriptor contract
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=implementation
semantic_role=delivery
class=compiler
predecessors=CCA2A
owner=compiler.compiler-bridge:additive CustomBlockDescriptor source-backed attachment contract
conflict_domains=compiler_execution,source_lineage
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
size=S
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/compiler-compiler-bridge/CCA2E0.md
max_production_loc=400
max_production_files=4
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA2E0 — Source-backed custom-block descriptor contract

## Independently acceptable outcome, role, and owners

Add an unused compiler-owned `CustomBlockDescriptor` for opaque source-backed attachments. It preserves role/tag, `lang`, `src`, ordered attributes, source order, SFC-absolute region, content availability/reference, source-unit identity, revision, and provenance without adding transform semantics. Current `CustomBlock`/`VerterCustomBlock`/`RuntimeCustomBlock` shapes conflate or omit these fields; final schema ownership begins here. Reverting removes only additive types and tests.

## Expected production surfaces and named APIs

- `crates/verter_compiler/src/compile/types.rs` or a bounded assembly attachment module — define `CustomBlockDescriptor`, content-state/reference, and validated construction.
- Bounded `crates/verter_compiler/src/assembly/fragment.rs`, `source_unit.rs`, or `publish.rs` — attach descriptors to CCA2A artifacts with typed relation/provenance.
- Compiler exports expose immutable construction/inspection only; no Vue/Svelte/session production caller is migrated here.

Rust stores the region as SFC-absolute byte `Span`; any serialized boundary uses its owning UTF-16 conversion. Unknown block content remains opaque. No plugin ABI, parser, transform, external AST, runtime evaluation, source load, or framework branch is introduced.

## Exact predecessor contract and binding architecture

- **CCA2A:** artifact/source-unit identity, provenance, typed relations, content availability, deterministic order, and qualified map vocabulary exist.
- Descriptor identity binds source unit/incarnation/revision, block role/tag, source order, region, attributes, `lang`/`src`, and content state. Malformed, stale, aliased, or source-mismatched descriptors fail closed.
- The currently implemented Svelte runtime producer has no custom-block output cell. That cell is bounded inapplicable and zero-work; this node must not invent a Svelte producer or migration.

## Internal subblocks, acceptance, and performance

1. Define immutable descriptor/content-state types and exact validation.
2. Define deterministic attachment to staged artifact/source-unit relations.
3. Prove positive local/src-backed/empty states and planted stale/source-mismatch/order/region/attribute-alias failures.

- **CCA2E0-AC1:** additive descriptor authority has zero production consumers and deletes nothing.
- **CCA2E0-AC2:** every named field, identity, provenance, span, content state, and deterministic order round-trips without copying unavailable/external content.
- **CCA2E0-AC3:** stale/cancelled/partial/source-mismatched descriptors cannot publish or warm.
- **CCA2E0-AC4:** construction performs bounded metadata validation only; unknown, absent, and Svelte-inapplicable cells perform zero semantic/runtime/provider work.

Ceiling: 400 production LOC, 4 production files, 1 crate. Abort on a production route mutation, custom-block semantics/plugin ABI, provider/source loading, invented Svelte production, or a fifth production file. Run compiler type/attachment/span tests and `targeted-domain`; CCA2EV consumes this additive contract.
