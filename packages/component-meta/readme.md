# @verter/component-meta

Extract Vue component metadata (props, events, slots, models, expose, imports, bindings, styles, flags) from Single File Components into a structured format. Includes a generic **Type IR** and adapters for Storybook, Histoire, Zod, and JSON Schema.

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

const adapter = createAdapter(); // auto-detects NAPI or WASM

adapter.upsert({
  inputId: "MyButton.vue",
  source: `
<script setup lang="ts">
import { ref, onMounted } from 'vue'
import Icon from './Icon.vue'

defineProps<{
  label: string
  variant?: 'primary' | 'secondary'
  disabled?: boolean
}>()

defineEmits<{
  (e: 'click', payload: MouseEvent): void
}>()

const count = ref(0)

onMounted(() => {
  console.log('mounted')
})
</script>

<template>
  <button ref="buttonEl" :disabled="disabled" @click="$emit('click', $event)">
    <Icon name="check" />
    <slot>{{ label }}</slot>
  </button>
</template>

<style scoped>
.btn {
  --btn-color: blue;
  color: var(--btn-color);
}
</style>
`,
});

const meta = extractComponentMeta(adapter, "MyButton.vue");
console.log(meta);
// {
//   filePath: "MyButton.vue",
//   componentName: "MyButton",
//   optionsApi: false,
//   props: [
//     { name: "label", type: { kind: "primitive", name: "string" }, required: true, ... },
//     { name: "variant", type: { kind: "union", types: [...] }, required: false, ... },
//     { name: "disabled", type: { kind: "primitive", name: "boolean" }, required: false, ... },
//   ],
//   events: [{ name: "click", ... }],
//   slots: [{ name: "default", isScoped: false, bindings: [] }],
//   models: [],
//   exposed: [],
//   components: [
//     { name: "Icon", importSource: "./Icon.vue", isDynamic: false, props: ["name"], ... }
//   ],
//   templateRefs: [{ name: "buttonEl", isDynamic: false }],
//   imports: [
//     { source: "vue", isTypeOnly: false, bindings: [{ name: "ref", ... }, { name: "onMounted", ... }] },
//     { source: "./Icon.vue", isTypeOnly: false, bindings: [{ name: "Icon", ... }] },
//   ],
//   bindings: [
//     { name: "count", reactivityKind: "ref", usedInTemplate: false, usedInStyle: false },
//   ],
//   vueApiCalls: [{ api: "OnMounted" }],
//   styles: [{
//     lang: "Css", scoped: true, isModule: false,
//     classes: ["btn"], ids: [], customProperties: ["--btn-color"], vBinds: [],
//     selectors: [{ text: ".btn", specificity: [0, 1, 0] }],
//   }],
//   flags: {
//     asyncSetup: false, hasReactiveState: true, hasComputed: false,
//     hasWatchers: false, hasLifecycleHooks: true, hasProvide: false,
//     hasInject: false, hasInheritAttrsFalse: false, hasStoreUsage: false,
//   },
// }
```

## JSDoc Extraction

Props, events, and slots automatically extract JSDoc comments:

```ts
adapter.upsert({
  inputId: "MyButton.vue",
  source: `
<script setup lang="ts">
defineProps<{
  /**
   * The button label.
   * @default "Submit"
   * @deprecated Use \`text\` instead
   */
  label: string
}>()
</script>
<template><button>{{ label }}</button></template>
`,
});

const meta = extractComponentMeta(adapter, "MyButton.vue");
meta.props[0].description; // "The button label."
meta.props[0].tags;
// [
//   { name: "default", text: "\"Submit\"" },
//   { name: "deprecated", text: "Use `text` instead" },
// ]
```

Works with both type-based (`defineProps<{}>()`) and runtime (`defineProps({})`) declarations.

## Volar Compatibility (`./compat`)

Drop-in replacement for Volar's `vue-component-meta`. Consumers like `nuxt-component-meta` and Nuxt UI docs can swap with zero code changes:

```diff
- import { createChecker } from 'vue-component-meta'
+ import { createChecker } from '@verter/component-meta/compat'
```

```ts
import { createChecker } from "@verter/component-meta/compat";

const checker = createChecker("./tsconfig.json");
const meta = checker.getComponentMeta("./src/MyButton.vue");

// Volar-compatible shape
meta.props;   // PropertyMeta[] (name, description, type, required, tags, schema)
meta.events;  // PropertyMeta[]
meta.slots;   // PropertyMeta[]
meta.exposed; // PropertyMeta[]

// Verter extension (opt-in)
meta._verter; // Full ComponentMeta with models, components, styles, flags, etc.
```

See the [compat API docs](https://verterjs.dev/api/component-meta#compat) for full details.

## Slots

Slot metadata is extracted from `<slot>` elements in the template. Named slots, scoped slots, and default slots are all captured:

```ts
const meta = extractComponentMeta(adapter, "DataTable.vue");
// DataTable.vue has:
//   <slot name="header" :columns="columns" :sortBy="sortBy" />
//   <slot name="row" :item="item" :index="index" />
//   <slot name="empty" />
//   <slot />  (default)

