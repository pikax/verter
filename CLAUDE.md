# Verter

Verter is a Vue compiler and Language Server Protocol (LSP) implementation. It converts Vue Single File Components (SFCs) to valid TSX (leveraging TypeScript for type checking) and compiles templates to optimized render functions. Unlike Volar, Verter generates actual valid TSX code rather than virtual files.

The project is a hybrid Rust + TypeScript monorepo: Rust crates handle template compilation (exposed via NAPI-RS native bindings and wasm-bindgen WASM) and the LSP server (`verter_lsp` binary, communicates over stdio), while TypeScript packages handle the SFC-to-TSX transformation and IDE integration.

## Architecture

Detailed module reference, key files, and implementation specifics are available in domain skills: `/type-resolution`, `/component-meta`, `/compiler-codegen`, `/host-session`, `/architecture`.

### Shared Optimized Codebase (CRITICAL)

Verter is one shared optimized codebase, not separate semantic implementations per consumer.

- Improvements should land in the lowest reusable owner crate that can correctly serve all consumers.
- `verter_session` and shared workspace/VFS integration are the authority for host-backed loading, invalidation, dependency tracking, and cache reuse.
- `verter_semantic` and `verter_compiler` own reusable semantics, lowering, and codegen.
- `verter_session::resolver_core` owns the host-backed resolver stack and type-resolution orchestration.
- `verter_protocol` owns transport-facing schema DTOs, while `verter_ffi` remains the thin native/WASM adapter layer.
- Consumer packages such as `@verter/component-meta`, the LSP, MCP, unplugin, and playground should consume the shared substrate rather than carrying their own semantic forks.

Architectural consequence:

- A performance or correctness fix discovered in one surface should be implemented in the shared owner layer whenever that behavior is reusable.
- Consumer-local wrappers should stay thin and should not bypass shared parsing, analysis, resolution, or cache ownership.

### Build Philosophy (CRITICAL)

This project follows the same end-state philosophy as `binary-exploring-lamport.md`.

Core rules carried forward:

1. Read, parse, shallow-process, and cache each canonical file once per content hash through one shared host path.
2. Store the full shallow symbol inventory up front, then process only the requested items on demand.
3. Same-file closure stays local to the owning file.
4. Cross-file deepening happens in one place only, one import level at a time.
5. The builder/solver reads only from cached lookup state; it does not reopen file loading or routing.
6. The entire design is demand-driven and query-scoped.
7. The final implementation lands as one clean cutover, not as a merged dual-path transition.
8. Component-meta, LSP, MCP, and other host-backed consumers must share the same file-ready/read/parse/shallow-process lifecycle.

These are architecture rules, not optimization hints. If a change conflicts with them, fix the owner layer or delete the legacy path rather than preserving a second read/parse/resolution flow.

### Shallow File Processing Core Invariant (CRITICAL)

The shallow file process is a core architectural invariant and must be preserved.

When a canonical file is processed, the host stores its shallow symbol inventory once. That inventory is the authoritative index later stages query.

At minimum, the shallow state must classify and retain:

- imports
- exports and reexports
- type declarations
- interfaces
- enums
- classes
- variables/constants
- functions/method signatures
- `typeof`-relevant value declarations
- local symbol dependency edges
- cross-file dependency edges

Design rule:

- processing a file means collecting and indexing its symbols, not eagerly evaluating them
- later stages look up the indexed items they need and only process those items on demand
- no stage should need to rescan the raw file to rediscover symbols that shallow processing already captured

Performance consequence:

- very high performance comes from targeted demand after broad shallow indexing, not from repeated partial reparsing

Architectural target for the project-global cache cutover:

