<!-- unified-charter-v2
id=CCA2DS
name=Svelte qualified-style consumer migration
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=migration
semantic_role=delivery
class=compiler
predecessors=CCA2D0
owner=compiler.compiler-bridge:Svelte stage-qualified external-style consumption
conflict_domains=style_semantics,compiler_execution,svelte_product
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
charter=charters/compiler-compiler-bridge/CCA2DS.md
max_production_loc=650
max_production_files=6
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA2DS — Svelte qualified-style consumer migration

## Independently acceptable outcome, role, and owners

Migrate Svelte external-style production/consumption to CCA2D0's stage-qualified continuation while retaining the legacy unqualified DTO only for unmigrated Vue consumers. Current Svelte external CSS is transported as `RuntimeStyleBlock`; final Svelte runtime/assembly paths publish one qualified J-owned result. Reverting restores only the Svelte adapter.

## Expected production surfaces and named boundary

- `crates/verter_compiler/src/svelte/carrier.rs` and bounded Svelte runtime CSS output helpers — construct/consume the qualified external-style artifact.
- `crates/verter_compiler/src/standalone.rs` or compiler request construction — carry qualified Svelte style input without reconstructing it.
- `crates/verter_session/src/host_resolve/compile_request_build.rs` and/or `virtual_file_pipeline.rs` — hand the completed J-owned Svelte style result to the compiler once.

Own Svelte external-style consumer selection, stage/basis validation, scope hash/`has_global`, diagnostics, source order, and qualified map transport. Vue, preprocessing algorithms, formatter behavior, and terminal legacy DTO deletion are excluded.

## Exact predecessor contract and binding architecture

- **CCA2D0:** additive qualified input/artifact construction exists and rejects wrong stage, stale basis, source mismatch, duplicate continuation, partial results, and unqualified maps.
- One applicable Svelte style block consumes one exact completed result. No fallback to raw supplied CSS, re-preprocessing, framework guessing, or second map chain is allowed after route selection.
- Identity binds source/style slot, revision, dialect, completed stage, content hash/basis, options, provenance, scope hash/global fact, and map source/generated spaces.

## Migration, exact deletions, acceptance, and performance

Characterize current Svelte external CSS bytes/maps/diagnostics/scope/global behavior, switch Svelte request and runtime consumers atomically, then delete only Svelte-side unqualified fields/adapters made unreachable. Retain shared legacy declarations for CCA2DV/CCA2D.

- **CCA2DS-AC1:** every applicable Svelte consumer requires the qualified boundary; a planted `RuntimeStyleBlock`/raw-style fallback fails.
- **CCA2DS-AC2:** CSS bytes, qualified maps, diagnostics, scope hash, global fact, canonical IDs, and deterministic source order remain equivalent.
- **CCA2DS-AC3:** fresh/incremental/edit-revert agree; wrong/stale/cancelled/partial style input cannot publish or warm.
- **CCA2DS-AC4:** one style slot performs one continuation with no duplicate preprocess, parse, transform, map encode, or source copy; absent/inapplicable style is zero-work.

Ceiling: 650 production LOC, 6 production files, 2 crates. Abort on Vue mutation, CSS semantic/preprocessor work, terminal shared DTO deletion, or a seventh production file. Run Svelte runtime/style/conformance/host/map suites and `targeted-domain`; CCA2D terminal deletion waits for this and CCA2DV.