console.log(meta.slots);
// [
//   { name: "default", isScoped: false, bindings: [] },
//   { name: "header", isScoped: true, bindings: [
//     { name: "columns", type: { kind: "unknown", rawType: "unknown" } },
//     { name: "sortBy", type: { kind: "unknown", rawType: "unknown" } },
//   ]},
//   { name: "row", isScoped: true, bindings: [
//     { name: "item", type: { kind: "unknown", rawType: "unknown" } },
//     { name: "index", type: { kind: "unknown", rawType: "unknown" } },
//   ]},
//   { name: "empty", isScoped: false, bindings: [] },
// ]
```

Note: Slot binding types are currently `unknown` — resolving them from `defineSlots<{}>()` type params is future work.

## Rich Analysis

Beyond props/events/slots, the metadata includes deeper analysis of each SFC:

### Components

Child components used in the template, with props passed, slots used, and class bindings:

```ts
meta.components
// [{ name: "MyButton", importSource: "./MyButton.vue", isDynamic: false,
//    props: ["label", "disabled"], slotsUsed: ["default"],
//    staticClasses: ["btn"], hasDynamicClass: false, vModels: [] }]
```

### Template Refs

All `ref="..."` usages, with dynamic ref detection:

```ts
meta.templateRefs
// [{ name: "inputEl", isDynamic: false }, { name: "container", isDynamic: false }]
```

### Bindings

Script-level bindings with reactivity classification and usage tracking:

```ts
meta.bindings
// [{ name: "count", reactivityKind: "ref", usedInTemplate: true, usedInStyle: false },
//  { name: "doubled", reactivityKind: "computed", usedInTemplate: true, usedInStyle: false }]
```

### Vue API Calls

Lifecycle hooks, watchers, provide/inject, and other Vue API call sites:

```ts
meta.vueApiCalls
// [{ api: "OnMounted" }, { api: "Watch", argValue: "count" }, { api: "Provide", argValue: "theme" }]
```

### Styles

Per-style-block analysis with classes, IDs, CSS variables, selectors, and specificity:

```ts
meta.styles
// [{ lang: "Scss", scoped: true, isModule: false,
//    classes: ["btn", "active"], ids: ["app"],
//    customProperties: ["--primary-color"], vBinds: ["color"],
//    selectors: [{ text: ".btn", specificity: [0, 1, 0] }] }]
```

### Flags

Quick boolean checks for component characteristics (O(1) bitflag reads):

```ts
meta.flags
// { asyncSetup: false, hasReactiveState: true, hasComputed: true,
//   hasWatchers: false, hasLifecycleHooks: true, hasProvide: false,
//   hasInject: false, hasInheritAttrsFalse: false, hasStoreUsage: false }
```

## Adapters

Each adapter is a separate export path so you only pay for what you use.

### Storybook

```ts
import { toArgTypes } from "@verter/component-meta/storybook";

const argTypes = toArgTypes(meta);
// {
//   label:    { type: { name: "string", required: true }, control: { type: "text" }, ... },
//   variant:  { control: { type: "select", options: ["primary", "secondary"] }, ... },
//   disabled: { control: { type: "boolean" }, ... },
//   onClick:  { action: "click", table: { category: "events" } },
// }
```

### Histoire

```ts
import {
  toHistoireConfig,
  generateDefaultProps,
  generateVariants,
} from "@verter/component-meta/histoire";

const config = toHistoireConfig(meta);
// { title: "MyButton", variants: [{ title: "Default", props: { label: "", variant: "primary", ... } }] }

const variants = generateVariants(meta);
// [{ title: 'variant: primary', props: { ... } }, { title: 'variant: secondary', props: { ... } }]
```

### Zod

Requires `zod` as a peer dependency for runtime mode.

```ts
import { propsToZodString, propsToZodSchema } from "@verter/component-meta/zod";

// Codegen — outputs schema source code as a string
const code = propsToZodString(meta);
// z.object({
//   "label": z.string(),
//   "variant": z.union([z.literal("primary"), z.literal("secondary")]).optional(),
//   "disabled": z.boolean().optional()
// })

// Runtime — builds an actual z.ZodType instance
const schema = propsToZodSchema(meta);
schema.parse({ label: "Click me", variant: "primary" }); // validates
```

### JSON Schema

```ts
import { propsToJsonSchema } from "@verter/component-meta/json-schema";

const schema = propsToJsonSchema(meta);
// {
//   type: "object",
//   properties: {
//     label: { type: "string" },
//     variant: { enum: ["primary", "secondary"] },
//     disabled: { type: "boolean" },
//   },
//   required: ["label"],
// }
```

## Type IR

All extracted types use a generic **TypeDescriptor** — a JSON-serializable discriminated union. Factory helpers are provided for programmatic construction:

```ts
import { primitive, literal, union, array, object, parseType } from "@verter/component-meta";

