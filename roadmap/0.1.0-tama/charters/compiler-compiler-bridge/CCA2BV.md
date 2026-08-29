<!-- unified-charter-v2
id=CCA2BV
name=Vue semantic module assembly migration
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=migration
semantic_role=delivery
class=compiler
predecessors=CCA2A
owner=compiler.compiler-bridge:Vue semantic module assembly authority
conflict_domains=compiler_execution,host_service_graph,vue_product
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
charter=charters/compiler-compiler-bridge/CCA2BV.md
max_production_loc=700
max_production_files=7
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA2BV — Vue semantic module assembly migration

## Independently acceptable outcome, role, and owners

Move Vue main-module assembly, including script/template contribution order and host decoration, behind the Vue runtime compiler authority and emit CCA2A `CompileArtifactSet` relations through a behavior-preserving host adapter. Current semantic topology is split between compiler fragments and session `assemble_vue_main_module`; final semantic assembly ownership is the Vue compiler backend. Svelte remains untouched and independently usable.

## Expected production surfaces and named boundary

- `crates/verter_compiler/src/assembly/vue_module.rs` and bounded `assembly/compose.rs`/`assembly/publish.rs` — own Vue contribution topology and staged artifacts.
- `crates/verter_compiler/src/framework_common/vue_bridge.rs` — emit the compiler-owned Vue main artifact and its typed relations.
- `crates/verter_session/src/compile.rs` — reduce `assemble_vue_main_module` to transport decoration or delete displaced semantic assembly decisions.
- `crates/verter_session/src/host_resolve/virtual_file_pipeline.rs` — consume already-assembled Vue artifacts through the temporary handoff adapter.

The boundary owns Vue script/template/style/custom virtual imports, HMR/SSR/client topology, declared imports/exports, dialect, qualified maps, and provenance. Host lifecycle/publication, Svelte assembly, style-continuation semantics, and custom-block schema are excluded.

## Exact predecessor contract and binding architecture

- **CCA2A:** stable artifact identities, roles, relations, provenance, content states, deterministic order, and source/generated map qualification exist without route mutation.
- One Vue request has one assembly authority. Generic session code may add only host-owned identifiers/transport decoration and may not infer framework module topology.
- Artifact identity binds source unit/revision, framework, product, options, contribution order, and exact source-map basis; stale, cancelled, refused, or partial assembly publishes and warms nothing.

## Internal subblocks, migration, and exact deletions

1. Characterize Vue main bytes/maps/diagnostics and HMR/SSR/client/import-export topology.
2. Produce the Vue staged main artifact behind the runtime backend while the host adapter transports it.
3. Atomically switch the Vue branch and delete only displaced session-owned Vue semantic assembly decisions.

No dual assembly, text scanning to recover imports/exports, hidden fallback to host composition, or Svelte mutation is allowed. Retain the bounded adapter needed by CCA2C/CCA2F.

## Acceptance, performance, aborts, and verification

- **CCA2BV-AC1:** Vue compiler code is sole semantic module assembler; planted host-side topology reconstruction fails.
- **CCA2BV-AC2:** main bytes, qualified maps, diagnostics, roles, imports/exports, HMR/SSR/client behavior, and deterministic order remain equivalent.
- **CCA2BV-AC3:** fresh/incremental/edit-revert agree; cancellation/refusal/partial results cannot publish or warm.
- **CCA2BV-AC4:** one request performs one assembly and no duplicate parse, semantic, compile, compose, map encode, source copy, or retained candidate; unrequested main output is zero-work.

Ceiling: 700 production LOC, 7 production files, 2 crates. Abort on host lifecycle/publication mutation, a Svelte surface, style/custom schema work, or an eighth production file. Run Vue assembly/direct-result/host/map suites and `targeted-domain`; CCA2B joins this result with CCA2BS.
