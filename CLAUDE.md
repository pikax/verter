# Verter

Verter is a Vue compiler and Language Server Protocol (LSP) implementation. It converts Vue Single File Components (SFCs) to valid TSX (leveraging TypeScript for type checking) and compiles templates to optimized render functions. Unlike Volar, Verter generates actual valid TSX code rather than virtual files.

The project is a hybrid Rust + TypeScript monorepo: Rust crates handle template compilation (exposed via NAPI-RS native bindings and wasm-bindgen WASM), while TypeScript packages handle the SFC-to-TSX transformation, LSP, and IDE integration.

## Architecture

### Package Dependency Graph

```
verter-vscode (VS Code extension)
├── @verter/language-server (LSP server)
│   ├── @verter/core (SFC → TSX transformation)
│   │   └── @verter/types (type utilities)
│   ├── @verter/language-shared (client/server protocol)
│   └── @verter/native (Rust template compiler, NAPI-RS)
├── @verter/typescript-plugin (IDE .vue import resolution)
│   └── @verter/core
└── @verter/unplugin (universal bundler plugin)
    └── @verter/native

@verter/playground (Netlify-hosted)
└── @verter/wasm (Rust template compiler, wasm-bindgen)
```

### Repository Structure

```
crates/
  verter_core/       # Core template compiler (Rust)
  verter_analysis/   # Static analysis: imports, exports, bindings, type resolution
  verter_host/       # In-memory file host: caching, dependency tracking, multi-file compilation
  verter_linter/     # Vue SFC linter engine: rule trait, visitor, diagnostics (depends only on verter_analysis)
  verter_ffi/        # FFI types: shared serializable structs for NAPI/WASM boundaries
  verter_bench/      # Benchmarks and comparison examples (Rust)
  verter_napi/       # Native Node.js bindings (NAPI-RS cdylib)
  verter_wasm/       # WASM bindings (wasm-bindgen cdylib)
packages/
  core/              # @verter/core - SFC parser & TSX transformer
  types/             # @verter/types - TypeScript utility types
  native/            # @verter/native - Native binding loader + platform packages
  wasm/              # @verter/wasm - WASM binding wrapper
  unplugin/          # @verter/unplugin - Universal bundler plugin
  language-server/   # @verter/language-server - LSP server
  language-shared/   # @verter/language-shared - Shared LSP protocol types
  typescript-plugin/ # @verter/typescript-plugin - TS language service plugin
  oxc-bindings/      # @verter/oxc-bindings - OXC parser binary helper
  playground/        # @verter/playground - Online playground (private, Netlify-hosted)
  vue-vscode/        # verter-vscode - VS Code extension
  example/           # Example project
scripts/
  check-versions.mjs # Version check + publish order for CI
```

### TypeScript Packages

| Package | Purpose | Entry Point |
|---------|---------|-------------|
| **`@verter/core`** | SFC parser & TSX transformer. Converts `.vue` files to valid TSX using `MagicString` for sourcemap preservation | `src/v5/index.ts` |
| **`@verter/types`** | TypeScript utility types (`PatchHidden`, `ExtractHidden`, `EmitsToProps`, etc.). Has `/string` export with `$V_` prefixed types for LSP injection | `src/index.ts` |
| **`@verter/language-server`** | LSP server implementation. Manages documents, provides completions, diagnostics, hover, go-to-definition | `src/server.ts` |
| **`@verter/language-shared`** | Shared protocol types between VS Code client and language server | `src/index.ts` |
| **`@verter/typescript-plugin`** | TypeScript plugin that resolves `.vue` imports in TS/JS files. Intercepts module resolution to return transformed TSX | `src/index.ts` |
| **`verter-vscode`** | VS Code extension. Bundles language server and TS plugin, handles extension activation | `src/extension.ts` |
| **`@verter/unplugin`** | Universal bundler plugin (Vite, Rollup, webpack, esbuild, rspack, Rolldown, Farm). Compiles `.vue` files via `@verter/native`. Supports `preCompile` for build-start cache warming | `src/index.ts` |
| **`@verter/oxc-bindings`** | Helper for downloading platform-specific OXC parser binaries | `src/index.ts` |

