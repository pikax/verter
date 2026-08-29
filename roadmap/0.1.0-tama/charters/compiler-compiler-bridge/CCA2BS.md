<!-- unified-charter-v2
id=CCA2BS
name=Svelte semantic module assembly migration
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=migration
semantic_role=delivery
class=compiler
predecessors=CCA2A
owner=compiler.compiler-bridge:Svelte self-contained semantic module assembly authority
conflict_domains=compiler_execution,host_service_graph,svelte_product
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
charter=charters/compiler-compiler-bridge/CCA2BS.md
max_production_loc=650
max_production_files=6
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA2BS — Svelte semantic module assembly migration

## Independently acceptable outcome, role, and owners

Make the Svelte runtime compiler's self-contained ESM body the sole Svelte semantic module assembly and convert it directly to CCA2A staged artifacts through a behavior-preserving host adapter. Current session code branches on `main.body_code`; final Svelte topology ownership is wholly compiler-side. Vue remains untouched and independently usable.

## Expected production surfaces and named boundary

- `crates/verter_compiler/src/svelte/carrier.rs` and bounded Svelte runtime output helpers — emit the complete Svelte main artifact, map, language, diagnostics, and declared relations.
- `crates/verter_compiler/src/assembly/mod.rs`/`publish.rs` — admit the self-contained artifact into `CompileArtifactSet` without reconstructing it.
- `crates/verter_session/src/host_resolve/virtual_file_pipeline.rs` — consume the already-assembled Svelte artifact through the temporary handoff adapter.

The boundary owns Svelte client/SSR module body, runtime imports/exports, external-style relation, language, qualified map, and provenance. Host lifecycle/publication, Vue assembly, style-continuation semantics, and custom-block schema are excluded.

## Exact predecessor contract and binding architecture

- **CCA2A:** stable artifact identities, roles, relations, provenance, content states, deterministic order, and source/generated map qualification exist without route mutation.
- One Svelte request has one assembly authority. Generic session code transports the complete body and may not inspect or reconstruct Svelte topology.
- Artifact identity binds source unit/revision, framework, product, options, runtime mode, and exact source-map basis; stale, cancelled, refused, or partial assembly publishes and warms nothing.

## Internal subblocks, migration, and exact deletions

1. Characterize Svelte main bytes/maps/diagnostics, client/SSR topology, imports/exports, and external-style relation.
2. Convert the compiler-owned self-contained module to staged artifacts behind the Svelte runtime backend.
3. Atomically switch the Svelte branch and delete only session-side body-shape interpretation made redundant by the staged artifact.

No dual assembly, text scanning, generic framework branch, fallback to host composition, or Vue mutation is allowed. Retain the bounded adapter needed by CCA2C/CCA2F.

## Acceptance, performance, aborts, and verification

- **CCA2BS-AC1:** Svelte compiler code is sole semantic module assembler; planted host-side topology reconstruction fails.
- **CCA2BS-AC2:** main bytes, qualified maps, diagnostics, roles, imports/exports, client/SSR behavior, and deterministic order remain equivalent.
- **CCA2BS-AC3:** fresh/incremental/edit-revert agree; cancellation/refusal/partial results cannot publish or warm.
- **CCA2BS-AC4:** one request performs one assembly and no duplicate parse, semantic, compile, map encode, source copy, or retained candidate; unrequested main output is zero-work.

Ceiling: 650 production LOC, 6 production files, 2 crates. Abort on host lifecycle/publication mutation, a Vue surface, style/custom schema work, or a seventh production file. Run Svelte runtime/conformance/host/map suites and `targeted-domain`; CCA2B joins this result with CCA2BV.
