# Verter Architecture

This document provides a high-level overview of Verter's architecture for developers who want to understand or contribute to the project.

## Core Concepts

### SFC to TSX Transformation

The fundamental idea behind Verter is converting Vue Single File Components into **typed TSX representations**. This generated code is syntactically valid TypeScript/TSX that TypeScript can analyze for type information.

> **Key distinction**: The generated TSX is for **type-checking purposes only** — it represents the component's type structure but is not meant to be executed or compiled as actual runnable JSX/TSX code.

```
┌─────────────────────────────────────────────────────────────┐
│                    Vue SFC (.vue)                            │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐          │
│  │  <script    │  │  <template> │  │   <style>   │          │
│  │   setup>    │  │             │  │             │          │
│  └─────────────┘  └─────────────┘  └─────────────┘          │
└─────────────────────────────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────┐
│                    @verter/core                              │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐          │
│  │   Parser    │→ │   Plugins   │→ │   Output    │          │
│  └─────────────┘  └─────────────┘  └─────────────┘          │
└─────────────────────────────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────┐
│              Typed TSX Representation (.tsx)                 │
│  - Type-safe props and emits definitions                     │
│  - Template represented as typed JSX (for analysis)          │
│  - Sourcemaps preserved for error mapping                    │
│  (Used for type-checking only, not for execution)            │
└─────────────────────────────────────────────────────────────┘
```

### Plugin-Based Transformation

The transformation system uses a plugin architecture where each plugin handles specific patterns:

```typescript
// Plugin interface
interface ScriptPlugin {
  name: string;
  enforce?: "pre" | "post";
  
  pre?(s: MagicString, context: ScriptContext): void;
  transform?(item: ScriptItem, s: MagicString, context: ScriptContext): void;
  transformFunctionCall?(item: FunctionCallItem, s: MagicString, context: ScriptContext): void;
  // ... other transform methods
  post?(s: MagicString, context: ScriptContext): void;
}
```

Plugins are executed in order:
1. **Pre-plugins** (`enforce: "pre"`) - Initialize state, add imports
2. **Normal plugins** - Process specific patterns
3. **Post-plugins** (`enforce: "post"`) - Finalize output, generate types

### Sourcemap Preservation

Verter uses `MagicString` to preserve sourcemaps throughout transformations. This enables:
- Accurate error locations in the original `.vue` file
- Go-to-definition pointing to source code
- Proper debugging with breakpoints

```typescript
const s = new MagicString(source);

// These operations preserve sourcemaps
s.overwrite(start, end, newContent);
s.prepend(header);
s.remove(start, end);

// Generate sourcemap
const map = s.generateMap({ source: "Component.vue" });
```

## Package Architecture

### Data Flow

```
User's .vue file
       │
       ▼
┌──────────────────┐
│ VS Code Extension │ ← User interactions (completions, hover, etc.)
└────────┬─────────┘
         │ LSP
         ▼
┌──────────────────┐
│ Language Server   │ ← Manages documents and TS services
└────────┬─────────┘
         │
         ▼
┌──────────────────┐
│   @verter/core   │ ← Transforms SFC to TSX
└────────┬─────────┘
         │
         ▼
┌──────────────────┐
│    TypeScript    │ ← Type checking and analysis
│ Language Service │
└──────────────────┘
```

### Package Responsibilities

| Package | Role |
|---------|------|
| `@verter/core` | Parse SFC, transform to TSX, preserve sourcemaps |
| `@verter/types` | Type utilities for Vue component inference |
| `@verter/language-server` | LSP implementation, document management |
| `@verter/language-shared` | Protocol types shared between client/server |
| `@verter/typescript-plugin` | Handle `.vue` imports in TS/JS files |
| `@verter/oxc-bindings` | Platform-specific OXC parser binaries |
| `verter-vscode` | VS Code extension, client-side integration |

## Core Package Deep Dive

### Parser (`packages/core/src/v5/parser/`)

The parser breaks down an SFC into typed blocks:

```
parser.ts          → Main entry, uses @vue/compiler-sfc
types.ts           → ParsedBlockScript, ParsedBlockTemplate, etc.
script/            → Extract script AST items (ScriptItem, ScriptTypes)
template/          → Parse template expressions and bindings
ast/               → Custom AST node types
walk/              → AST traversal utilities
```

### Process (`packages/core/src/v5/process/`)

The processing system transforms parsed blocks:

