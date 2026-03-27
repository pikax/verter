# Features

::: warning Pre-Release
Verter is pre-release software. APIs may change between releases — see the [API Stability](/api-stability) document.
:::

Verter provides full TypeScript type safety for Vue Single File Components by generating real TSX output. This page covers the key features that go beyond what standard Vue tooling offers.

## Generic Components

Vue's `generic` attribute on `<script setup>` lets you parameterize a component with type variables. Verter fully supports generic components, including generic slots and constrained type parameters.

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

When consuming the component, the generic parameter flows through to `InstanceType` and props:

```typescript
import Comp from "./Comp.vue";

const foo = {} as InstanceType<typeof Comp<"myName">>;
foo.$props.name; // Type: 'myName'
```

Generic components work with all Vue features including slots, emits, and expose. The type parameter is preserved through the entire compilation pipeline, so downstream consumers get full type inference.

## Automatic Event Handler Type Inference

When you bind an event handler to a native HTML element, Verter infers the event parameter type from `HTMLElementEventMap`. You do not need to manually annotate the event parameter -- the correct type is inferred from the element and event name.

```vue
<script setup lang="ts">
function handleClick(e) {
  // e is inferred as MouseEvent from HTMLElementEventMap["click"]
  console.log(e.clientX, e.clientY);
}
</script>

<template>
  <button @click="handleClick">Click me</button>
</template>
```

This works for all native DOM events. The inference is based on the element type (`HTMLButtonElement`, `HTMLInputElement`, etc.) and the event name, using the standard `HTMLElementEventMap` type definitions.

## Fully Typed Vue Directives

Custom directives receive full type checking for their value, argument, and modifiers. Verter validates all three at compile time.

```vue
<script setup lang="ts">
import type { Directive } from "vue";

const vColor: Directive<HTMLElement, string, "red" | "blue"> = (el, binding) => {
  el.style.color = binding.value;
};
</script>

<template>
  <!-- Valid: "blue" is an allowed modifier -->
  <span v-color.blue="'red'" />

  <!-- Type error: "green" is not in "red" | "blue" -->
  <span v-color.green="'red'" />

  <!-- Built-in modifiers are typed too -->
  <input v-model.number.trim="count" />
</template>
```

The directive type signature `Directive<Element, Value, Modifiers>` constrains:

- **Element** -- which HTML elements the directive can be applied to
- **Value** -- the type of the binding value
- **Modifiers** -- a union of allowed modifier names

Invalid modifiers or value types produce compile-time type errors rather than silent runtime failures.

## Script Setup Macros

Verter supports all Vue script setup macros with full type inference:

| Macro           | Purpose                                                   |
| --------------- | --------------------------------------------------------- |
| `defineProps`   | Declare component props with type-based or runtime syntax |
| `defineEmits`   | Declare emitted events with typed payloads                |
| `defineModel`   | Two-way binding prop declaration (Vue 3.4+)               |
| `defineSlots`   | Typed slot definitions                                    |
| `defineExpose`  | Control which bindings are exposed to parent refs         |
| `withDefaults`  | Provide default values for type-based props               |
| `defineOptions` | Set component options (name, inheritAttrs, etc.)          |

All macros work with both inline type literals and imported types. Cross-file type resolution is supported -- you can define your prop types in a separate `.ts` file and import them into `defineProps<Props>()`.

```vue
<script setup lang="ts">
import type { UserProps } from "./types";

const props = withDefaults(defineProps<UserProps>(), {
  role: "viewer",
});

const emit = defineEmits<{
  update: [name: string, value: unknown];
  delete: [id: number];
}>();

const model = defineModel<string>({ required: true });

defineSlots<{
  default: (props: { item: string }) => any;
  header: () => any;
}>();

defineExpose({ reset: () => {} });
</script>
```

## Options API Support

Verter provides full Options API support with type inference. Components defined with `defineComponent` and the Options API (`data`, `computed`, `methods`, `watch`, etc.) receive the same level of type checking as script setup components.

```vue
<script lang="ts">
import { defineComponent } from "vue";

export default defineComponent({
  props: {
    title: { type: String, required: true },
    count: { type: Number, default: 0 },
  },
  data() {
    return {
      message: "hello",
    };
  },
  computed: {
    greeting(): string {
      // this.title and this.message are correctly typed
      return `${this.title}: ${this.message}`;
    },
  },
  methods: {
    increment() {
      // this.count is number
      this.$emit("update", this.count + 1);
    },
  },
});
</script>
```

## JSX/TSX Interoperability

Vue SFCs compiled by Verter can be seamlessly used in JSX/TSX projects. Since Verter generates real TSX, the type information is natively compatible with TypeScript's JSX support.

```tsx
// App.tsx
import MyComponent from "./MyComponent.vue";

export default function App() {
  return <MyComponent title="Hello" count={42} onUpdate={(value) => console.log(value)} />;
}
```

Props, events, slots, and generic parameters are all available with full type inference when consuming `.vue` components from `.tsx` files. No additional configuration or type shims are needed.

## Next Steps

- [Getting Started](./getting-started) -- Install Verter and set up your project
- [Performance](./performance) -- Compilation benchmark results
- [Diagnostics & Linting](./linting) -- Built-in diagnostic rules
- [Architecture](./architecture) -- How Verter works under the hood
