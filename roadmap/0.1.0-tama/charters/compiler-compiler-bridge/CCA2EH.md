<!-- unified-charter-v2
id=CCA2EH
name=Neutral custom-block consumer migration
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=migration
semantic_role=delivery
class=compiler
predecessors=CCA2EV
owner=compiler.compiler-bridge:neutral bundle and session CustomBlockDescriptor consumption
conflict_domains=compiler_execution,source_lineage,host_service_graph
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
size=M
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/compiler-compiler-bridge/CCA2EH.md
max_production_loc=650
max_production_files=6
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA2EH — Neutral custom-block consumer migration

## Independently acceptable outcome, role, and owners

Migrate the neutral compiler bundle, Vue main assembly inputs, and session virtual custom-node population to consume `CustomBlockDescriptor` directly while retaining the legacy descriptor declaration only for terminal deletion. Current consumers reconstruct language/type/content from `RuntimeCustomBlock` plus session metadata; final consumer ownership is the source-backed descriptor and its staged relation.

## Expected production surfaces and named boundary

- `crates/verter_compiler/src/framework_common/carrier_compiler.rs`/`mod.rs` and `standalone.rs` — carry descriptors through the neutral bundle without reducing them.
- `crates/verter_session/src/compile.rs` — use descriptor identity/order for Vue custom imports/invocations without reconstructing source facts.
- `crates/verter_session/src/host_resolve/virtual_file_pipeline.rs` and bounded session handoff types — populate virtual custom nodes from descriptor content state, role/tag, language, and source identity.

Own neutral transport and session consumption only. Vue production is CCA2EV; descriptor schema is CCA2E0; terminal legacy type/export deletion is CCA2E. No plugin ABI, transform, provider load, or new public wire shape enters.

## Exact predecessor contract and binding architecture

- **CCA2EV:** each Vue custom block has one complete descriptor bound to registered source/parse identity and preserves legacy behavior through a bounded adapter.
- Consumers preserve descriptor source order/identity/provenance and use content only when its typed state makes it available. `src` is not fetched; unavailable content is not replaced with empty text.
- The current Svelte runtime producer cell remains bounded inapplicable and zero-work. Generic transport accepts no invented Svelte descriptor stream and contains no framework-specific branch.

## Migration, exact deletions, acceptance, and performance

Characterize Vue virtual custom-node IDs/content/lang/type/import-invocation behavior, switch neutral transport and session consumers atomically, then delete all conversion/reconstruction helpers made unreachable. Retain only the unreferenced legacy declaration/export for CCA2E.

- **CCA2EH-AC1:** neutral/session consumers take descriptors directly; planted metadata reconstruction, empty-content fallback, reorder, or source alias fails.
- **CCA2EH-AC2:** virtual IDs, import/invocation order, role/tag/lang/src/attrs/region/content state, diagnostics, and staged relations remain deterministic and source-bound.
- **CCA2EH-AC3:** fresh/incremental/edit-revert agree; stale/cancelled/partial descriptors cannot publish or warm.
- **CCA2EH-AC4:** one block is transported once with no source load, transform, semantic pass, duplicate content copy, or retained stale descriptor; absent and Svelte-inapplicable cells are zero-work.

Ceiling: 650 production LOC, 6 production files, 2 crates. Abort on custom-block semantics/plugin ABI, provider I/O, invented Svelte production, public-wire change, terminal legacy deletion beyond the named conversion helpers, or a seventh production file. Run compiler bundle/assembly and session virtual/host/custom-block tests plus `targeted-domain`; CCA2E performs terminal deletion.
