<!-- unified-charter-v2
id=CCA2EV
name=Vue custom-block descriptor production
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=migration
semantic_role=delivery
class=compiler
predecessors=CCA2E0
owner=compiler.compiler-bridge:Vue CustomBlockDescriptor production
conflict_domains=compiler_execution,source_lineage,vue_product
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
charter=charters/compiler-compiler-bridge/CCA2EV.md
max_production_loc=500
max_production_files=5
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA2EV — Vue custom-block descriptor production

## Independently acceptable outcome, role, and owners

Make the Vue compiler produce complete `CustomBlockDescriptor` values from the registered Vue parse/source artifact while retaining behavior-preserving conversion to the legacy neutral bundle. Current Vue output reduces blocks to tag/content/attrs and later loses source identity; final production ownership is the Vue compiler frontend/runtime boundary. Neutral bundle/session consumers migrate later.

## Expected production surfaces and named boundary

- `crates/verter_compiler/src/compile/types.rs`/`compile/mod.rs` — retain all Vue parse facts needed for descriptor construction.
- `crates/verter_compiler/src/framework_common/vue_bridge.rs` — construct descriptors from the exact registered source/parse identity and preserve them beside the legacy adapter.
- Bounded assembly attachment surfaces admit the descriptors to CCA2A artifacts without adding semantics.

Own Vue role/tag, `lang`, `src`, ordered attrs, source order, SFC-absolute region, content state/reference, source unit/revision, and provenance. Neutral bundle/session consumption, legacy DTO deletion, custom-block transforms, and provider/source loading are excluded.

## Exact predecessor contract and binding architecture

- **CCA2E0:** validated descriptor and staged attachment types exist, including complete-only and source-mismatch rejection.
- One Vue block produces one descriptor bound to the same source revision and parse artifact; `src` remains a source reference and is never fetched here. Unknown blocks remain opaque.
- The current Svelte runtime producer cell is bounded inapplicable and zero-work. Do not add a Svelte branch, fixture expectation, descriptor producer, or migration.

## Migration, exact deletions, acceptance, and performance

Characterize current Vue block order/content/virtual-node behavior, produce the full descriptor in parallel with one compatibility projection, and delete only Vue-side field reconstruction made unreachable. No dual semantic authority or descriptor synthesis from generated module text is allowed.

- **CCA2EV-AC1:** every Vue custom block has exactly one source-backed descriptor; planted tag-only/content-only or reordered production fails.
- **CCA2EV-AC2:** named fields, spans, identities, provenance, local/src/empty content states, and source order are exact and deterministic.
- **CCA2EV-AC3:** fresh/incremental/edit-revert agree; stale/cancelled/partial/source-mismatched production cannot publish or warm.
- **CCA2EV-AC4:** one block performs one metadata construction with no source reload, transform, AST, semantic/runtime work, or duplicate content copy; absent and Svelte-inapplicable cells are zero-work.

Ceiling: 500 production LOC, 5 production files, 1 crate. Abort on neutral session migration, legacy DTO deletion, transform/plugin semantics, provider I/O, invented Svelte production, or a sixth production file. Run Vue compile/custom-block/assembly/span tests and `targeted-domain`; CCA2EH consumes the descriptors.
