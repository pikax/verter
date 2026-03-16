# @verter/core

::: warning Pre-Release
Verter is pre-release software. APIs may change between releases — see the [API Stability](/api-stability) document.
:::

TypeScript SFC parser and TSX transformer. Converts `.vue` files to valid typed TSX representations for IDE analysis. Used internally by the VS Code extension and TypeScript plugin.

## Installation

```bash
pnpm add @verter/core
```

## API

### `parser(source, filename?, options?)`

Parse a Vue SFC into typed blocks. Returns a `ParserResult` containing the `MagicString` instance, parsed blocks, and metadata.

```ts
import { parser } from '@verter/core'

const result = parser(sfcSource, 'App.vue')
// result.s — MagicString instance for source manipulation
// result.blocks — array of parsed SFC blocks
// result.isTS — whether the script uses TypeScript
// result.isSetup — whether <script setup> is present
// result.isAsync — whether the script is async
// result.generic — generic type params from <script setup generic="T">
// result.filename — the filename passed in
```

**Parameters:**
- `source` (`string`) -- Raw SFC source code
- `filename` (`string`, default: `"temp.vue"`) -- Filename for source map generation
- `options` (`Partial<VerterParserOptions>`) -- Parser options (extends `@vue/compiler-sfc` `SFCParseOptions`)

**Returns:** `ParserResult`

### `processScript(items, plugins, context, autorun?)`

Run the plugin-based script transformation pipeline. Executes plugins in order against parsed script items.

```ts
import { parser, processScript, definePlugin } from '@verter/core'

const parsed = parser(sfcSource, 'App.vue')
const scriptBlock = parsed.blocks.find(b => b.type === 'script' && b.isMain)

// Auto-run mode (default): runs all phases immediately
const { context, s, result } = processScript(
  scriptBlock.result.items,
  plugins,
  {
    filename: 'App.vue',
    s: parsed.s,
    blocks: parsed.blocks,
    block: scriptBlock,
    blockNameResolver: (name) => name,
  },
)
```

**Overloads:**

- `processScript(items, plugins, context)` -- Runs all phases, returns `{ context, s, result }`
- `processScript(items, plugins, context, false)` -- Returns individual phase runners: `{ context, s, pre, main, post }`

The deferred mode (`autorun: false`) lets you run pre/main/post phases separately, useful when you need to inject logic between phases.

### `buildSingle(context)`

High-level builder that processes an SFC through both the script and template plugin pipelines in one call. Automatically selects the main script block and wires up template bindings.

```ts
import { parser, buildSingle } from '@verter/core'

const parsed = parser(sfcSource, 'App.vue')
const { s } = buildSingle({
  ...parsed,
  override: true,
})
const output = s.toString()
```

## Plugin System

Plugins transform parsed SFC script items into TSX output. Each plugin can hook into pre/post phases and transform specific `ScriptTypes`.

### `definePlugin(plugin)`

Type-safe plugin factory function.

```ts
import { definePlugin } from '@verter/core'

const myPlugin = definePlugin({
  name: 'my-plugin',
  enforce: 'pre', // or 'post', or omit for main phase

  pre(s, ctx) {
    // Runs before any transform hooks
  },

  transformImport(item, s, context) {
    // Called for each import declaration
  },

  transformFunctionCall(item, s, context) {
    // Called for each top-level function call (e.g., defineProps)
  },

  transformDeclaration(item, s, context) {
    // Called for each variable/function declaration
  },

  post(s, context) {
    // Runs after all transform hooks
  },
})
```

### Plugin Execution Order

Plugins execute in three phases based on the `enforce` field:

1. **Pre** (`enforce: "pre"`) -- Runs first. Used for macros and early transforms.
2. **Main** (no `enforce`) -- Default phase. Most plugins run here.
3. **Post** (`enforce: "post"`) -- Runs last. Used for final assembly and cleanup.

Within each phase, plugins execute in array order.

### Transform Hooks

Each plugin can define typed transform hooks for specific `ScriptTypes`:

| Hook | Receives | Description |
|------|----------|-------------|
| `transformImport` | `ScriptImport` | Import declarations |
| `transformExport` | `ScriptExport` | Named/all export declarations |
| `transformDefaultExport` | `ScriptDefaultExport` | Default export declarations |
| `transformDeclaration` | `ScriptDeclaration` | Variable and function declarations |
| `transformFunctionCall` | `ScriptFunctionCall` | Top-level function calls |
| `transformBinding` | `ScriptBinding` | Variable bindings |
| `transformAsync` | `ScriptAsync` | Async/await expressions |
| `transformTypeAssertion` | `ScriptTypeAssertion` | TypeScript type assertions |
| `transform` | `ScriptItem` | Catch-all for any item type |