- the canonical post-parse artifact is `IndexedReady`
- `IndexedReady` owns canonical imports/exports plus compact owned symbol indexes, spans, operator tags, interned names, and shallow bodies that are safe for host-owned `Send + Sync` caches
- parse each live file version once through the scheduler, then lower only the shallow syntax needed by later passes into the long-lived shared representation
- transient OXC parse arenas are per-file and per-version only; they may be dropped after lowering completes and must not leak into host-owned shared caches
- component-meta and later analysis layers must both build from `IndexedReady`
- if analysis or component-meta expands a symbol, they must populate and reuse the same shared resolver caches rather than introducing separate expansion paths
- type navigation must stay narrower than expansion: walking `A['c']['full']['bar']` should navigate intermediate hops and expand only the terminal requested projection unless limited normalization is required to continue
- generic substitutions are part of semantic meaning; navigation and expansion must operate on instantiated types and cache keys must include the relevant substitutions/type arguments
- navigators must stay non-owning: they may choose the next hop and perform non-owning normalization, but any reusable semantic work must enter through the shared query API rather than a private drill-down path
- the shared semantic layer should be keyed by semantic query identity and store immutable semantic data or ids, not borrowed AST pointers or retained parser arenas
- top-level live-host results must publish through a completion fence: record touched dependency facts, revalidate before publish, retry at most 3 times on mid-flight changes, and never warm shared caches with torn provisional results
- distinct top-level waiters on in-flight semantic/artifact work must block cooperatively on completion rather than busy-spinning; same-path recursion must never self-await
- reusable cache population must be path-independent: if the same semantic result is computed from different entry points, it must populate the same shared cache entry
- broader successful results may backfill narrower cache entries they actually satisfied, but narrower results must not pretend broader work is cached
- final payload caches should hand out immutable `Arc` values and may use any backend that preserves concurrency, size bounds, and validation semantics
- cancelled, superseded, interrupted, budget-exceeded, or partial semantic results must not be promoted as warm shared cache entries

**Project-global cache (final state):** `VerterHost` owns a single `ProjectTypeStore` accessed via `.project_type_store()`. The store owns `IndexedReadyDb`, `AnalysisReadyDb`, the rehomed `RouteDb`, `OwnerImportSurfaceDb`, `ComponentMetaResultDb<ComponentMetaAnalysis>`, `SemanticGraphStore`, and the `IntrinsicRegistry`. Vue macro resolution artifacts (the former `ResolvedNamedTypesDb`) live inside `SemanticGraphStore` via `SemanticNodeData::VueMacroElements` and the `HostResolvedNamedTypeKey` identity map — the parser's `NamedTypeCache` adapter hits the graph directly on the refcount-only hot path via `SemanticGraphStore::get_resolved_named_type`. `IndexedReady` is the single canonical post-parse artifact (the former `ModuleFactsDb` has been retired). `get_component_meta` consults the final-result cache first (with `HostFenceValidator` dep-signature revalidation) before falling back to the cold resolver. Direct owner imports resolve through `resolve_owner_direct_import` once per `(owner, whole_hash)`. Semantic subqueries dedup through `SemanticGraphStore::execute_cooperative` via `ProjectSemanticDispatch::execute` — every `SemanticQueryKey` variant dispatches through this memo. Intrinsic dispatch routes through `IntrinsicRegistry::lookup` — the SDK audit test asserts every `= intrinsic` declaration in `lib*.d.ts` has a registry entry. Validated cache writes record dep-signatures (`HostFenceValidator` revalidates on warm hit). Host-backed resolvers use `HostStoreView` directly; the request-view-era `RequestStoreView`, `CURRENT_REQUEST_VIEW`, and the `_in_view` signature surface are fully retired.

### Canonical Dependency Cache Rule (CRITICAL)

Host-backed type/import resolution must treat the canonical file ID as the cache identity. Load and parse each dependency at most once per canonical ID per workspace content generation. Cache the parsed state, eval env, symbol/export tables, and prepared declarations together. Later lookups hit cached maps — never rewalk the AST. VFS is the authority for file-change invalidation. Concurrent cold requests to the same file must collapse onto one materialization path. Architectural changes land as one clean cutover with no dual-path shims.

See `/type-resolution` skill for the full rule set (invalidation semantics, route caches, prepared declarations, cross-owner reuse, negative caching, and the concrete performance contract).

### Macro Type Traversal Rule (CRITICAL)

