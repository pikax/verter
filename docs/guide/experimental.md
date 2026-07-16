# Experimental Features

Experimental features are behind opt-in flags and may change or be removed in future versions.

## Conditional Root Narrowing

**Setting:** `verter.experimental.conditionalRootNarrowing` (default: `false`)

When a Vue component has conditional root elements (`v-if`/`v-else-if`/`v-else`) controlled by prop values, this feature converts those props into TypeScript generic type parameters on the component's constructor. This enables TypeScript to narrow the root element type at the call site based on the prop value passed by the parent.

### Example

```vue
<script setup lang="ts">
const props = defineProps<{ mode: "light" | "dark"; simple?: boolean }>();
</script>
<template>
  <div v-if="simple">Simple mode</div>
  <canvas v-else-if="mode === 'dark'">Dark canvas</canvas>
  <section v-else>Light section</section>
</template>
```

With this feature enabled, the component's type signature gains generic parameters `T_simple` and `T_mode` that mirror the prop types. When a parent passes `<MyComp simple />`, TypeScript knows the root element is `HTMLDivElement`. When it passes `<MyComp mode="dark" />`, the root is `HTMLCanvasElement`.

### How to Enable

In VS Code settings (JSON):

```json
{
  "verter.experimental.conditionalRootNarrowing": true
}
```

Or in the UI: search for "conditionalRootNarrowing" in Settings.

### Supported Condition Patterns

The following simple patterns are supported for narrowing:

| Pattern          | Example                  | Narrows to                      |
| ---------------- | ------------------------ | ------------------------------- |
| Bare prop        | `v-if="show"`            | `T_show extends true ? A : B`   |
| Negated prop     | `v-if="!show"`           | `T_show extends false ? A : B`  |
| Prop === string  | `v-if="mode === 'dark'"` | `T_mode extends 'dark' ? A : B` |
| Prop !== string  | `v-if="mode !== 'dark'"` | Inverted conditional            |
| Prop === number  | `v-if="count === 42"`    | `T_count extends 42 ? A : B`    |
| Prop === boolean | `v-if="flag === true"`   | `T_flag extends true ? A : B`   |

### Unsupported Patterns

Complex conditions fall back to the standard union type (no narrowing). When the feature is enabled, a `conditional-root-complex` warning diagnostic is shown for unsupported patterns:

- **Logical operators:** `v-if="show && variant === 'dark'"`
- **Member expressions:** `v-if="items.length > 0"`
- **Function calls:** `v-if="isReady()"`
- **Non-prop bindings:** `v-if="computedValue"` (refs, computed, etc.)
- **Template literals:** ``v-if="`${x}`"``

### Interaction with SFC Generics

If the component already uses `<script setup generic="T extends string">`, narrowing generics are appended after the existing ones:

```typescript
new<T extends string, T_show extends boolean = boolean>(): { ... }
```

### Known Limitations

- Only root-level `v-if`/`v-else-if`/`v-else` elements are analyzed (not nested conditionals).
- Each condition must reference exactly one prop. Multi-prop conditions are not supported.
- The feature is experimental and its behavior may change.

## Strict props vs fallthrough (Vue) and Svelte

Verter’s Vue surface is **strict-first**:

- Props that are neither **declared** nor proven by **fallthrough / root inheritance** are errors (unlike Volar’s looser unknown-prop acceptance).
- When Verter **accepts** `class` / `data-*` / etc. on a wrapper that only declares e.g. `tone`, that is because fallthrough proved a native (or nested) root accepts them — not because unknown names are ignored.

Svelte does **not** use Vue multi-hop fallthrough. Typed `$props()` are strict; extra attrs require an **author-declared** rest surface (`...rest`), not automatic inheritance.

E2E contract: `packages/vue-vscode/e2e/STRICT_PROPS.md` and `parity/shared/strict-props.test.ts`.

## Expose Bindings Testing (`.spec.ts` / test importers)