### Unplugin Configuration (`packages/unplugin/`)

`@verter/unplugin` provides a `VerterPluginOptions` interface:

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `componentId` | `(filename, source, isProd) => string` | hash-based | Custom component ID generator |
| `include` | `string \| RegExp \| (string \| RegExp)[]` | `[/\.vue$/]` | File patterns to include |
| `preCompile` | `boolean` | `false` | Pre-compile all `.vue` files during `buildStart`. Scans the project root, upserts files into the host cache (including type dependencies for macros), and compiles them. When `transform()` later receives the same content, the host returns the cached result instantly. `node_modules` are excluded from scanning. |
| `crossFileOptimize` | `boolean` | `false` | Cross-file prop constness optimization. Requires `preCompile: true`. After pre-compilation, analyzes the render tree to determine which props are always passed constant values, skipping dynamic tracking in compiled output. |
| `template` | `object` | — | Template compiler options (compat with `@vitejs/plugin-vue`) |

**`preCompile` architecture:**
- During `buildStart()`, scans the project root for `.vue` files (excluding `node_modules` and dot-directories)
- For each file: upserts it into the host, resolves external `src` attributes and macro type dependencies (e.g., `import type { Props } from './types'` used in `defineProps<Props>()`), then triggers compilation
- When another plugin modifies the file before `transform()`, the host detects the content change via internal hashing and recompiles
- Third-party `.vue` files in `node_modules` compile on-demand during `transform()` — no pre-compilation overhead

### Core Transformation Pipeline (`packages/core/src/v5/`)

```
Vue SFC → parser/ → process/script/plugins/ → TSX output
              ↓              ↓
         ParsedBlock    MagicString (preserves sourcemaps)
```

1. **`parser/`** - Parses SFC into typed blocks
   - `parser.ts` - Main entry, uses `@vue/compiler-sfc`
   - `types.ts` - `ParsedBlockScript`, `ParsedBlockTemplate`, `ParsedBlockUnknown`
   - `script/` - Extracts script AST items (`ScriptItem`, `ScriptTypes`)
   - `template/` - Parses template expressions and bindings

2. **`process/`** - Plugin-based transformation system
   - `script/script.ts` - Orchestrates plugin execution
   - `types.ts` - `ProcessContext`, `ProcessPlugin`, `ProcessItemType`

### Plugin System (`packages/core/src/v5/process/script/plugins/`)

Plugins transform parsed SFC items into TSX. Each plugin can:
- Hook into `pre`/`post` phases
- Transform specific `ScriptTypes` via `transformXxx` methods
- Add items to `context.items` for downstream plugins

| Plugin | Purpose |
|--------|---------|
| `macros/` | Transforms Vue macros (`defineProps`, `defineEmits`, `defineModel`, `defineSlots`, `defineExpose`, `withDefaults`) |
| `template-binding/` | Generates template binding type for IDE support |
| `binding/` | Tracks variable declarations for binding context |
| `imports/` | Handles import statements |
| `script-block/` | Wraps script setup content |
| `full-context/` | Generates component context type |
| `attributes/` | Processes component attributes |
| `resolvers/` | Resolves component references |

**Plugin execution order**: Controlled by `enforce: "pre" | "post"`. Pre-plugins run first, then main transforms, then post-plugins.

### Language Server Architecture (`packages/language-server/src/v5/`)

```
server.ts (LSP connection)
    ↓
documents/
├── manager/manager.ts    → DocumentManager (file tracking)
├── verter/manager/       → VerterManager (TS services per tsconfig)
└── verter/vue/           → VueDocument (parsed .vue with sub-documents)
    └── sub/              → VueTypescriptDocument, VueStyleDocument
```

- **DocumentManager**: Tracks open files, handles file changes, caches snapshots
- **VerterManager**: Manages TypeScript LanguageService instances per tsconfig.json
- **VueDocument**: Represents a `.vue` file, lazily parses and creates sub-documents for each block

### Rust Compiler Architecture (`crates/verter_core/src/`)