// Parse a type annotation string
const type = parseType("string | number");
// { kind: "union", types: [{ kind: "primitive", name: "string" }, { kind: "primitive", name: "number" }] }

// Build types programmatically
const buttonSize = union([literal("sm"), literal("md"), literal("lg")]);
```

### Supported type kinds

| Kind | Example | Factory |
|------|---------|---------|
| `primitive` | `string`, `number`, `boolean`, ... | `primitive("string")` |
| `literal` | `'primary'`, `42`, `true` | `literal("primary")` |
| `union` | `A \| B` | `union([a, b])` |
| `intersection` | `A & B` | `intersection([a, b])` |
| `array` | `string[]`, `Array<string>` | `array(primitive("string"))` |
| `tuple` | `[string, number]` | `tuple([...])` |
| `object` | `{ key: string }` | `object([...])` |
| `function` | `(x: string) => void` | `func([...], returnType)` |
| `enum` | enum members | `—` |
| `ref` | `Map<K, V>`, `MyType` | `ref("Map", [...])` |
| `unknown` | *(fallback)* | `unknown(rawType)` |

## Host Adapters

The extraction engine works with both NAPI (Node.js native) and WASM backends:

```ts
import {
  createAdapter,       // auto-detect (prefers NAPI)
  createNapiAdapter,   // NAPI only
  createWasmAdapter,   // WASM only (async)
  wrapNapiHost,        // wrap an existing NAPI VerterHost
  wrapWasmHost,        // wrap an existing WASM Host
} from "@verter/component-meta";
```

If you already have a `VerterHost` instance (e.g. from `@verter/unplugin`), wrap it instead of creating a new one:

```ts
import { wrapNapiHost, extractComponentMeta } from "@verter/component-meta";
import { VerterHost } from "@verter/native";

const host = new VerterHost({ devMode: false, analysisLevel: "full" });
const adapter = wrapNapiHost(host);
// ... upsert files via the existing host, then extract metadata
```

## API Reference

### Core

| Function | Description |
|----------|-------------|
| `extractComponentMeta(adapter, fileId, filePath?)` | Extract metadata from a compiled SFC |
| `snapshotToMeta(snapshot, filePath)` | Convert a raw analysis snapshot to `ComponentMeta` |
| `parseType(input)` | Parse a TS type annotation string into a `TypeDescriptor` |
| `runtimeTypeToDescriptor(name)` | Convert a Vue runtime constructor (`"String"`) to a `TypeDescriptor` |

### Types

| Type | Description |
|------|-------------|
| `ComponentMeta` | Full component metadata |
| `PropMeta` | Prop declaration |
| `EventMeta` | Event declaration |
| `SlotMeta` | Slot with optional scoped bindings |
| `ModelMeta` | `defineModel` declaration |
| `ExposedMeta` | `defineExpose` member |
| `ComponentUsage` | Child component used in template |
| `TemplateRefMeta` | Template `ref` attribute |
| `ImportMeta` | Import statement |
| `BindingMeta` | Script binding with reactivity |
| `VueApiCallMeta` | Vue API call site |
| `StyleMeta` | Style block analysis |
| `SelectorMeta` | CSS selector with specificity |
| `ComponentFlags` | Boolean component characteristics |
| `JsdocTag` | JSDoc tag (`{ name, text? }`) |

### Compat (Volar Drop-in)

| Export path | Symbol | Description |
|-------------|--------|-------------|
| `./compat` | `createChecker(tsconfig, options?)` | Create a checker from a tsconfig path |
| `./compat` | `createCheckerByJson(root, config, options?)` | Create a checker from a JSON config object |
| `./compat` | `ComponentMetaChecker` | Checker class |
| `./compat` | `PropertyMeta` | Volar-compatible property metadata type |
| `./compat` | `PropertyMetaSchema` | Recursive schema type |
| `./compat` | `MetaCheckerOptions` | Checker options |
| `./compat` | `typeDescriptorToString(td)` | TypeDescriptor → human-readable string |
| `./compat` | `typeDescriptorToSchema(td, options?)` | TypeDescriptor → PropertyMetaSchema |

### Adapters

| Export path | Function | Description |
|-------------|----------|-------------|
| `./storybook` | `toArgTypes(meta)` | Storybook argTypes with controls |
| `./histoire` | `toHistoireConfig(meta)` | Histoire story config |
| `./histoire` | `generateDefaultProps(meta)` | Sensible default prop values |
| `./histoire` | `generateVariants(meta)` | One variant per union/enum value |
| `./zod` | `typeToZodString(type)` | Zod schema as code string |
| `./zod` | `propsToZodString(meta)` | Props Zod object schema string |
| `./zod` | `typeToZodSchema(type)` | Runtime Zod schema instance |
| `./zod` | `propsToZodSchema(meta)` | Runtime props Zod object schema |
| `./json-schema` | `typeToJsonSchema(type)` | JSON Schema (draft-07) |
| `./json-schema` | `propsToJsonSchema(meta)` | Props JSON Schema object |

## License

MIT
