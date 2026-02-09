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
└── @verter/vite-plugin
    └── @verter/native

@verter/playground (Firebase-hosted)
└── @verter/wasm (Rust template compiler, wasm-bindgen)
```

### Repository Structure

```
crates/
  verter_core/       # Core template compiler (Rust)
  verter_napi/       # Native Node.js bindings (NAPI-RS cdylib)
  verter_wasm/       # WASM bindings (wasm-bindgen cdylib)
packages/
  core/              # @verter/core - SFC parser & TSX transformer
  types/             # @verter/types - TypeScript utility types
  native/            # @verter/native - Native binding loader + platform packages
  wasm/              # @verter/wasm - WASM binding wrapper
  vite-plugin/       # @verter/vite-plugin - Vite integration
  language-server/   # @verter/language-server - LSP server
  language-shared/   # @verter/language-shared - Shared LSP protocol types
  typescript-plugin/ # @verter/typescript-plugin - TS language service plugin
  oxc-bindings/      # @verter/oxc-bindings - OXC parser binary helper
  playground/        # @verter/playground - Online playground (private, Firebase-hosted)
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
| **`@verter/oxc-bindings`** | Helper for downloading platform-specific OXC parser binaries | `src/index.ts` |

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

## Build

```bash
pnpm install                  # Install all dependencies
pnpm build                    # Build everything: native → wasm → ts packages
pnpm run build:native         # Build native .node bindings only
pnpm run build:wasm           # Build WASM + copy to playground
pnpm run build:ts             # Build all TypeScript packages
pnpm run build:playground     # Build the playground for deployment
```

`pnpm build` runs sequentially: native bindings first (needed by vite-plugin), then WASM (needed by playground), then all TS packages. This ensures F5 debugging in VS Code and `pnpm --filter @verter/playground dev` both work.

## Development

```bash
pnpm watch                    # Watch-build TS packages for extension dev
pnpm dev-extension            # Watch language-server + vscode extension
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
cargo test --workspace --verbose             # All Rust tests
cargo test --package verter_core test_name   # Specific Rust test
cargo test --package verter_core 2>&1 | tail -60  # Full suite with truncated output
```

### Testing Requirements

**IMPORTANT**: When making any code changes, always add corresponding tests whenever possible:
- New features: Add tests covering the new functionality
- Bug fixes: Add tests that would have caught the bug
- Refactoring: Ensure existing tests pass and add tests for edge cases discovered
- Behavioral changes: Add tests verifying the new behavior

Tests serve as documentation of expected behavior and prevent regressions.

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
- **State management** — `TemplateCodegenState` in `types.rs`
- **Element processing** — "store in open, emit in close" pattern

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
| `crates/verter_core/src/codegen/vue/template/types.rs` | Codegen state structs, patch flags |
| `crates/verter_core/src/codegen/vue/template/element.rs` | Element open/close processing |
| `crates/verter_core/src/builder/codegen.rs` | Pipeline setup, E2E tests |

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
  vite     - vite-plugin
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
