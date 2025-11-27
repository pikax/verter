# Copilot Instructions

## Project Overview

Verter is a Vue Language Server Protocol (LSP) implementation that converts Vue Single File Components (SFCs) to valid TSX, leveraging TypeScript for type checking. Unlike Volar, Verter generates actual valid TSX code rather than virtual files.

## Architecture

### Package Dependency Graph

```
verter-vscode (VS Code extension)
├── @verter/language-server (LSP server)
│   ├── @verter/core (SFC → TSX transformation)
│   │   └── @verter/types (type utilities)
│   └── @verter/language-shared (client/server protocol)
└── @verter/typescript-plugin (IDE .vue import resolution)
    └── @verter/core
```

### Packages (`packages/`)

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

## Testing

Uses **Vitest**. Always use `--run` flag (non-watch mode):

```bash
pnpm vitest --run                          # All tests
pnpm vitest --run path/to/test.spec.ts     # Specific file
```

**Test locations:**
- Unit tests are co-located: `*.spec.ts` next to source files
- Type tests in `packages/types/` use `vitest --typecheck`

**Sourcemap testing pattern** (see `macros.map.spec.ts`):
```typescript
// Verify transformed code maps back to original positions
const { s, source, result } = processMacrosForSourcemap(code);
const map = s.generateMap({ source: "test.vue" });
```

## Development Workflow

```bash
pnpm build          # Build all packages (respects dependency order)
pnpm watch          # Watch mode for development
pnpm dev-extension  # Watch language-server + vscode extension
pnpm clean          # Remove build artifacts
```

## Code Patterns

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
- `packages/core/src/v5/process/script/plugins/macros/macros.ts` - Vue macro transformations
- `packages/language-server/src/server.ts` - LSP server setup
- `packages/types/src/helpers/helpers.ts` - Core type utilities