When resolving cross-file macro types (`defineProps<T>()`, `defineEmits<T>()`, component-meta deep expansion, etc.), only follow the import graph reachable from the requested type's declaration graph. There is one shared cross-file type resolver with four query modes: `Identity`, `Navigate`, `Shallow`, `Expanded` (see `/type-resolution` → Query Mode Contract). Path projection is path-precise: intermediate hops run in `Navigate`, the terminal hop runs in the caller's mode, non-contributing intersection arms are ignored (not rewritten to `never`), open conditionals distribute the remaining path into both branches, closed conditionals reduce immediately. Do not walk unrelated imports. Do not treat plain imports as implicit exports. Cache discovered symbol mappings and barrel hops.

**TS-first resolution priority:** TypeScript types always take priority over JavaScript files. Use `effective_target()` which selects: `.d.ts` > `.d.cts` > `.d.mts` > `.ts` > `.tsx` > `.js` > `.jsx` > `.cjs` > `.mjs`.

**Owned resolution is bounded by `workspace_root`:** `node_modules` and package `#imports` ancestor walks stop at `IdeProjectConfig.workspace_root`.

See `/type-resolution` skill for the full traversal rules and resolver mode details.

### Two Template Codegen Paths (CRITICAL)

The Rust compiler has two separate template codegen paths. Modifying one does NOT affect the other: **VDOM/Vapor** (`template/code_gen/vdom/`) for runtime render functions, and **IDE** (`ide/template/`) for valid JSX/TSX used by LSP/TSGO type checking. The LSP uses the IDE path via `CompileTarget::IDE`.

See `/compiler-codegen` skill for full codegen pipeline, backends, and CompileTarget details.

### Fallthrough / Root Inheritance (CRITICAL)

The shared Rust pipeline owns all fallthrough and root inheritance semantics. `verter_semantic::analysis` extracts root reachability facts only. `verter_session` owns the single inheritance resolver, recursion, conditional branch composition, generic propagation, caching, and final metadata projection.

Key rules: `inheritAttrs: false` → no inherited surface. Single native root → intrinsic attrs minus declared props/events. Single component root → recursive propagation. Conditional branches → exact union. Cycles → unresolved branches. `class`/`style` are never consumed.

See `/component-meta` skill for the full semantic rules, public contract, authority chain, and key files.

### Component-Meta Native Vs Compat (CRITICAL)

The native component-meta payload is the semantic authority. `@verter/component-meta/compat` is a projection layer for `vue-component-meta` interoperability, not a second semantic pipeline.

Core rules: Fix metadata in the native layer first. Rust owns resolution, declaration routing, and graph construction. One async native request per query. JS may transform structure but must not recover meaning. JS must not become a second resolver or expander. Cache-owned type recovery only — no AST/source fallbacks.

See `/component-meta` skill for the full policy, resolver rules, and cache contracts.

### CodeTransform Is the Single Source of Truth (CRITICAL)

**All modifications to generated code MUST go through `CodeTransform` operations** (`overwrite`, `prepend_left`, `append_left`, `move_with_suffix`, etc.). Never apply string replacements, regex transforms, or manual splicing to the output of `build_string()` or to content that was produced by a `CodeTransform`.

Post-hoc string manipulation breaks sourcemap accuracy: the `CodeTransform` generates source maps by tracking chunks (Original, Inserted, Moved, Overwritten). If you modify the string after the transform, byte offsets in the source map no longer match the actual content. This causes position mismatches in the LSP (e.g., hover landing on the wrong token, go-to-definition jumping to wrong locations).

**Correct:** Use `ct.prepend_left(pos, ".ts")` to insert text at a known position — the chunk list and source map stay consistent.

**Wrong:** Call `content.replace(".vue'", ".vue.ts'")` on the built string — the source map still reflects the pre-replace byte offsets.

## Build

```bash
pnpm install                  # Install all dependencies
pnpm build                    # Build everything: native → lsp → wasm → ts packages
pnpm run build:native         # Build native .node bindings only
pnpm run build:lsp            # Build Rust LSP binary (debug)
pnpm run build:lsp:release    # Build Rust LSP binary (release, optimized)
pnpm run build:mcp            # Build MCP server binary (debug)
pnpm run build:mcp:release    # Build MCP server binary (release, optimized)
pnpm run build:wasm           # Build WASM + copy to playground
pnpm run build:ts             # Build all TypeScript packages
pnpm run build:playground     # Build the playground for deployment
```