The Rust compiler uses an AST-based pipeline. The `compile()` orchestrator drives a linear 5-phase pipeline:

```
Vue SFC Source
    ↓
[Tokenizer]  byte-level SFC tokenization (tokenizer/byte.rs)
    ↓
[Parser]     builds arena-based template AST + extracts script/style blocks (parser/)
    ↓
[Style]      v-bind() scan + CSS processing (style/ + css/)
    ↓
[Script]     macro expansion, binding extraction, component wrapper (script/)
    ↓
[Template]   render function codegen — VDOM or Vapor backends (template/)
    ↓
[Compile]    orchestrates the above, applies CodeTransform, emits output (compile.rs)
```

**Module overview:**

```
compile.rs                # Pipeline orchestrator, options, result types
tokenizer/
├── byte.rs               # Zero-copy byte-level SFC tokenizer (production)
├── helpers.rs            # Tokenizer utility functions
└── types.rs              # Event, QuoteType
parser/
├── mod.rs                # Syntax state machine (tokenizer events → AST)
└── types.rs              # RootNodeScript, RootNodeStyle, RootNodeTemplate
ast/
├── mod.rs                # TemplateAst (flat arena with O(1) navigation)
├── builder.rs            # TemplateAstBuilder (incremental AST construction)
└── types.rs              # AstNode, ElementNode, NodeId, pre-computed flags
script/
├── mod.rs                # generate_script() entry point
├── process.rs            # Script setup processing, companion script merging
├── macros.rs             # defineProps/Emits/Model/Slots/Expose/Options
└── css_vars.rs           # _useCssVars() injection for v-bind() in styles
template/
├── oxc/                  # OXC expression parsing for template bindings
│   ├── mod.rs            # parse_template_expressions()
│   └── types.rs          # OxcParsedAst, OxcParsedElement, OxcParsedExpression
└── code_gen/             # Render function codegen
    ├── mod.rs            # generate_template() entry point
    ├── walker.rs         # DFS tree walker (shared by all backends)
    ├── types.rs          # TemplateCodeGen trait, CodeGenOutput
    ├── binding.rs        # BindingResolver (_ctx./$setup. prefix resolution)
    ├── shared/           # Shared codegen helpers
    ├── vdom/             # VDOM render function output (_createElementVNode, etc.)
    ├── vapor/            # Vapor mode output (_template, _renderEffect, etc.)
    └── vapor2/           # Experimental: alternative Vapor codegen approach
style/
├── mod.rs                # generate_style() entry point
└── v_bind.rs             # v-bind() scanning in CSS
css/
├── mod.rs                # process_style() — CSS pipeline entry point
├── prepass.rs            # Vue syntax → valid CSS markers (v-bind, :deep, :slotted)
├── scoped.rs             # Scoped CSS: insert [data-v-xxx] selectors
├── modules.rs            # CSS Modules: hash class names
├── walk.rs               # String-level CSS selector walking
└── types.rs              # ProcessStyleOptions, ProcessStyleResult
code_transform/
├── code_transform.rs     # Chunk-based deferred mutation engine (MagicString equivalent)
├── chunk.rs              # Chunk types (Original, Overwritten, Inserted)
└── source_map.rs         # Source map generation from chunk positions
utils/
├── oxc/                  # OXC parser utilities
│   ├── bindings/         # Expression binding extraction
│   └── vue/              # Vue-specific OXC helpers (macros, type resolution, v-for, v-slot)
└── vue/                  # Vue runtime helpers (tag detection, patch flags)
```

## Build

```bash
pnpm install                  # Install all dependencies
pnpm build                    # Build everything: native → wasm → ts packages
pnpm run build:native         # Build native .node bindings only
pnpm run build:wasm           # Build WASM + copy to playground
pnpm run build:ts             # Build all TypeScript packages
pnpm run build:playground     # Build the playground for deployment
```

`pnpm build` runs sequentially: native bindings first (needed by unplugin), then WASM (needed by playground), then all TS packages. This ensures F5 debugging in VS Code and `pnpm --filter @verter/playground dev` both work.

### Build Dependency Chain

When changing Rust code, you must rebuild downstream artifacts in order:

