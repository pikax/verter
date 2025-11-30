# @verter/core

The core SFC-to-TSX transformation engine for Verter. This package parses Vue Single File Components and transforms them into typed TSX representations for TypeScript analysis.

## Overview

`@verter/core` is the heart of Verter's transformation pipeline. It:

1. Parses Vue SFCs using `@vue/compiler-sfc`
2. Analyzes script blocks, extracting AST items and Vue macros
3. Transforms the parsed content into typed TSX representations using a plugin-based system
4. Preserves sourcemaps for accurate error reporting and debugging

> **Note**: The generated TSX is syntactically valid TypeScript used for **type-checking and IDE features** — it's not meant to be compiled or executed as actual JSX/TSX code.

## Installation

```bash
pnpm add @verter/core
```

## Architecture

### High-Level Flow

```
Vue SFC (.vue)
    ↓
┌─────────────────┐
│     Parser      │  → Parses SFC into typed blocks
└─────────────────┘
    ↓
┌─────────────────┐
│   Processors    │  → Plugin-based transformation
└─────────────────┘
    ↓
Typed TSX representation (for type analysis)
```

### Directory Structure

```
src/
├── index.ts              # Main entry point
└── v5/                   # Current implementation version
    ├── config.ts         # Configuration options
    ├── index.ts          # v5 entry point
    ├── parser/           # SFC parsing
    │   ├── parser.ts     # Main parser using @vue/compiler-sfc
    │   ├── types.ts      # ParsedBlock types
    │   ├── script/       # Script block AST extraction
    │   ├── template/     # Template expression parsing
    │   ├── ast/          # AST node types
    │   └── walk/         # AST traversal utilities
    ├── process/          # Transformation system
    │   ├── script/       # Script processing
    │   │   ├── script.ts # Plugin orchestration
    │   │   ├── types.ts  # ScriptPlugin, ScriptContext
    │   │   └── plugins/  # Transformation plugins
    │   ├── template/     # Template processing
    │   ├── styles/       # Style processing
    │   ├── types.ts      # ProcessContext, ProcessPlugin
    │   └── runner.ts     # Process runner
    └── utils/            # Shared utilities
```

## Parser

The parser converts Vue SFCs into structured data that plugins can transform.

### Parsed Block Types

```typescript
// Template block
interface ParsedBlockTemplate {
  type: "template";
  lang: "vue";
  block: SFCTemplateBlock;
  result: ParsedTemplateResult;
}

// Script block
interface ParsedBlockScript {
  type: "script";
  lang: "js" | "jsx" | "ts" | "tsx";
  isMain: boolean;
  isSetup: boolean;
  block: SFCScriptBlock;
  result: ParsedScriptResult;
}

// Custom/unknown block
interface ParsedBlockUnknown {
  type: string;
  lang: string;
  block: SFCBlock;
  result: null;
}
```

### Script Items

The script parser extracts categorized AST items (`ScriptItem`) of various types:

| Type | Description |
|------|-------------|
| `Import` | Import statements |
| `Export` | Named exports |
| `DefaultExport` | Default exports |
| `Declaration` | Variable/function declarations |
| `FunctionCall` | Function calls (including Vue macros) |
| `Binding` | Variable bindings for template context |
| `Async` | Async setup markers |
| `TypeAssertion` | TypeScript type assertions |

## Plugin System

The transformation is handled by a plugin-based architecture that allows modular, composable transformations.

### Defining a Plugin

```typescript
import { definePlugin, ScriptContext } from "@verter/core";

export const MyPlugin = definePlugin({
  name: "my-plugin",
  
  // Control execution order
  enforce: "pre", // or "post", or omit for normal
  
  // Run before all transforms
  pre(s, context) {
    // Initialize state, add imports, etc.
  },
  
  // Transform specific script item types
  transformFunctionCall(item, s, context) {
    if (item.callee.name === "myMacro") {
      // Transform the macro call
      s.overwrite(item.start, item.end, "transformedCode");
    }
  },
  
  transformDeclaration(item, s, context) {
    // Handle variable declarations
  },
  
  // Run after all transforms
  post(s, context) {
    // Finalize, generate types, etc.
  }
});
```