`pnpm build` runs sequentially: native bindings first (needed by unplugin), then LSP binary (shares compiled Rust deps with native, avoids recompilation), then WASM (needed by playground), then all TS packages.

See `/build-and-profiling` skill for build dependency chains, rebuild sequences, and profiling setup.

## Development

```bash
pnpm watch                    # Watch-build TS packages for extension dev
pnpm dev-extension            # Build LSP binary, then watch language-shared + vscode extension + typescript-plugin
pnpm clean                    # Remove build artifacts
```

## Testing

### Running Tests

```bash
# TypeScript / JavaScript
pnpm test                                    # All JS/TS tests
pnpm vitest --run                            # All tests (non-watch)
pnpm vitest --run path/to/test.spec.ts       # Specific file

# Rust
cargo test --workspace --tests --verbose     # Default Rust verification for agents (workspace test targets only; skips doctests/examples)
cargo test --workspace --doc                 # Rust doctests only; run when rustdoc examples changed or explicitly requested
cargo test --package verter_compiler test_name   # Specific Rust test
cargo test --package verter_compiler 2>&1 | tail -60  # Full suite with truncated output
```

### End-of-change Checks

Run these after **every** change. Verter's crates are highly interconnected — a change in one crate frequently breaks tests in dependent crates. Always run the full workspace suite:

```bash
cargo test --workspace --tests --verbose 2>&1 | tee /tmp/test-output.txt
cargo clippy --fix --allow-dirty --allow-staged --workspace -- -D warnings
cargo fmt --all
pnpm install --frozen-lockfile   # Verify lockfile is in sync (CI uses this)
```

For TypeScript changes, also run `pnpm test`. Do not skip workspace-wide testing even for "small" changes.

**Agent test policy:** Do not run bare `cargo test --workspace` by default. In this repository it pulls in doctests and example builds, which adds substantial runtime without improving the normal agent verification loop. Run doctests only when rustdoc examples changed or the user explicitly asks for them.

### Documentation Updates

After adding, changing, or removing features, update the **owning** documentation:

- **Domain skills** (`.claude/skills/`) — update the skill that owns the affected module or API
- **`CLAUDE.md`** — update only if summaries or skill pointers change
- **`AGENTS.md`** — update if skill routing or shared sources change
- **`docs/`** — API docs, guide pages, contributing guides
- **Inline doc comments** — Public API rustdoc (`///`) and JSDoc (`/** */`) on changed signatures

Skip this for purely internal refactors that don't change any public behavior, module paths, or APIs.

### Testing Requirements

**MANDATORY: TDD must be followed for EVERY code change. Non-negotiable.**

1. Write failing tests FIRST — verify they fail before implementing
2. Implement minimum code to pass
3. Run tests, verify green
4. Refactor while keeping tests green

Coverage: new features need tests, bug fixes need regression tests, refactors must keep existing tests passing.

**Always include negative assertions**: verify both what SHOULD and should NOT be present. Codegen tests must check removed syntax is absent. Type tests must include `@ts-expect-error` guards against `any`/`never`.

**Rust test file organization**: When inline `#[cfg(test)]` exceeds ~400 lines, extract to a sibling `*_tests.rs` file.

See `/testing` skill for full TS/Rust test patterns, sourcemap testing, and server cleanup.

### VS Code Extension Testing (MANDATORY)

Changes to the VS Code extension or the LSP server MUST be verified with automated tests, NOT manual testing. Unit tests (Vitest) for pure logic, E2E tests (Mocha) for LSP integration features.

See `/testing` and `/e2e-vscode-testing` skills for commands, fixture design, and helpers API.

## Agent Implementation Rules

### Planning

Prefer architecturally correct, long-term solutions over easy or quick implementations. Evaluate approaches by correctness and durability, not by implementation speed.

The codebase expects the best architecture for the problem. Time constraints, implementation size, migration breadth, anticipated breaking changes, or the fact that something is "a lot of work" are not valid reasons to weaken the design, preserve a compromised path, or diverge from the approved plan. If the correct implementation is larger or breaking, plan for it explicitly or raise it before execution; do not silently ship an architectural deviation.

