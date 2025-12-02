# Verter

Verter is a Vue Language Server Protocol (LSP) implementation that provides enhanced TypeScript support for Vue Single File Components (SFCs). Verter converts Vue SFCs to **typed TSX representations** that TypeScript can analyze for type-checking, completions, and error reporting.

> [!WARNING]
> **This project is in active development.** Not everything is working perfectly yet. APIs may change, and you may encounter bugs or incomplete features. Use at your own risk and please report any issues you find.

> [!IMPORTANT]
> The generated TSX is syntactically valid TypeScript/TSX used for **type analysis only** — it's not meant to be executed or compiled as actual JSX/TSX code.

## Features

- **Full TypeScript Support**: Converts `.vue` files to typed TSX representations, enabling complete TypeScript type inference
- **Vue 3 Support**: Optimized for Vue 3 with Composition API and Script Setup
- **Options API Support**: While Script Setup receives more attention, Options API is fully supported
- **Strict Type Safety**: Built with a "strict first" approach to type safety
- **JSX/TSX Interoperability**: SFCs can be seamlessly used in JSX/TSX projects
- **Improved Generic Component Handling**: Full support for generic Vue components with proper constructor typing
- **Automatic Event Handler Type Inference**: Infers parameter types for functions used as template event handlers

### Generic Components

Verter provides improved handling for generic Vue components, respecting Vue constructors with proper type inference. This allows you to get correctly typed instances of generic components:

```vue
<!-- Comp.vue -->
<script setup lang="ts" generic="T extends string">
defineProps<{
  name: T;
}>();

defineSlots<
  Record<T & string, (args: { test: T }) => any> & {
    header: (a: { foo: string }) => any;
  }
>();
</script>
```

```typescript
// Using the generic component with type parameters
import Comp from "./Comp.vue";

// Get a typed instance with specific generic parameter
const foo = {} as InstanceType<typeof Comp<'myName'>>;
foo.$props.name; // Type: 'myName'
```

This enables full type safety when working with generic components, including proper inference for props, slots, and component instances.

### Automatic Event Handler Type Inference

Verter automatically infers types for function parameters used as event handlers in templates. This eliminates the need to manually annotate event types:

```vue
<script setup lang="ts">
// No type annotation needed - Verter infers the type automatically!
function handleClick(e) {
  // e is inferred as MouseEvent from HTMLElementEventMap["click"]
  console.log(e.clientX, e.clientY)
}

function handleInput(event) {
  // event is inferred as Event from HTMLElementEventMap["input"]
  console.log(event.target)
}
</script>

<template>
  <button @click="handleClick">Click me</button>
  <input @input="handleInput" />
</template>
```

This works for:
- **Native HTML elements**: Infers types from `HTMLElementEventMap` (e.g., `click` → `MouseEvent`, `input` → `Event`)
- **Vue components**: Infers types from the component's emits/props definitions
- **Multiple parameters**: All parameters in the function signature are wrapped correctly

## Why Verter?

Since the Vetur days, Vue has struggled with type safety and tooling quality. Vue 3 and Volar brought significant improvements, but challenges remain. Verter aims to provide the **best possible TypeScript experience for Vue** by taking a fundamentally different approach.

### Approach

Verter follows an approach similar to Svelte's language tools by converting SFCs into **typed TSX representations**. This allows TypeScript's language service to handle:

- Type inference
- Code completion
- Error checking
- Refactoring

The generated TSX is syntactically valid TypeScript that represents the Vue component's type structure. It's used internally by the language server for analysis — you never see or run this code directly.

### Verter vs Volar

| Aspect | Verter | Volar |
|--------|--------|-------|
| Maturity | Experimental | Production-ready |
| Approach | SFC → Typed TSX representation | Virtual file mapping |
| Focus | Best TypeScript integration | Feature-rich IDE support |
| Use case | When you need strict type safety | General Vue development |

> [!NOTE]
> If you haven't encountered specific issues with Volar, there's no reason to switch. Verter is for developers who need enhanced TypeScript support.

## Architecture

Verter is organized as a monorepo with the following packages:

```
verter/
├── packages/
│   ├── @verter/core           # SFC → TSX transformation engine
│   ├── @verter/types          # TypeScript utility types
│   ├── @verter/language-server # LSP server implementation
│   ├── @verter/language-shared # Shared client/server protocol
│   ├── @verter/typescript-plugin # TS plugin for .vue imports
│   ├── @verter/oxc-bindings   # OXC parser binary helper
│   └── verter-vscode          # VS Code extension
└── examples/                  # Example Vue projects
```

### Package Dependencies

```
verter-vscode (VS Code extension)
├── @verter/language-server (LSP server)
│   ├── @verter/core (SFC → TSX transformation)
│   │   └── @verter/types (type utilities)
│   └── @verter/language-shared (client/server protocol)
└── @verter/typescript-plugin (IDE .vue import resolution)
    └── @verter/core
```

## Installation

### VS Code Extension

Install the Verter VS Code extension from the marketplace (coming soon).

### Manual Build

```bash
# Clone the repository
git clone https://github.com/pikax/verter.git
cd verter

# Install dependencies
pnpm install

# Build all packages
pnpm build

# Package VS Code extension
pnpm package
```

## Development

### Prerequisites

- Node.js 18+
- pnpm 10+

### Commands

```bash
# Build all packages (respects dependency order)
pnpm build

# Watch mode for development
pnpm watch

# Watch language-server + vscode extension
pnpm dev-extension

# Run tests
pnpm vitest --run

# Clean build artifacts
pnpm clean
```

### Testing

Verter uses **Vitest** for testing. Always use the `--run` flag for non-watch mode:

```bash
pnpm vitest --run                          # All tests
pnpm vitest --run path/to/test.spec.ts     # Specific file
```

Test files are co-located with source files as `*.spec.ts`.

## Documentation

- **[Architecture Overview](./docs/architecture.md)** - Deep dive into Verter's design
- **[Contributing Guide](./CONTRIBUTING.md)** - How to contribute

### Package Documentation

- [`@verter/core`](./packages/core/README.md) - SFC parsing and TSX transformation
- [`@verter/types`](./packages/types/readme.md) - TypeScript type utilities
- [`@verter/language-server`](./packages/language-server/readme.md) - LSP server
- [`@verter/language-shared`](./packages/language-shared/readme.md) - Shared protocol types
- [`@verter/typescript-plugin`](./packages/typescript-plugin/readme.md) - TypeScript plugin
- [`@verter/oxc-bindings`](./packages/oxc-bindings/readme.md) - OXC bindings helper
- [`verter-vscode`](./packages/vue-vscode/readme.md) - VS Code extension

## Credits

- [Svelte language-tools](https://github.com/sveltejs/language-tools) for proving inspiration
- [Vetur](https://github.com/vuejs/vetur) for providing the base for language support
- [Volar](https://github.com/vuejs/language-tools) for inspiration and testing

## License

MIT
