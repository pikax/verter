# @verter/component-meta

Extract Vue component metadata (props, events, slots, models, expose) from Single File Components into a structured format. Includes a generic **Type IR** and adapters for Storybook, Histoire, Zod, and JSON Schema.

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
defineProps<{
  label: string
  variant?: 'primary' | 'secondary'
  disabled?: boolean
}>()

defineEmits<{
  (e: 'click', payload: MouseEvent): void
}>()
</script>

<template>
  <button :disabled="disabled" @click="$emit('click', $event)">
    <slot>{{ label }}</slot>
  </button>
</template>
`,
});

const meta = extractComponentMeta(adapter, "MyButton.vue");
console.log(meta);
// {
//   filePath: "MyButton.vue",
//   componentName: "MyButton",
//   apiStyle: "composition",
//   props: [
//     { name: "label", type: { kind: "primitive", name: "string" }, required: true, ... },
//     { name: "variant", type: { kind: "union", types: [{ kind: "literal", value: "primary" }, ...] }, ... },
//     { name: "disabled", type: { kind: "primitive", name: "boolean" }, ... },
//   ],
//   events: [{ name: "click", ... }],
//   slots: [{ name: "default", isScoped: false, bindings: [] }],
//   models: [],
//   exposed: [],
// }
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