```
script/
├── script.ts      → Plugin orchestration
├── types.ts       → ScriptPlugin, ScriptContext, definePlugin
└── plugins/       → Individual transformation plugins
    ├── macros/    → defineProps, defineEmits, defineModel, etc.
    ├── binding/   → Variable binding tracking
    ├── imports/   → Import handling
    └── ...        → Other transformations
```

### Plugin Examples

**Macros Plugin** - Transforms Vue macros like `defineProps`:

```typescript
// Input (TypeScript)
const props = defineProps<{ msg: string }>()

// Output (simplified)
type ___VERTER___defineProps_Type = ___VERTER___Prettify<{ msg: string }>
const props = defineProps<___VERTER___defineProps_Type>()
```

```typescript
// Input (JavaScript with runtime declaration)
const props = defineProps({ a: String })

// Output (simplified)
const ___VERTER___defineProps_Boxed = ___VERTER___defineProps_Box({ a: String })
const props = defineProps(___VERTER___defineProps_Boxed)
```

The `___VERTER___` prefixed helpers are type utilities that allow TypeScript to properly infer and validate the component's props, emits, slots, etc.

**Template Binding Plugin** - Generates binding types for template:

```typescript
// Generates type information for template access
type __VERTER_TemplateBindings__ = {
  props: typeof props;
  count: typeof count;
  // ... all accessible bindings
};
```

## Language Server Architecture

### Document Management

```
DocumentManager
├── Tracks open/changed/closed files
├── Maintains document snapshots
└── Notifies VerterManager of changes

VerterManager
├── One TS LanguageService per tsconfig.json
├── Manages virtual file mapping
└── Handles CSS language services
```

### VueDocument Structure

A `.vue` file becomes multiple virtual documents:

```
MyComponent.vue
├── MyComponent.vue.ts        (bundled module)
├── MyComponent.vue.script.ts (script block)
├── MyComponent.vue.render.tsx (template as JSX)
└── MyComponent.vue.style.css (styles)
```

### LSP Feature Flow

```
Completion Request
       │
       ▼
┌──────────────────┐
│ Find VueDocument │
└────────┬─────────┘
         │
         ▼
┌──────────────────┐
│ Map position to  │ ← Source position → Generated position
│ virtual document │
└────────┬─────────┘
         │
         ▼
┌──────────────────┐
│   TS Language    │
│    Service       │
└────────┬─────────┘
         │
         ▼
┌──────────────────┐
│ Map completions  │ ← Generated position → Source position
│ back to source   │
└──────────────────┘
         │
         ▼
Completion Response
```

## Type System

### @verter/types Overview

The types package provides utilities for Vue component type inference:

```typescript
// Hidden property pattern - attach metadata without affecting the shape
type WithHidden = PatchHidden<{ id: number }, { meta: true }>;
type Meta = ExtractHidden<WithHidden>; // { meta: true }

// Emit function to props conversion
type Emits = (e: 'update', value: number) => void;
type Props = EmitsToProps<Emits>; // { onUpdate: (value: number) => void }

// Partial for undefined properties
type Opts = { a: string; b: number | undefined };
type Result = PartialUndefined<Opts>; // { a: string; b?: number }
```

### String Export

For language server injection, types are exported as a string with `$V_` prefix:

```typescript
import typeHelpers from "@verter/types/string";

// typeHelpers contains:
// export type $V_PatchHidden<T, E> = ...
// export type $V_ExtractHidden<T> = ...
```

This allows injecting type helpers into virtual files without naming collisions.

## Development Tips

### Understanding Transformations

1. Write a sample `.vue` file
2. Add a test in the relevant plugin
3. Check the generated TSX output
4. Verify sourcemaps with sourcemap tests

### Adding a New Plugin

1. Create plugin file in `packages/core/src/v5/process/script/plugins/`
2. Implement `ScriptPlugin` interface using `definePlugin`
3. Register in `packages/core/src/v5/process/script/plugins/index.ts`
4. Add tests

### Testing Strategy

- **Unit tests**: Test individual plugins with minimal SFC snippets
- **Integration tests**: Test full transformation pipeline
- **Type tests**: Verify TypeScript inference (using `vitest --typecheck`)
- **Sourcemap tests**: Verify position mappings

## Resources

- [Vue Compiler SFC](https://github.com/vuejs/core/tree/main/packages/compiler-sfc)
- [Svelte Language Tools](https://github.com/sveltejs/language-tools) - Inspiration
- [LSP Specification](https://microsoft.github.io/language-server-protocol/)
- [TypeScript Language Service](https://github.com/microsoft/TypeScript/wiki/Using-the-Language-Service-API)
