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

### Factory Functions

```ts
import { createChecker, createCheckerByJson } from "@verter/component-meta/compat";

// From a tsconfig.json path
const checker = createChecker("./tsconfig.json", {
  schema: true, // enable PropertyMetaSchema generation (default: true)
});

// From a JSON object
const checker2 = createCheckerByJson("/project/root", {
  include: ["src/**/*.vue"],
  compilerOptions: { strict: true },
});
```

### Checker API

```ts
// Get component metadata in Volar-compatible shape
const meta = checker.getComponentMeta("./src/MyButton.vue");

// meta.props: PropertyMeta[]
// meta.events: PropertyMeta[]
// meta.slots: PropertyMeta[]
// meta.exposed: PropertyMeta[]
// meta._verter: ComponentMeta (full Verter metadata, opt-in)

// Get export names
const exports = checker.getExportNames("./src/MyButton.vue");
// ["default"]

// Update a file (hot reload)
checker.updateFile("./src/MyButton.vue", newSource);

// Delete a file
checker.deleteFile("./src/MyButton.vue");

// Re-read all tracked files from disk
checker.reload();

// Clear internal caches
checker.clearCache();
```

### PropertyMeta Shape

The compat layer maps Verter's rich types to Volar's `PropertyMeta` shape:

```ts
interface PropertyMeta {
  name: string;
  description: string;     // from JSDoc
  type: string;            // human-readable type string
  default?: string;
  required: boolean;
  global?: boolean;
  tags: Tag[];             // JSDoc tags
  schema: PropertyMetaSchema;  // recursive type schema
}
```

### Schema Options

```ts
// Disable schema generation (returns "unknown" for all schemas)
const checker = createChecker("./tsconfig.json", { schema: false });

// Ignore specific types in schema expansion
const checker = createChecker("./tsconfig.json", {
  schema: { ignore: (type) => type.includes("HTMLElement") },
});
```

### Verter Extensions

The compat `ComponentMeta` includes an optional `_verter` field with the full Verter native metadata, giving opt-in access to:

- **Models** — `defineModel` as first-class `ModelMeta[]`
- **Template usage** — child components, template refs, binding occurrences
- **Style analysis** — CSS classes, selectors, specificity, `v-bind()` expressions
- **Component flags** — reactive state, computed, watchers, lifecycle, store usage
- **Vue API calls** — lifecycle hooks, watchers, provide/inject call sites
- **Import analysis** — import sources, type-only classification

### Performance

Verter's Rust-powered analysis is significantly faster than Volar's TypeScript-based approach. The compat layer adds negligible overhead — the mapping from Verter types to Volar shapes is O(n) in the number of props/events/slots.

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
