# @verter/component-meta

Extract Vue component metadata (props, events, slots, models, expose, imports, bindings, styles, flags) from Single File Components. Includes a generic **Type IR**, adapters for Storybook/Histoire/Zod/JSON Schema, and a **Volar-compatible compat layer** for drop-in replacement of `vue-component-meta`.

## Install

```bash
npm install @verter/component-meta
# or
pnpm add @verter/component-meta
```

`@verter/native` is required (installed automatically). For browser/WASM usage, install `@verter/wasm` instead.

## Quick Start

```ts
import { createAdapter, extractComponentMeta } from "@verter/component-meta";

const adapter = createAdapter();

adapter.upsert({
  inputId: "MyButton.vue",
  source: `
<script setup lang="ts">
/**
 * Button label text.
 * @example "Click me"
 */
defineProps<{
  label: string
  variant?: 'primary' | 'secondary'
}>()

defineEmits<{
  /** Fired on button click */
  (e: 'click', payload: MouseEvent): void
}>()
</script>

<template>
  <button @click="$emit('click', $event)">
    <slot>{{ label }}</slot>
  </button>
</template>
`,
});

const meta = extractComponentMeta(adapter, "MyButton.vue");
console.log(meta.props);
// [
//   { name: "label", type: { kind: "primitive", name: "string" },
//     required: true, description: "Button label text.",
//     tags: [{ name: "example", text: "\"Click me\"" }] },
//   { name: "variant", type: { kind: "union", types: [...] },
//     required: false },
// ]
```

## JSDoc Extraction

Props, events, and slots automatically extract JSDoc comments:

```vue
<script setup lang="ts">
defineProps<{
  /**
   * The button label.
   * @default "Submit"
   * @deprecated Use `text` instead
   */
  label: string
}>()
</script>
```

Produces:

```ts
meta.props[0].description // "The button label."
meta.props[0].tags
// [
//   { name: "default", text: "\"Submit\"" },
//   { name: "deprecated", text: "Use `text` instead" },
// ]
```

Works with both type-based (`defineProps<{}>()`) and runtime (`defineProps({})`) declarations.

## Core API

| Function | Description |
|----------|-------------|
| `extractComponentMeta(adapter, fileId, filePath?)` | Extract metadata from a compiled SFC |
| `snapshotToMeta(snapshot, filePath)` | Convert a raw analysis snapshot to `ComponentMeta` |
| `parseType(input)` | Parse a TS type annotation string into a `TypeDescriptor` |
| `runtimeTypeToDescriptor(name)` | Convert a Vue runtime constructor (`"String"`) to a `TypeDescriptor` |

## Types

| Type | Description |
|------|-------------|
| `ComponentMeta` | Full component metadata |
| `PropMeta` | Prop declaration with type, JSDoc, default |
| `EventMeta` | Event declaration with payload type, JSDoc |
| `SlotMeta` | Slot with scoped bindings, JSDoc |
| `ModelMeta` | `defineModel` declaration |
| `ExposedMeta` | `defineExpose` member |
| `JsdocTag` | JSDoc tag (`{ name, text? }`) |
| `ComponentUsage` | Child component used in template |
| `TemplateRefMeta` | Template `ref` attribute |
| `ImportMeta` | Import statement |
| `BindingMeta` | Script binding with reactivity classification |
| `VueApiCallMeta` | Vue API call site |
| `StyleMeta` | Style block analysis |
| `ComponentFlags` | Boolean component characteristics |

## Type IR

All extracted types use a generic **TypeDescriptor** — a JSON-serializable discriminated union:

```ts
import { primitive, literal, union, parseType } from "@verter/component-meta";

const type = parseType("string | number");
// { kind: "union", types: [{ kind: "primitive", name: "string" }, ...] }

const buttonSize = union([literal("sm"), literal("md"), literal("lg")]);
```