Do not provide time estimates unless the user explicitly asks for one. Do not use estimated effort, duration, or perceived time cost as a factor for doing, not doing, or partially doing planned work; approved plans already account for timing expectations.

Plans must include these sections:
1. **Context** — why this change is being made
2. **Changes** — specific files to modify with concrete modifications
3. **Legacy Deletions** — explicit list of files, functions, code paths, and feature flags to remove
4. **Verification** — full workspace test commands and expected outcomes

Without explicit legacy deletion lists, agents skip deletions and leave dual paths alive.

### Execution

Execute plans fully in one pass without intermediate checkpoints unless explicitly requested. Do not stop mid-plan to ask for confirmation on steps already approved in the plan.

Once execution starts, complete the approved plan end-to-end in the same pass. Do not pause, defer scope, or leave planned work unfinished because of estimated time or effort unless the user explicitly changes the request.
Do not rewrite the plan into a smaller or safer variant during execution because the correct path is breaking, broad, or labor-intensive. Approved plans are expected to land as written unless the user explicitly re-scopes them.

### Self-Review

After completing a plan, review the full implementation before declaring done:
- Verify all plan steps were executed
- Check for missed edge cases or incomplete migrations
- Run the full workspace test suite (see End-of-change Checks above)

### Legacy Code Deletion

When replacing a feature or refactoring a system, delete the superseded code in the same change. Do not add shims, double branches, compatibility wrappers, or feature flags to preserve old behavior alongside new behavior. If unsure whether specific files or code paths should be preserved, ask the user explicitly rather than silently keeping them.

### Fix Quality

When encountering issues during implementation:
- If the correct fix aligns with the architecture → implement it properly
- If the fix would be a workaround, patch, or shim → do NOT apply it. Instead: add a `TODO(follow-up)` comment explaining the proper fix needed, note it in the feedback file, and continue with the plan
- Never apply a dirty fix that contradicts architectural rules just to make tests pass
- A clean TODO with a follow-up plan is always better than a quick patch that accumulates debt

### Stub Prevention (CRITICAL)

Do not use empty test bodies, trivially-passing stubs, or "deferred to follow-up commit" placeholders to satisfy a named contract — a gate check, a characterization test, a plan invariant, a review obligation, or a declared completion criterion. A stub that happens to pass is a gate-bypass, not a pass.

**Concrete anti-patterns, all forbidden on landed/mainline commits:**

- **Empty `#[test]` bodies.** `#[test] fn verifies_cycle_guard_terminates_on_recursion() {}` passes trivially and proves nothing. An un-ignored empty-body test is worse than an `#[ignore]`'d one — it falsely advertises coverage. If the test body cannot be written yet, keep `#[ignore]` on the test until the implementation lands.
- **Unconditional "unknown" / "default" returns as "scaffolding".** `fn relate_nodes(...) -> RelationResult::Unknown` that always returns Unknown is not a relation-engine scaffold; it is a nop. `fn resolve(...) -> Opaque(Miss)` that always returns Miss is the same defect. Either write real logic, or use `todo!()` / `unimplemented!()` so callers panic loudly and the nop is obvious from any first call.
- **"Real body deferred to follow-up commit."** A commit that claims to satisfy a gate via a stub, with the plan of a later commit "fleshing it out", is bypassing the gate, not passing it. The gate reflects implementation state on the tree under review, not future intent.
- **Always-true assertions.** `assert!(true)`, `assert_eq!(1, 1)`, `assert!(result.is_ok() || true)` — any predicate that holds regardless of the code under test is a stub in disguise.
- **Characterization tests that do not discriminate.** A characterization test must be writable such that it FAILS against the pre-change codebase AND PASSES against the post-change codebase. If that property does not hold, the test is not characterizing anything.

**Rule of thumb:** for every assertion you commit, ask "would this test catch the bug the cutover was written to fix?". If no, the test is a stub.

**WIP exemption.** Scratch branches that will be squashed (e.g., `staging/*` → squash-merge to mainline) may contain `todo!()` bodies, empty tests, and placeholder returns — that is their purpose. The rule applies to the squashed/landed commit, to any PR branch, and to any gate evaluated on the final tree. A landed commit message cannot cite "stub satisfies gate mechanically" as a legitimate state; that statement itself is a self-identified gate-bypass.

