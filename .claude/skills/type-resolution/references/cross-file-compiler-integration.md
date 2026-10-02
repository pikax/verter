## Cross-File Type Resolution (Compiler Integration)

Typed Vue macro semantics are projected by the host into compiler-facing DTOs:

1. `produce_vue_macro_codegen` inventories one already-indexed SFC under one
   request-bound resolver context.
2. It resolves each typed macro root through `ProjectSemanticDispatch` and
   independently produces runtime and/or terminal TSC entries.
3. It returns `MacroRuntimeBundle` / `MacroTscBundle` entries keyed by stable
   macro `syntax_index`, plus the exact transitive canonical footprint.
4. The compiler consumes the explicit `VueMacroSemanticInput`; it never
   resolves typed macro parameters or merges companion/external type maps.

The producer output is request-local. Its aggregate bundle is not a durable
cache; underlying semantic query nodes retain their normal memo and
singleflight behavior. The Rust compiler performs no file I/O and is not a
type-resolution authority.

**Shallow file state and semantic-dispatch integration:**
`ShallowFileState` (the shallow symbol/export surface per imported file,
keyed by `(canonical_id, whole_hash)`) and `ExternalTypeFrontier` (the
layered BFS route/dependency engine) are the shared cross-file discovery
primitives. Local closure runs same-file dependencies iteratively without
crossing import boundaries. The frontier records route and dependency facts;
it does not evaluate a terminal type body. See
`resolver_core/shallow_file_state.rs`,
`resolver_core/external_type_frontier.rs`, `host_resolve/frontier_engine.rs`,
and `resolver_core/imported_root_db.rs`.

All query-time expansion for macro types, component-meta, and imported aliases
enters through `ProjectSemanticDispatch::execute`. Consumer-specific framework
surfaces and macro DTOs are terminal projections of the shared graph result.
`type_eval.rs` is a content-free declaration inventory, and `type_solver/`
contains retained DTOs only; neither is an execution engine.