Each hook receives `(item, s, context)` where `s` is the `MagicString` instance and `context` is the `ScriptContext`.

### Built-in Plugins

The following plugins are included and used by `buildSingle()`:

**Pre-phase** (`enforce: "pre"`):
| Plugin | Purpose |
|--------|---------|
| `MacrosPlugin` | Transforms Vue macros: `defineProps`, `defineEmits`, `defineModel`, `defineSlots`, `defineExpose`, `withDefaults` |
| `DefineOptionsPlugin` | Handles `defineOptions()` macro |
| `TemplateRefPlugin` | Handles `useTemplateRef()` tracking |

**Main phase** (no `enforce`):
| Plugin | Purpose |
|--------|---------|
| `ImportsPlugin` | Processes import statements |
| `BindingPlugin` | Tracks variable declarations for binding context |
| `ScriptBlockPlugin` | Wraps `<script setup>` content |
| `DeclarePlugin` | Handles TypeScript `declare` statements |
| `InferFunctionPlugin` | Infers function return types |
| `SFCCleanerPlugin` | Cleans up SFC-specific artifacts |
| `ScriptDefaultPlugin` | Handles default export processing |

**Post-phase** (`enforce: "post"`):
| Plugin | Purpose |
|--------|---------|
| `TemplateBindingPlugin` | Generates template binding type for IDE support |
| `FullContextPlugin` | Generates component context type |
| `AttributesPlugin` | Processes component attributes (`inheritAttrs`, etc.) |
| `ResolversPlugin` | Resolves component references |
| `ComponentInstancePlugin` | Generates component instance type |
| `ComponentTypePlugin` | Generates component constructor type |
| `CurrentInstancePlugin` | Generates `getCurrentInstance()` type |

## Types

### `ParserResult`

```ts
interface ParserResult {
  filename: string
  s: MagicString
  isAsync: boolean
  generic: GenericInfo | null
  blocks: ParsedBlock[]
  isTS: boolean
  isSetup: boolean
}
```

### `ParsedBlock`

A discriminated union of block types:

```ts
type ParsedBlock = ParsedBlockTemplate | ParsedBlockScript | ParsedBlockUnknown

interface ParsedBlockTemplate {
  type: 'template'
  lang: 'vue'
  block: VerterSFCBlock<SFCTemplateBlock>
  result: ParsedTemplateResult
}

interface ParsedBlockScript {
  type: 'script'
  lang: 'js' | 'jsx' | 'ts' | 'tsx'
  isMain: boolean
  isSetup: boolean
  block: VerterSFCBlock<SFCScriptBlock>
  result: ParsedScriptResult
}

interface ParsedBlockUnknown {
  type: string
  lang: string
  block: VerterSFCBlock<SFCBlock>
  result: null
}
```

### `ScriptItem`

Discriminated union of parsed script AST items:

```ts
type ScriptItem =
  | ScriptImport       // import declarations
  | ScriptExport       // named/all exports
  | ScriptDefaultExport // default export
  | ScriptDeclaration  // variable/function declarations
  | ScriptFunctionCall // top-level function calls
  | ScriptBinding      // variable bindings
  | ScriptAsync        // async/await
  | ScriptTypeAssertion // TS type assertions
  | ScriptError        // parse errors
  | ScriptWarning      // parse warnings
```

Each variant has a `type` field matching the `ScriptTypes` enum:

```ts
const enum ScriptTypes {
  Binding = 'Binding',
  Import = 'Import',
  FunctionCall = 'FunctionCall',
  Declaration = 'Declaration',
  Async = 'Async',
  Export = 'Export',
  DefaultExport = 'DefaultExport',
  TypeAssertion = 'TypeAssertion',
  Error = 'Error',
  Warning = 'Warning',
}
```

### `ScriptContext`

The context object passed to plugin transform hooks:

```ts
interface ScriptContext extends ProcessContext {
  prefix(name: string): string
  isSetup: boolean
  templateBindings: TemplateBinding[]
  handledAttributes?: Set<string>
  isSingleFile?: boolean
}
```

### `ProcessContext`

Base context for all processing pipelines:

```ts
interface ProcessContext {
  filename: string
  s: MagicString
  override?: boolean
  blockNameResolver: (name: string) => string
  isAsync: boolean
  generic: GenericInfo | null
  isTS: boolean
  block: ParsedBlock
  blocks: ParsedBlock[]
  items: ProcessItem[]
}
```

### `ProcessPlugin`

Generic plugin interface:

```ts
interface ProcessPlugin<T, C extends ProcessContext> {
  name?: string
  enforce?: 'pre' | 'post'
  transform?: (item: T, s: MagicString, context: C) => void
  pre?: (s: MagicString, context: C) => void
  post?: (s: MagicString, context: C) => void
}
```