See the [package README](https://github.com/pikax/verter/tree/main/packages/component-meta#type-ir) for the full type kind table.

## Adapters

| Export path | Function | Description |
|-------------|----------|-------------|
| `./storybook` | `toArgTypes(meta)` | Storybook argTypes with controls |
| `./histoire` | `toHistoireConfig(meta)` | Histoire story config |
| `./zod` | `propsToZodSchema(meta)` | Runtime Zod schema |
| `./json-schema` | `propsToJsonSchema(meta)` | JSON Schema (draft-07) |

## Volar Compatibility (`./compat`) {#compat}

The `@verter/component-meta/compat` export provides a **drop-in replacement** for Volar's `vue-component-meta`. Consumers like `nuxt-component-meta`, Nuxt UI docs, and Nuxt Content can swap to Verter with zero code changes.

### Migration

Replace your import:

```diff
- import { createChecker } from 'vue-component-meta'
+ import { createChecker } from '@verter/component-meta/compat'
```

That's it. The API surface is identical.

### Full Example

Given a component `src/MyButton.vue`:

```vue
<script setup lang="ts">
/**
 * A reusable button component.
 */
defineProps<{
  /** Display label */
  label: string
  /** Visual variant */
  variant?: 'primary' | 'secondary' | 'danger'
  /** @deprecated Use `variant` instead */
  color?: string
}>()

defineEmits<{
  /** Fired on click */
  (e: 'click', payload: MouseEvent): void
}>()

defineSlots<{
  /** Custom button content */
  default(props: { active: boolean }): any
  /** Icon slot on the left */
  icon(): any
}>()
</script>

<template>
  <button @click="$emit('click', $event)">
    <slot name="icon" />
    <slot :active="true">{{ label }}</slot>
  </button>
</template>
```

```ts
import { createChecker } from "@verter/component-meta/compat";

const checker = createChecker("./tsconfig.json");
const meta = checker.getComponentMeta("./src/MyButton.vue");

// ── Props ──
meta.props[0].name;        // "label"
meta.props[0].type;        // "string"
meta.props[0].required;    // true
meta.props[0].description; // "Display label"
meta.props[0].tags;        // []
meta.props[0].schema;      // "string"

meta.props[1].name;        // "variant"
meta.props[1].type;        // "\"primary\" | \"secondary\" | \"danger\""
meta.props[1].required;    // false
meta.props[1].schema;      // { kind: "enum", type: "...", schema: ["\"primary\"", ...] }

meta.props[2].tags;        // [{ name: "deprecated", text: "Use `variant` instead" }]

// ── Events ──
meta.events[0].name;       // "click"
meta.events[0].type;       // "(payload: MouseEvent) => void"
meta.events[0].description // "Fired on click"

// ── Slots ──
meta.slots[0].name;        // "default"
meta.slots[0].type;        // "{ active: boolean }"
meta.slots[0].description; // "Custom button content"

meta.slots[1].name;        // "icon"
meta.slots[1].type;        // "{}"

// ── Exposed ──
meta.exposed;              // [] (nothing exposed)
```

### Factory Functions

```ts
import { createChecker, createCheckerByJson } from "@verter/component-meta/compat";

// From a tsconfig.json path — auto-discovers and loads all .vue files
const checker = createChecker("./tsconfig.json", {
  schema: true, // enable PropertyMetaSchema generation (default: true)
});

// From a JSON config object
const checker2 = createCheckerByJson("/project/root", {
  include: ["src/**/*.vue"],
  compilerOptions: { strict: true },
});
```

### Checker API

```ts
// Get component metadata in Volar-compatible shape
const meta = checker.getComponentMeta("./src/MyButton.vue");

// Get export names (always ["default"] for SFCs)
checker.getExportNames("./src/MyButton.vue"); // ["default"]

// Update a file (hot reload)
checker.updateFile("./src/MyButton.vue", newSource);

// Delete a file
checker.deleteFile("./src/MyButton.vue");

// Re-read all tracked files from disk
checker.reload();

// Clear internal caches (alias for reload)
checker.clearCache();

// Not supported — throws
checker.getProgram(); // Error: Verter does not use a TypeScript Program
```

### PropertyMeta Shape

The compat layer maps Verter's rich types to Volar's `PropertyMeta` shape. Props, events, slots, and exposed members all use this same interface:

```ts
interface PropertyMeta {
  name: string;               // member name
  description: string;        // JSDoc description (empty string if none)
  type: string;               // human-readable type (e.g. "string | number")
  default?: string;           // default value string
  required: boolean;          // whether the member is required
  global?: boolean;           // always false (Verter doesn't track globals)
  tags: Tag[];                // JSDoc tags (e.g. @deprecated, @default)
  schema: PropertyMetaSchema; // recursive type schema for tooling
}

type PropertyMetaSchema = string | {
  kind: "enum" | "object" | "array";
  type: string;
  schema?: PropertyMetaSchema[];
};
```

### Schema Options

```ts
// Disable schema generation (all schemas return "unknown")
const checker = createChecker("./tsconfig.json", { schema: false });

// Ignore specific types in schema expansion
const checker = createChecker("./tsconfig.json", {
  schema: { ignore: (type) => type.includes("HTMLElement") },
});
```

### Accessing Verter Extensions via `_verter`

The compat result includes an optional `_verter` field with Verter's full native `ComponentMeta`. This gives opt-in access to metadata that Volar doesn't provide:

```ts
const meta = checker.getComponentMeta("./src/MyButton.vue");

if (meta._verter) {
  // Models — defineModel as first-class citizens
  meta._verter.models;
  // [{ name: "modelValue", type: { kind: "primitive", name: "string" } }]

  // Child component usage in template
  meta._verter.components;
  // [{ name: "Icon", importSource: "./Icon.vue", props: [...], slotsUsed: [...] }]

  // Template ref analysis
  meta._verter.templateRefs;
  // [{ name: "buttonEl", isDynamic: false, targetTag: "button" }]

  // Style analysis per style block
  meta._verter.styles;
  // [{ lang: "Css", scoped: true, classes: ["btn"], selectors: [...] }]

  // Quick boolean flags
  meta._verter.flags;
  // { hasReactiveState: true, hasComputed: false, hasWatchers: false, ... }

  // Script bindings with reactivity classification
  meta._verter.bindings;
  // [{ name: "count", reactivityKind: "ref", usedInTemplate: true }]

  // Vue API call sites
  meta._verter.vueApiCalls;
  // [{ api: "OnMounted" }, { api: "Watch", argValue: "count" }]

  // Import analysis
  meta._verter.imports;
  // [{ source: "vue", bindings: [{ name: "ref" }, { name: "onMounted" }] }]
}
```

### Native API vs Compat: When to Use Which {#comparison}

| | **Native API** (`@verter/component-meta`) | **Compat API** (`./compat`) |
|---|---|---|
| **Use when** | Building new tooling, need rich metadata | Replacing `vue-component-meta` in existing code |
| **API style** | Functional: `createAdapter()` → `upsert()` → `extractComponentMeta()` | Class-based: `createChecker(tsconfig)` → `checker.getComponentMeta()` |
| **Type system** | `TypeDescriptor` (11-kind discriminated union, JSON-serializable) | `PropertyMetaSchema` (recursive `string \| { kind, type, schema }`) |
| **Props type** | `PropMeta` with `type: TypeDescriptor`, `hasDefault`, `runtimeTypes` | `PropertyMeta` with `type: string`, `schema: PropertyMetaSchema` |
| **Events type** | `EventMeta` with `payload: TypeDescriptor`, `hasValidator`, `isDeclared` | `PropertyMeta` with `type: string` (same shape as props) |
| **Models** | First-class `ModelMeta[]` | Not at top level (access via `_verter`) |
| **Template analysis** | Components, template refs, bindings, Vue API calls | Not at top level (access via `_verter`) |
| **Style analysis** | Classes, selectors, specificity, `v-bind()`, CSS custom properties | Not at top level (access via `_verter`) |
| **Component flags** | `flags: { hasReactiveState, hasComputed, hasWatchers, ... }` | Not at top level (access via `_verter`) |
| **File management** | Manual `adapter.upsert()` per file | Auto-discovers from tsconfig, `updateFile()` / `deleteFile()` |
| **Environment** | Node.js (NAPI) or Browser (WASM) | Node.js only (NAPI, reads files from disk) |
| **Adapters** | Storybook, Histoire, Zod, JSON Schema | N/A (use native API result) |

**Recommendation**: Use the native API for new projects — it's more expressive and works in both Node.js and browser environments. Use the compat API when you need to swap out `vue-component-meta` with zero code changes.

## Host Adapters

```ts
import {
  createAdapter,       // auto-detect (prefers NAPI)
  createNapiAdapter,   // NAPI only
  createWasmAdapter,   // WASM only (async)
  wrapNapiHost,        // wrap an existing NAPI VerterHost
  wrapWasmHost,        // wrap an existing WASM Host
} from "@verter/component-meta";
```
