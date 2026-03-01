# @verter/types

::: warning Experimental
Verter is experimental software at v0.0.1-alpha.3. APIs may change without notice.
:::

TypeScript utility types for Vue SFC type safety. These types power Verter's TSX type checking and IDE integration, providing type-safe props, emits, models, slots, and directives.

## Installation

```bash
pnpm add -D @verter/types
```

## Export Paths

| Export | Description |
|--------|-------------|
| `@verter/types` | All type helpers (main entry) |
| `@verter/types/string` | String export with `$V_` prefixed types for LSP injection |
| `@verter/types/tsx` | JSX `IntrinsicClassAttributes` augmentations for Vue components |
| `@verter/types/tsx-string` | TSX augmentations as string (for runtime injection) |

## Core Helpers

### `PatchHidden<T, E>`

Attaches hidden metadata to a type via a unique symbol key. The hidden property does not affect the public interface of `T`.

```ts
import type { PatchHidden } from '@verter/types'

type WithMeta = PatchHidden<{ name: string }, { internal: true }>
// Has { name: string } publicly, but carries { internal: true } as hidden metadata
```

### `ExtractHidden<T, R>`

Extracts the hidden metadata from a type patched with `PatchHidden`. Returns `R` (default: `never`) if no hidden property exists.

```ts
import type { ExtractHidden } from '@verter/types'

type Meta = ExtractHidden<WithMeta>
// { internal: true }
```

### `PartialUndefined<T>`

Makes properties that can be `undefined` optional. Properties that cannot be `undefined` remain required.

```ts
import type { PartialUndefined } from '@verter/types'

type Props = { name: string; label: string | undefined }
type Result = PartialUndefined<Props>
// { name: string; label?: string | undefined }
```

### `UnionToIntersection<U>`

Converts a union type to an intersection type.

```ts
import type { UnionToIntersection } from '@verter/types'

type Result = UnionToIntersection<{ a: 1 } | { b: 2 }>
// { a: 1 } & { b: 2 }
```

### `OmitNever<T>`

Removes properties with type `never` from an object type.

```ts
import type { OmitNever } from '@verter/types'

type Result = OmitNever<{ a: string; b: never; c: number }>
// { a: string; c: number }
```

### `PickByValue<T, V>`

Picks properties from `T` whose values extend `V`.

```ts
import type { PickByValue } from '@verter/types'

type Result = PickByValue<{ a: string; b: number; c: string }, string>
// { a: string; c: string }
```

### `FunctionToObject<T>`

Converts a function type representing event emissions into an object type mapping event names to their argument tuples.

```ts
import type { FunctionToObject } from '@verter/types'

type Emit = (e: 'change', value: string) => void
type Result = FunctionToObject<Emit>
// Maps 'change' -> [string]
```

## Emits Helpers

### `EmitsToProps<T>`

Converts event emission function types into Vue-style `onXxx` props. For each event name `K`, creates a prop named `onK` (capitalized) that accepts the same arguments.

```ts
import type { EmitsToProps } from '@verter/types'

type Emits = ((e: 'change', value: string) => void) &
  ((e: 'update', id: number) => void)

type Props = EmitsToProps<Emits>
// { onChange?: (value: string) => void; onUpdate?: (id: number) => void }
```

### `ComponentEmitsToProps<T>`

Extracts emit event types from a Vue component constructor and converts them to props. Works with components created via `defineComponent`.

```ts
import type { ComponentEmitsToProps } from '@verter/types'

type EventProps = ComponentEmitsToProps<typeof MyComponent>
```

## Props Helpers

### `PropsWithDefaults<P, D>`

Marks which properties in `P` have defaults. `D` is a union of keys that have default values. Used internally by `withDefaults()` type resolution.

```ts
import type { PropsWithDefaults } from '@verter/types'

type Props = { name: string; count: number }
type WithDefs = PropsWithDefaults<Props, 'count'>
// Props where 'count' is marked as having a default
```

### `MakePublicProps<T>`

Derives the public (external) props type from a props type with defaults information. Properties that have defaults become optional in the public API.

```ts
import type { MakePublicProps } from '@verter/types'

// Used by component-type plugin to derive the $props type
type PublicProps = MakePublicProps<PropsWithDefaults<Props, 'count'>>
// { name: string; count?: number }
```