```
verter_core + verter_analysis + verter_host + verter_ffi (Rust crates)
    ↓ cargo build
verter_napi (NAPI-RS cdylib)        verter_wasm (wasm-bindgen cdylib)
    ↓ pnpm run build:native             ↓ pnpm run build:wasm
@verter/native (.node binary)       @verter/wasm (WASM pkg)
    ↓                                    ↓
@verter/unplugin (bundler plugin)   @verter/playground (browser editor)
    ↓
playground build (Vite)
    ↓
playground E2E tests
```

**Common rebuild sequences:**

| What changed | Rebuild commands (in order) |
|---|---|
| Rust crate (`verter_core`) | `pnpm run build:native` → rebuild any downstream consumer |
| Unplugin (`packages/unplugin`) | `pnpm run build:ts` (or just rebuild unplugin) |
| Playground after Rust/unplugin change | `pnpm run build:native` → `cd packages/playground && rm -rf dist node_modules/.vite && npx vite build` |
| WASM (for playground browser editor) | `pnpm run build:wasm` |
| Everything | `pnpm build` (runs native → wasm → ts in correct order) |

**Key details:**
- `@verter/unplugin` depends on `@verter/native` — compiles `.vue` files at build time via the Rust native binary
- `@verter/playground` uses `@verter/unplugin` (devDep) for its own Vue SFC compilation, and `@verter/wasm` (dep) for the in-browser editor
- The native binary lives in `packages/native/dist/` after `build:native`
- Clear Vite cache (`node_modules/.vite`) when rebuilding playground after native changes

## Development

```bash
pnpm watch                    # Watch-build TS packages for extension dev
pnpm dev-extension            # Watch language-server + vscode extension
pnpm clean                    # Remove build artifacts
```

## Profiling with MCP (for agents)

Use the real-world profiling example with hotpath instrumentation. Two pipeline modes are available:

```bash
# AST-only pipeline (tokenize → parse → OXC expressions):
pnpm run profile:hotpath          # Timing hotspots
pnpm run profile:hotpath:alloc    # Timing + allocation hotspots
pnpm run profile:hotpath:mcp      # Starts MCP endpoint at http://localhost:6771/mcp

# Full compile pipeline (tokenize → parse → style → script → template codegen):
pnpm run profile:hotpath:full          # Timing hotspots
pnpm run profile:hotpath:full:alloc    # Timing + allocation hotspots
pnpm run profile:hotpath:full:mcp      # Starts MCP endpoint at http://localhost:6771/mcp
```

The full pipeline exercises all instrumented functions across the compilation flow:
compile, generate_script, process_script_setup, process_macro_item, generate_style,
process_style, apply_scoped_normalized, parse_template_expressions, generate_template,
walk_template, apply_to, batch_overwrite, batch_prepend_left_static, build_string,
generate_map, generate_map_json, alloc_node, attach_to_parent.

Agent MCP config template is checked in at:

```text
mcp/hotpath.mcp.json
```

Point your MCP-capable agent to that file (or copy its `mcpServers` entry into your local MCP config).
For client-specific setup examples, see [mcp/README.md](mcp/README.md).

## Testing

### Running Tests

```bash
# TypeScript / JavaScript
pnpm test                                    # All JS/TS tests
pnpm vitest --run                            # All tests (non-watch)
pnpm vitest --run path/to/test.spec.ts       # Specific file

# Rust
cargo test --workspace --verbose             # All Rust tests
cargo test --package verter_core test_name   # Specific Rust test
cargo test --package verter_core 2>&1 | tail -60  # Full suite with truncated output
```

### End-of-change Checks

Run these after making changes:

```bash
cargo clippy --fix --allow-dirty --allow-staged --workspace -- -D warnings
cargo fmt --all
```

### Testing Requirements

**IMPORTANT — TDD (Test-Driven Development) is mandatory**:
1. **Write failing tests first** — before implementing any feature or fix, write one or more tests that demonstrate the expected behavior and verify they fail
2. **Implement the minimum code** to make the failing tests pass
3. **Refactor** if needed while keeping tests green

