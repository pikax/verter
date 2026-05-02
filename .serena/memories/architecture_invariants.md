# Architecture Invariants

Core principle: Verter is one shared optimized codebase, not separate semantic implementations per consumer. Correctness and performance fixes should land in the lowest reusable owner layer that can serve all consumers.

Ownership rules:
- `verter_session` and shared workspace/VFS integration own host-backed loading, invalidation, dependency tracking, cache reuse, workspace/session orchestration, and component-meta final-result caching.
- `verter_semantic` and `verter_compiler` own reusable semantics, lowering, and codegen.
- `verter_session::resolver_core` owns the host-backed resolver stack and type-resolution orchestration.
- `verter_protocol` owns transport-facing schema DTOs.
- `verter_ffi`, NAPI, WASM, LSP, MCP, component-meta TS packages, unplugin, and playground should stay thin adapter/consumer layers.

Build philosophy and shallow processing:
- Read, parse, shallow-process, and cache each canonical file once per content hash through one shared host path.
- Store full shallow symbol inventory up front, then process requested items on demand.
- Same-file closure stays local to the owning file.
- Cross-file deepening happens in one place only, one import level at a time.
- Builders/solvers read cached lookup state and must not reopen file loading/routing.
- Design is demand-driven and query-scoped.
- Architectural changes land as clean cutovers, not dual-path transitions.
- Host-backed consumers share the same file-ready/read/parse/shallow-process lifecycle.

Canonical dependency cache rule:
- Host-backed type/import resolution uses canonical file ID as cache identity.
- Load and parse each dependency at most once per canonical ID per workspace content generation.
- Cache parsed state, eval env, symbol/export tables, and prepared declarations together.
- VFS is the authority for file-change invalidation.
- Concurrent cold requests to the same file collapse onto one materialization path.

Type/macro traversal:
- For cross-file macro types (`defineProps<T>()`, `defineEmits<T>()`, component-meta expansion), follow only the import graph reachable from the requested type's declaration graph.
- Do not walk unrelated imports. Do not treat plain imports as implicit exports.
- Query modes include `Identity`, `Navigate`, `Shallow`, `Expanded`, and `Skeleton`; path projection should be path-precise.
- TypeScript types take priority over JavaScript: `.d.ts` > `.d.cts` > `.d.mts` > `.ts` > `.tsx` > `.js` > `.jsx` > `.cjs` > `.mjs`.
- Owned resolution is bounded by `workspace_root`; `node_modules` and package `#imports` ancestor walks stop there.

Component-meta rules:
- Native Rust component-meta payload is semantic authority.
- `@verter/component-meta/compat` is a projection layer for `vue-component-meta` interop, not a second resolver/expander.
- Rust owns resolution, declaration routing, graph construction, fallthrough/root inheritance, recursion, conditional branch composition, generic propagation, caching, and final metadata projection.
- JS may transform structure but must not recover meaning from AST/source fallbacks.
- `inheritAttrs: false` means no inherited surface; single native root inherits intrinsic attrs minus declared props/events; single component root propagates recursively; conditional branches become exact unions; cycles become unresolved branches; `class`/`style` are never consumed.

Compiler/codegen:
- Rust compiler has two separate template codegen paths: VDOM/Vapor runtime render functions under `template/code_gen/vdom/`, and IDE valid JSX/TSX generation under `ide/template/` for LSP/TSGO type checking.
- LSP uses the IDE path via `CompileTarget::IDE`.
- Generated code modifications must go through `CodeTransform` operations (`overwrite`, `prepend_left`, `append_left`, `move_with_suffix`, etc.). Do not post-process generated output with string replacement/regex/manual splicing after `build_string()` because that breaks sourcemaps.

Documentation ownership:
- Update the owning doc when public behavior, module paths, APIs, or durable architecture changes.
- Domain-specific changes usually update `.claude/skills/<domain>/SKILL.md`; update `CLAUDE.md` only for summaries/pointers and high-level rules; update `docs/` for user/contributor/API docs.