**Self-review obligation.** Before concluding a step that un-ignores or adds tests, re-open each test file and verify bodies are non-empty and assertions are discriminating. Before concluding a step that implements a function, verify the body exercises its inputs (branches on them, calls through to real logic) rather than returning a constant.

### Agent Feedback Capture

During work sessions, agents MUST continuously log feedback to a per-conversation file at `.claude/feedback/feedback-{YYYY-MM-DD}-{short-id}.md`. The `.claude/feedback/` directory is gitignored.

**What to log** — append entries whenever encountering something noteworthy:

- `[issue]` — bugs, unexpected behavior, workarounds applied
- `[improvement]` — code quality, performance, architecture ideas
- `[debt]` — things that work but could be better
- `[docs]` — missing or outdated documentation discovered

**Format**: `- [{category}] \`{file_path}\` — Brief description`

When delegating to subagents, pass the feedback file path and instruct them to append observations. One feedback file per conversation session.

## Dependencies Policy

- Keep dependencies at their latest versions
- Rust deps: update in `Cargo.toml`, run `cargo update`
- JS deps: `pnpm up -r -i -L` to interactively update all
- `workspace:^` deps are rewritten by `pnpm publish` automatically

## Commit Convention

This project uses **conventional commits** for automatic changelog generation via [git-cliff](https://git-cliff.org/).

```
<type>(<scope>): <description>

Types:
  feat     - New feature
  fix      - Bug fix
  perf     - Performance improvement
  refactor - Code refactoring (no behavior change)
  docs     - Documentation only
  test     - Adding/updating tests
  chore    - Build, CI, tooling changes
  release  - Version bump and release

Scopes:
  core     - verter_compiler Rust crate
  napi     - verter_napi / @verter/native
  wasm     - verter_wasm / @verter/wasm
  play     - playground
  unplugin - @verter/unplugin
  lsp      - language-server
  types    - @verter/types
  ts       - @verter/core (TypeScript)
  meta     - @verter/component-meta
  ci       - CI/CD workflows
  *        - multiple areas

Examples:
  feat(core): add v-memo directive support
  fix(wasm): correct memory leak in compile()
  chore(ci): add nightly WASM build workflow
  release(all): v0.0.1-alpha.1
```

## CI/CD

See [docs/contributing/ci-cd.md](docs/contributing/ci-cd.md) for detailed CI/CD documentation including:

- Workflow specifications (CI, nightly, release)
- Pre-release versioning flow (alpha → beta → rc → stable)
- Publishing process (npm + crates.io)
- Nightly WASM builds and playground deployment
- Required GitHub secrets configuration

## Skills Reference

Detailed reference material is available as on-demand skills (loaded automatically when relevant):

| Skill                  | Use When                                                                                         |
| ---------------------- | ------------------------------------------------------------------------------------------------ |
| `/type-resolution`     | Type solver, cross-file types, ShallowFileState, frontier engine, cache rules, macro traversal   |
| `/component-meta`      | Component metadata extraction, native/compat boundary, fallthrough, root inheritance             |
| `/compiler-codegen`    | Template codegen (VDOM/IDE), CodeTransform, cached directives, strict slots, style preprocessing |
| `/host-session`        | TypeProvider (TSGO/tsserver), workspace management, async scheduler, LSP host integration        |
| `/architecture`        | High-level module map, TS packages, plugin system, CSS analysis, MCP server, analysis types      |
| `/position-encoding`   | Span types, position encoding, coordinate conversions, path normalization                        |
| `/build-and-profiling` | Build order, rebuild sequences, profiling, MCP server setup                                      |
| `/testing`             | Test patterns, TDD workflow, sourcemap testing, server cleanup                                   |
| `/e2e-vscode-testing`  | VS Code E2E test fixtures, helpers API, adding new tests                                         |
| `/wsl-e2e-testing`     | WSL E2E tests to reproduce Linux/CI failures, fixture matrix                                     |
| `/rust-performance`    | Rust optimization patterns, allocation hierarchy, CodeTransform API                              |