Coverage expectations:
- New features: Add tests covering the new functionality
- Bug fixes: Add tests that would have caught the bug
- Refactoring: Ensure existing tests pass and add tests for edge cases discovered
- Behavioral changes: Add tests verifying the new behavior

Tests serve as documentation of expected behavior and prevent regressions.

### Server Cleanup

**IMPORTANT**: After starting any dev server, preview server, or other long-running process for testing purposes, **always kill it when done**. This prevents stale servers from interfering with subsequent test runs (e.g., Playwright's `reuseExistingServer: true` will use a stale server serving old builds).

```bash
# After finishing with a server, kill it
# If started in background, use the process ID or port:
kill $(lsof -t -i:4173)   # Unix
taskkill //F //PID <pid>   # Windows

# Or if using pnpm/npm scripts, Ctrl+C the process
```

### Test Output Best Practices

When running E2E tests or test suites where you need to inspect output, **redirect output to a temp file first**, then grep/read the file. This avoids re-running expensive builds and tests just to search for different patterns:

```bash
# Good: capture once, search multiple times
pnpm exec playwright test --project=preview 2>&1 | tee /tmp/e2e-output.log
# Then search as needed:
grep -i "fail\|error" /tmp/e2e-output.log

# Bad: re-running the full test suite each time you need different output
pnpm exec playwright test --project=preview 2>&1 | grep "fail"
pnpm exec playwright test --project=preview 2>&1 | grep "error"  # wasteful re-run
```

### TypeScript Test Patterns

**Test locations**: Unit tests are co-located as `*.spec.ts` next to source files. Type tests in `packages/types/` use `vitest --typecheck`.

**AI-generated tests**: Add appropriate comments indicating AI assistance:

```typescript
// For new test files, add a JSDoc at the top:
/**
 * @ai-generated - This test file was generated with AI assistance.
 * Brief description of what the tests cover.
 */

// For individual tests in existing files:
// @ai-generated - Tests X functionality with Y scenarios
it("does something", () => { /* ... */ });
```

**Sourcemap testing** (see `macros.map.spec.ts`):
```typescript
const { s, source, result } = processMacrosForSourcemap(code);
const map = s.generateMap({ source: "test.vue" });
```

**Type testing best practices** (`packages/types/`):
- Always include **both** a positive assertion and a `@ts-expect-error` negative assertion
- This prevents `any`/`unknown`/`never` types from silently passing tests

```typescript
it("type is correctly inferred", () => {
  type Result = SomeTypeHelper<Input>;

  // Positive assertion - type matches expected
  assertType<Result>({} as ExpectedType);
  assertType<ExpectedType>({} as Result);

  // @ts-expect-error - Result is not any/unknown/never
  assertType<{ unrelated: true }>({} as Result);
});
```

### Rust Test Patterns

See [CLAUDE_IMPLEMENTATION_GUIDE.md](CLAUDE_IMPLEMENTATION_GUIDE.md) for detailed Rust testing patterns including:

- **TDD workflow** — write failing tests first, then implement
- **`gen_and_validate()`** — all codegen tests MUST validate JS syntax via oxc parser
- **AST comparison** — E2E tests compare against Vue's official compiler output

## TypeScript Code Patterns

**Defining script plugins:**
```typescript
import { definePlugin, ScriptContext } from "../../types";
export const MyPlugin = definePlugin({
  name: "my-plugin",
  enforce: "pre", // or "post"
  pre(s, ctx) { /* runs before transforms */ },
  transformFunctionCall(item, s, context) { /* transform specific type */ },
  transformDeclaration(item, s, context) { /* another type */ },
  post(s, context) { /* runs after all transforms */ }
});
```

**Type helper prefix convention:**
- Internal helpers use `___VERTER___` prefix (see `packages/core/`)
- String-exported types use `$V_` prefix for collision avoidance

**Parser types** (`packages/core/src/v5/parser/`):
- `ParsedBlockScript`, `ParsedBlockTemplate` - Block-specific parsed data
- `ScriptItem`, `ScriptTypes` - Categorized script AST items

## Key Files

| File | Purpose |
|------|---------|
| `packages/core/src/v5/parser/parser.ts` | Main SFC parser entry |
| `packages/core/src/v5/process/script/script.ts` | Script processing orchestration |
| `packages/core/src/v5/process/script/types.ts` | `definePlugin`, `ScriptContext`, `ScriptPlugin` |
| `packages/core/src/v5/process/script/plugins/macros/macros.ts` | Vue macro transformations |
| `packages/language-server/src/server.ts` | LSP server setup |
| `packages/language-server/src/v5/documents/verter/manager/manager.ts` | TS service management |
| `packages/types/src/helpers/helpers.ts` | Core type utilities |
| `crates/verter_core/src/compile.rs` | Pipeline orchestrator (tokenize → parse → style → script → template) |
| `crates/verter_core/src/parser/mod.rs` | SFC parser: tokenizer events → root nodes + template AST |
| `crates/verter_core/src/ast/types.rs` | AstNode, ElementNode, NodeId, PropFlags |
| `crates/verter_core/src/script/macros.rs` | defineProps/Emits/Model/Slots/Expose/Options |
| `crates/verter_core/src/script/process.rs` | Script setup processing, companion script merging |
| `crates/verter_core/src/template/code_gen/mod.rs` | Template codegen entry point |
| `crates/verter_core/src/template/code_gen/walker.rs` | DFS tree walker (shared by VDOM/Vapor backends) |
| `crates/verter_core/src/template/code_gen/binding.rs` | BindingResolver (_ctx./$setup. prefix resolution) |
| `crates/verter_core/src/template/code_gen/vdom/` | VDOM render function codegen |
| `crates/verter_core/src/template/code_gen/vapor/` | Vapor mode codegen |
| `crates/verter_core/src/css/` | CSS preprocessing and style transformation |
| `crates/verter_core/src/code_transform/code_transform.rs` | Chunk-based deferred mutation engine |
| `crates/verter_analysis/src/lib.rs` | Static analysis entry: imports, exports, bindings |
| `crates/verter_host/src/lib.rs` | Host entry: compile, cache, upsert, dependency tracking |
| `crates/verter_ffi/src/lib.rs` | FFI types shared between NAPI and WASM |
| `packages/unplugin/src/index.ts` | Unplugin factory: `buildStart` (preCompile), `transform`, `load` hooks |
| `packages/unplugin/src/core/types.ts` | `VerterPluginOptions`, `HmrStrategy` |
| `packages/unplugin/src/core/scanner.ts` | `scanVueFiles()` — async recursive directory walker for preCompile |
| `packages/unplugin/src/core/compiler.ts` | Host singleton, `generateComponentId`, `processStyle` |

## Rust Performance

See [.claude/performance-guide.md](.claude/performance-guide.md) for Rust performance patterns including:

- **Batch over incremental** — collect mutations, apply in single O(n+m) passes
- **Allocation hierarchy** — `&'static str` > bump `&'alloc str` > `&str` > reusable buffer > `String`
- **Reusable buffer** — `std::mem::take` pattern to thread a single `String` through processing
- **Object pooling** — recycle structs with `.clear()` to retain Vec capacities
- **Reduce work** — skip expensive operations for trivial cases, cache repeated computations

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
  core     - verter_core Rust crate
  napi     - verter_napi / @verter/native
  wasm     - verter_wasm / @verter/wasm
  play     - playground
  unplugin - @verter/unplugin
  lsp      - language-server
  types    - @verter/types
  ts       - @verter/core (TypeScript)
  ci       - CI/CD workflows
  *        - multiple areas

Examples:
  feat(core): add v-memo directive support
  fix(wasm): correct memory leak in compile()
  chore(ci): add nightly WASM build workflow
  release(all): v0.0.1-alpha.1
```

## CI/CD

See [.claude/ci-cd.md](.claude/ci-cd.md) for detailed CI/CD documentation including:

- Workflow specifications (CI, nightly, release)
- Pre-release versioning flow (alpha → beta → rc → stable)
- Publishing process (npm + crates.io)
- Nightly WASM builds and playground deployment
- Required GitHub secrets configuration
