# Contributing to Verter

Thank you for your interest in contributing to Verter! This guide will help you get started.

## Getting Started

### Prerequisites

- Node.js 18+
- pnpm 10+
- VS Code (for extension development)

### Setup

```bash
# Clone the repository
git clone https://github.com/pikax/verter.git
cd verter

# Install dependencies
pnpm install

# Build all packages
pnpm build
```

## Development Workflow

### Building

```bash
# Build all packages (respects dependency order)
pnpm build

# Build specific package
pnpm --filter @verter/core build

# Watch mode
pnpm watch

# Watch language-server + vscode extension
pnpm dev-extension
```

### Testing

Verter uses **Vitest** for testing.

```bash
# Run all tests
pnpm vitest --run

# Run specific test file
pnpm vitest --run macros.spec.ts

# Run tests for a package
cd packages/core && pnpm test

# Run with coverage
pnpm vitest --run --coverage
```

### Test Patterns

Tests are co-located with source files as `*.spec.ts`:

```
src/
├── parser/
│   ├── parser.ts
│   └── parser.spec.ts    # Tests for parser.ts
└── process/
    └── script/
        └── plugins/
            └── macros/
                ├── macros.ts
                └── macros.spec.ts
```

## Project Architecture

### Package Structure

```
packages/
├── @verter/core           # SFC → TSX transformation
├── @verter/types          # TypeScript utilities
├── @verter/language-server # LSP server
├── @verter/language-shared # Shared protocol
├── @verter/typescript-plugin # TS plugin
├── @verter/oxc-bindings   # OXC parser helper
└── verter-vscode          # VS Code extension
```

### Dependency Graph

```
verter-vscode
├── @verter/language-server
│   ├── @verter/core
│   │   └── @verter/types
│   └── @verter/language-shared
└── @verter/typescript-plugin
    └── @verter/core
```

### Core Transformation Pipeline

```
Vue SFC → parser/ → process/script/plugins/ → TSX output
```

1. **Parser** - Parses SFC into typed blocks
2. **Plugins** - Transform specific patterns
3. **Output** - Valid TSX with sourcemaps

## Code Patterns

### Defining Script Plugins

```typescript
import { definePlugin, ScriptContext } from "../../types";

export const MyPlugin = definePlugin({
  name: "my-plugin",
  enforce: "pre", // or "post"
  
  pre(s, ctx) {
    // Runs before transforms
  },
  
  transformFunctionCall(item, s, context) {
    // Transform function calls
  },
  
  post(s, context) {
    // Runs after transforms
  }
});
```

### Type Helper Prefixes

All the types that Verter uses in the templates are prefixed with `___VERTER___` this is arbitrary but allows the removal of those helpers when providing information to the user

- `___VERTER___` - Internal helpers in core

### MagicString Usage

Always use `MagicString` for source manipulation to preserve sourcemaps:

```typescript
import { MagicString } from "@vue/compiler-sfc";

function transform(code: string) {
  const s = new MagicString(code);
  
  // Use methods that preserve sourcemaps
  s.overwrite(start, end, newContent);
  s.prepend(header);
  s.append(footer);
  
  return {
    code: s.toString(),
    map: s.generateMap({ source: "file.vue" })
  };
}
```

## Pull Request Process

1. **Fork** the repository
2. **Create** a feature branch: `git checkout -b feature/my-feature`
3. **Make** your changes
4. **Test** your changes: `pnpm vitest --run`
5. **Commit** with clear messages
6. **Push** to your fork
7. **Open** a Pull Request

### Commit Messages

Use clear, descriptive commit messages:

```
feat(core): add support for defineModel macro
fix(language-server): resolve completion for template refs
docs(readme): update installation instructions
test(macros): add tests for withDefaults
```

### PR Checklist

- [ ] Tests added/updated
- [ ] Documentation updated
- [ ] No TypeScript errors
- [ ] Follows existing code style

## Debugging

### VS Code Extension

1. Open monorepo in VS Code
2. Run "Launch Extension" debug configuration
3. New window opens with extension loaded
4. Set breakpoints in language server code

### Language Server

```bash
# Start server with inspector
node --inspect=6009 packages/language-server/dist/server.js
```

### TypeScript Plugin

Plugin logs go to TypeScript's server log. In VS Code:
1. Open command palette
2. "TypeScript: Open TS Server Log"

## Questions?

- Open an issue for bugs or feature requests
- Check existing issues and PRs first
- Include reproduction steps for bugs

## License

By contributing, you agree that your contributions will be licensed under the MIT License.