### Plugin Execution Order

1. **Pre-plugins** (`enforce: "pre"`) - Run first
2. **Normal plugins** (no `enforce`) - Run in registration order
3. **Post-plugins** (`enforce: "post"`) - Run last

Within each phase:
- `pre()` hooks execute first
- Item transforms execute by type
- `post()` hooks execute last

### Available Plugins

| Plugin | Purpose |
|--------|---------|
| `macros` | Transforms Vue macros (`defineProps`, `defineEmits`, `defineModel`, `defineSlots`, `defineExpose`, `withDefaults`) |
| `template-binding` | Generates template binding types for IDE support |
| `binding` | Tracks variable declarations for binding context |
| `imports` | Handles import statement processing |
| `script-block` | Wraps script setup content |
| `full-context` | Generates component context type |
| `attributes` | Processes component attributes |
| `resolvers` | Resolves component references |
| `define-options` | Handles `defineOptions` macro |
| `component-instance` | Generates component instance type |
| `template-ref` | Handles template refs |

## Process Context

Plugins receive a context object with transformation state:

```typescript
interface ScriptContext extends ProcessContext {
  // File information
  filename: string;
  
  // MagicString for source manipulation
  s: MagicString;
  
  // Script characteristics
  isTS: boolean;
  isSetup: boolean;
  isAsync: boolean;
  
  // Generic component info
  generic: GenericInfo | null;
  
  // Parsed blocks
  block: ParsedBlock;
  blocks: ParsedBlock[];
  
  // Generated items (imports, bindings, etc.)
  items: ProcessItem[];
  
  // Template bindings for IDE support
  templateBindings: TemplateBinding[];
  
  // Prefix for generated identifiers
  prefix(name: string): string;
}
```

## Usage

### Basic Transformation

```typescript
import { parse, process } from "@verter/core";

const source = `
<script setup lang="ts">
import { ref } from 'vue';

const count = ref(0);
const props = defineProps<{ msg: string }>();
</script>

<template>
  <div>{{ props.msg }}: {{ count }}</div>
</template>
`;

// Parse the SFC
const parsed = parse(source, { filename: "Component.vue" });

// Process to typed TSX representation
const result = process(parsed);

console.log(result.code); // Typed TSX for analysis (not runnable code)
```

### Sourcemap Support

The transformation preserves sourcemaps using `MagicString`:

```typescript
const result = process(parsed);

// Generate sourcemap
const map = result.s.generateMap({
  source: "Component.vue",
  file: "Component.vue.tsx",
  includeContent: true
});
```

## Type Prefixes

Internal generated identifiers use specific prefixes to avoid collisions:

- `___VERTER___` - Internal helper prefix (used in core)


## Testing

```bash
# Run all tests
pnpm vitest --run

# Run specific test
pnpm vitest --run macros.spec.ts

# Run with coverage
pnpm vitest --run --coverage
```

### Sourcemap Testing

```typescript
import { processMacrosForSourcemap } from "./test-utils";

it("should preserve sourcemaps", () => {
  const code = `const props = defineProps<{ msg: string }>();`;
  const { s, source, result } = processMacrosForSourcemap(code);
  const map = s.generateMap({ source: "test.vue" });
  
  // Verify mappings
  expect(map.mappings).toBeDefined();
});
```

## Dependencies

- `@vue/compiler-sfc` - Vue SFC parsing
- `@vue/compiler-core` - Vue template compilation
- `@babel/parser` - JavaScript/TypeScript AST parsing
- `oxc-parser` - Fast JavaScript parser (alternative)
- `magic-string` - Sourcemap-preserving string manipulation
- `@verter/types` - Type utilities

## License

MIT