### `MakeInternalProps<T>`

Derives the internal props type. Properties with defaults are always defined (not optional), since Vue guarantees they have values at runtime.

```ts
import type { MakeInternalProps } from '@verter/types'

type InternalProps = MakeInternalProps<PropsWithDefaults<Props, 'count'>>
// { name: string; count: number } — count is always defined internally
```

### `MakeBooleanOptional<T>`

Makes boolean properties optional. Boolean props in Vue default to `false` when not provided, so they are always optional in the public API.

```ts
import type { MakeBooleanOptional } from '@verter/types'

type Props = { disabled: boolean; label: string }
type Result = MakeBooleanOptional<Props>
// { disabled?: boolean; label: string }
```

### `ExtractBooleanKeys<T>`

Extracts the keys of properties that are boolean types.

```ts
import type { ExtractBooleanKeys } from '@verter/types'

type Keys = ExtractBooleanKeys<{ disabled: boolean; label: string; active: boolean }>
// 'disabled' | 'active'
```

## Model Helpers

### `ModelToEmits<T>`

Converts `defineModel()` return types into emit function types. For each model property, creates an emit function for the `update:modelName` event.

```ts
import type { ModelToEmits } from '@verter/types'

// Given defineModel() returns:
// const name = defineModel<string>('name')
// const count = defineModel<number>('count')
type Emits = ModelToEmits<{ name: typeof name; count: typeof count }>
// ((e: 'update:name', arg: string) => any) & ((e: 'update:count', arg: number) => any)
```

### `ModelToProps<T>`

Converts `defineModel()` return types into Vue prop types. Each model becomes a prop with its value type. Boolean models default to `boolean` (not `boolean | undefined`).

```ts
import type { ModelToProps } from '@verter/types'

type Props = ModelToProps<{ name: typeof name; count: typeof count }>
// { name: string; count: number }
```

### `MacroToPropEvents<T>`

Converts model macro returns to `onUpdate:xxx` event prop types for JSX/TSX usage.

```ts
import type { MacroToPropEvents } from '@verter/types'

type EventProps = MacroToPropEvents<{ name: typeof name }>
// { 'onUpdate:name'?: (v: string) => any }
```

### `ModelToModelInfo<T>`

Maps an object type of `ModelRef` properties to their model information (type, setter, resolved name).

## Slots Helpers

### `SlotsToRender<T>`

Converts Vue slot types into JSX-compatible component types for rendering slots as components. Each slot becomes a component constructor that can be used in JSX/TSX templates.

```ts
import type { SlotsToRender } from '@verter/types'
import { defineComponent, SlotsType } from 'vue'

const Component = defineComponent({
  slots: {} as SlotsType<{
    default: (props: { msg: string }) => any
    header: (props: { title: string }) => any
    footer: () => any
  }>,
})

type Slots = SlotsToRender<typeof Component.$slots>
// {
//   default: { new(): { $props: { msg: string } } }
//   header: { new(): { $props: { title: string } } }
//   footer: { new(): { $props: {} } }
// }
```

### `strictRenderSlot(slot, children)`

