# Experimental Features

Experimental features are behind opt-in flags and may change or be removed in future versions.

## Conditional Root Narrowing

**Setting:** `verter.experimental.conditionalRootNarrowing` (default: `false`)

When a Vue component has conditional root elements (`v-if`/`v-else-if`/`v-else`) controlled by prop values, this feature converts those props into TypeScript generic type parameters on the component's constructor. This enables TypeScript to narrow the root element type at the call site based on the prop value passed by the parent.

### Example

```vue
<script setup lang="ts">
const props = defineProps<{ mode: 'light' | 'dark', simple?: boolean }>()
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

| Pattern | Example | Narrows to |
|---------|---------|------------|
| Bare prop | `v-if="show"` | `T_show extends true ? A : B` |
| Negated prop | `v-if="!show"` | `T_show extends false ? A : B` |
| Prop === string | `v-if="mode === 'dark'"` | `T_mode extends 'dark' ? A : B` |
| Prop !== string | `v-if="mode !== 'dark'"` | Inverted conditional |
| Prop === number | `v-if="count === 42"` | `T_count extends 42 ? A : B` |
| Prop === boolean | `v-if="flag === true"` | `T_flag extends true ? A : B` |

### Unsupported Patterns

Complex conditions fall back to the standard union type (no narrowing). When the feature is enabled, a `conditional-root-complex` warning diagnostic is shown for unsupported patterns:

- **Logical operators:** `v-if="show && variant === 'dark'"`
- **Member expressions:** `v-if="items.length > 0"`
- **Function calls:** `v-if="isReady()"`
- **Non-prop bindings:** `v-if="computedValue"` (refs, computed, etc.)
- **Template literals:** `` v-if="`${x}`" ``

### Interaction with SFC Generics

If the component already uses `<script setup generic="T extends string">`, narrowing generics are appended after the existing ones:

```typescript
new<T extends string, T_show extends boolean = boolean>(): { ... }
```

### Known Limitations

- Only root-level `v-if`/`v-else-if`/`v-else` elements are analyzed (not nested conditionals).
- Each condition must reference exactly one prop. Multi-prop conditions are not supported.
- The feature is experimental and its behavior may change.