**Setting:** `verter.experimental.exposeBindingsTesting` (default: `false`)

**Also:** `@verter/typescript-plugin` option `exposeBindingsTesting` in `tsconfig.json` / `jsconfig.json`.

Vue and Svelte are both first-class carriers. This experimental flag is **framework-aware**: it changes how **test files** resolve **Vue** component imports. Svelte keeps a single public instance shape (no testing virtual file).

### What it does (Vue)

When enabled, importers classified as **test files** resolve `.vue` imports to Verter’s **testing API** virtual file:

| Importer kind        | Virtual file                     | Instance shape                                                                                   |
| -------------------- | -------------------------------- | ------------------------------------------------------------------------------------------------ |
| App / library source | `Foo.vue.verter.ts` / public API | Public props, emits, **and** `defineExpose()` only                                               |
| Test file            | `Foo.vue.__verter_test.ts`       | VTU-style debug surface: **all** `<script setup>` bindings, **not** narrowed by `defineExpose()` |

That matches Vue Test Utils `wrapper.vm` property access in unit tests: internals are visible under types without weakening the public component type used by production code.

### Test-file classification

Shared across the TypeScript plugin and editor wiring (same heuristics for every framework):

- Filename: `*.spec.*`, `*.test.*`
- Directories: `__tests__/`, `__specs__/`
- Plus Vitest / Vite / Jest include patterns when config can be read

### Vue example

```vue
<!-- Counter.vue -->
<script setup lang="ts">
import { ref } from "vue";
const count = ref(0);
const hidden = ref("secret");
defineExpose({ count });
</script>
```

```ts
// Counter.spec.ts  — testing surface (with the flag on)
import Counter from "./Counter.vue";

function probe(c: InstanceType<typeof Counter>) {
  c.count; // ok
  c.hidden; // ok under testing API (setup binding)
}
```

```ts
// useCounter.ts  — public surface (unchanged)
import Counter from "./Counter.vue";

function probe(c: InstanceType<typeof Counter>) {
  c.count; // ok if exposed
  // c.hidden — type error on the public surface
}
```

### How to enable

**VS Code / Verter extension** (workspace or user `settings.json`):

```json
{
  "verter.experimental.exposeBindingsTesting": true
}
```

The extension only forwards the flag into `@verter/typescript-plugin` when it is set **explicitly** (so a project `tsconfig` default can remain authoritative until you opt in from the editor).

**TypeScript plugin** (`tsconfig.json`):

```jsonc
{
  "compilerOptions": {
    "plugins": [
      {
        "name": "@verter/typescript-plugin",
        "exposeBindingsTesting": true,
      },
    ],
  },
}
```

### Svelte (first-class, different contract)

|                      | Vue                                                   | Svelte                                                                                       |
| -------------------- | ----------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| Public API virtual   | `*.vue.verter.ts`                                     | `*.svelte.verter.ts`                                                                         |
| Testing API virtual  | `*.vue.__verter_test.ts` when flag on + test importer | **None** (`testing_api_suffix` is null)                                                      |
| `*.spec.ts` importer | Testing / debug instance shape                        | **Same public shape** as app code                                                            |
| Setting still valid? | Yes — enables dual surface                            | Yes — test-filename heuristics apply, but **no second Svelte instance surface is generated** |

So enabling the flag in a mixed Vue+Svelte workspace is safe: Vue tests get VTU-style types; Svelte components keep a single public type for both app and test importers. There is no `.svelte.__verter_test.ts` name.

### Known limitations

- Experimental: virtual suffixes and config may change.
- Vue-only testing content producer today (`PublicApiMode::Testing`); Svelte returns no testing virtual file by design until a future framework-neutral testing surface is designed.
- Classification depends on test-file heuristics / test-runner config; exotic test layouts may need an explicit Vitest/Jest include pattern.
- E2E coverage lives under `packages/vue-vscode/e2e/suite/parity/shared/testing-api-surface.test.ts` (vue-parity + svelte-parity fixtures).