Type-safe slot rendering function. Validates that slot children match the expected return type of the slot function. See the [Vue RFC discussion](https://github.com/vuejs/rfcs/discussions/733).

### `renderSlotJSX(slot)`

Converts a slot function into a JSX-compatible render callback. Returns a function that accepts a callback and returns `JSX.Element`.

```ts
import { renderSlotJSX } from '@verter/types'

// slot: (props: { msg: string }) => VNode[]
const render = renderSlotJSX(slot)
render(({ msg }) => <div>{msg}</div>)
```

### `extractArgumentsFromRenderSlot(component, slotName)`

Extracts the slot props (arguments) from a component instance for a given slot name. Used internally by the component-type plugin for scoped slot type checking.

```ts
import { extractArgumentsFromRenderSlot } from '@verter/types'

const slotProps = extractArgumentsFromRenderSlot(new MyComponent(), 'default')
// Type is Parameters<MyComponent.$slots['default']>[0]
```

## Directive Helpers

### `vOnModifiers<El, Event>`

Computes valid `v-on` modifier keys for a given element and event type. Modifiers are conditionally available based on the event type:

- `stop` -- available when event has `stopPropagation()`
- `prevent` -- available when event has `preventDefault()`
- `self` -- available when event has `target` and `currentTarget`
- `ctrl`, `shift`, `alt`, `meta` -- available when event has modifier key booleans
- `left`, `right`, `middle` -- available when event has `button` property
- `exact` -- available for keyboard events with modifier keys
- `once` -- always available
- `passive`, `capture` -- available for DOM events on HTML elements

### `vModelModifiers<El, Prop>`

Computes valid `v-model` modifiers for a given element and prop type:

- `lazy` -- debounce input updates
- `number` -- cast input to number
- `trim` -- trim whitespace

### `vBindModifiers<El, Prop>`

Computes valid `v-bind` modifiers for a given element and prop type:

- `.prop` -- bind as DOM property
- `.attr` -- bind as HTML attribute
- `.camel` -- camelCase the attribute name

## Component Helpers

### `GetVueComponent<T>`

Extracts the instance type from a Vue component definition. Handles class-based components, functional components, and native HTML elements.

```ts
import type { GetVueComponent } from '@verter/types'

type Instance = GetVueComponent<typeof MyComponent>
// The component's instance type with $props, $emit, $slots, etc.
```

### `toComponentRender(is, components)`

Resolves a component from a components map or native element registry for dynamic component rendering. Provides compile-time rejection for invalid component names.

## Name Helpers

### `PascalToKebab<T>`

Converts PascalCase strings to kebab-case at the type level.

```ts
import type { PascalToKebab } from '@verter/types'

type Result = PascalToKebab<'MyComponent'>
// 'My-Component'
```

### `CamelToKebab<T>`

Converts camelCase strings to kebab-case at the type level.

## Loop Helpers

### `ExtractLoopsResult<T>`

Extracts key and value types from a `v-for` iterable (array or object).

```ts
import type { ExtractLoopsResult } from '@verter/types'

type ArrayLoop = ExtractLoopsResult<string[]>
// { key: number; value: string }

type ObjectLoop = ExtractLoopsResult<{ a: number; b: string }>
// { key: 'a'; value: number } | { key: 'b'; value: string }
```

### `extractLoops(iterable)`

Runtime function that extracts key/value pairs from a `v-for` iterable. Returns the typed key-value pair for use in template type checking.

## Setup / Macro Helpers

These types power the internal macro resolution pipeline (`defineProps`, `defineEmits`, `defineModel`, `defineSlots`, `defineExpose`, `defineOptions`).

### `createMacroReturn(macros)`

Creates a structured macro return object from the macro definitions. Used internally by the `MacrosPlugin` to produce typed component metadata.

### Key Types

| Type | Description |
|------|-------------|
| `CreateMacroReturn` | Constructs the full macro return structure |
| `NormaliseMacroReturn<T>` | Normalizes a macro return to a consistent shape |
| `NormalisedMacroReturn<T>` | The normalized macro return type |
| `ExtractMacroProps<T>` | Extracts props from a macro return |
| `ExtractPropsFromMacro<T>` | Resolves props including defaults |
| `MacroToEmitValue<T>` | Resolves emits to callable functions |
| `MacroToModelRecord<T>` | Resolves model definitions to a record |
| `SlotsToSlotType<T>` | Converts slot definitions to Vue's `SlotsType` |
| `MacroOptionsToOptions<T>` | Converts options macro to Vue options type |
| `ExtractExpose<T>` | Extracts exposed properties |
| `ExtractTemplateRef<T>` | Extracts template ref types |

## Instance Helpers

### `CreateTypedInternalInstanceFromNormalisedMacro<T, Attrs, DEV>`

Creates a fully typed component internal instance from a normalized macro return. This is the final type that powers `$props`, `$emit`, `$slots`, `$refs`, and `$exposed` on a component instance.

```ts
// The resulting type has:
// - props: resolved from defineProps + ModelToProps
// - emit: resolved from defineEmits + ModelToEmits
// - slots: resolved from defineSlots
// - refs: resolved from useTemplateRef
// - exposed: resolved from defineExpose
```

## Vue Overrides

### `shallowUnwrapRef(obj)`

Typed wrapper for Vue's `ShallowUnwrapRef` utility. Unwraps ref values one level deep.

### System Modifiers

The `SystemModifiers` type is re-exported, representing the set of system modifier keys: `"ctrl"`, `"shift"`, `"alt"`, `"meta"`.
