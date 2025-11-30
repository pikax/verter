# Slots - Strict Type Checking for Vue Slot Content

Type-safe helpers for rendering Vue slots with strict type validation based on [RFC 733](https://github.com/vuejs/rfcs/discussions/733).

## Overview

`StrictRenderSlot` provides compile-time validation that slot content matches the expected return type. It enforces:
- **Array slots**: Accept arrays of the expected component/type
- **Single slots**: Require single-element tuples (wraps single values)
- **Non-empty slots**: Enforce at least one element using tuple types
- **Tuple slots**: Exact length and type matching for fixed-size content
- **Literal types**: Support for string literals and unions
- **HTML elements**: Strict type discrimination between element types

## API

```ts
import { StrictRenderSlot } from '@verter/types/slots';

declare function StrictRenderSlot<T, U>(
  slot: T,
  children: /* inferred based on slot return type */
): any;
```

The function has two overloads:
1. **Single-value overload**: For non-array slot return types, wraps the value in a tuple
2. **Array/tuple overload**: For array or tuple return types, validates element types

## Usage Examples

### Basic Array Slots

For slots returning arrays of components:

```ts
import { defineComponent, SlotsType } from 'vue';
import { StrictRenderSlot } from '@verter/types/slots';

const TabItem = defineComponent({
  props: { id: { type: String, required: true } }
});

const Tabs = defineComponent({
  slots: {} as SlotsType<{
    default: () => (typeof TabItem)[];
  }>
});

const tabs = new Tabs();

// ✅ Valid: array of TabItem
StrictRenderSlot(tabs.$slots.default, [TabItem, TabItem]);

// ✅ Valid: empty array
StrictRenderSlot(tabs.$slots.default, []);

// ✅ Valid: single item
StrictRenderSlot(tabs.$slots.default, [TabItem]);

// ❌ Error: wrong component type
StrictRenderSlot(tabs.$slots.default, [OtherComponent]);
```

### Single-Value Slots

For slots returning a single value (not an array):

```ts
const Panel = defineComponent({
  slots: {} as SlotsType<{
    header: () => typeof HeaderComponent;
  }>
});

const panel = new Panel();

// ✅ Valid: single element in tuple
StrictRenderSlot(panel.$slots.header, [HeaderComponent]);

// ❌ Error: multiple elements not allowed
StrictRenderSlot(panel.$slots.header, [HeaderComponent, HeaderComponent]);
```

### Non-Empty Slots (Advanced Pattern)

Use TypeScript tuple types to enforce at least one element:

```ts
const List = defineComponent({
  slots: {} as SlotsType<{
    // [Item, ...Item[]] means "at least one Item"
    items: () => [typeof ListItem, ...Array<typeof ListItem>];
  }>
});

const list = new List();

// ✅ Valid: one or more items
StrictRenderSlot(list.$slots.items, [ListItem]);
StrictRenderSlot(list.$slots.items, [ListItem, ListItem, ListItem]);

// ❌ Error: empty array not allowed
StrictRenderSlot(list.$slots.items, []);
```

**Why this pattern is useful:**
- Enforces business logic at compile time (e.g., "a list must have items")
- Prevents runtime errors from empty arrays
- Documents intent in the type signature
- Works with existing TypeScript tuple syntax

### Tuple Slots (Fixed Length)

For slots requiring exact number and types of elements:

```ts
const Dialog = defineComponent({
  slots: {} as SlotsType<{
    actions: () => [typeof CancelButton, typeof ConfirmButton];
  }>
});

const dialog = new Dialog();

// ✅ Valid: exactly two buttons in order
StrictRenderSlot(dialog.$slots.actions, [CancelButton, ConfirmButton]);

// ❌ Error: wrong number of elements
StrictRenderSlot(dialog.$slots.actions, [CancelButton]);

// ❌ Error: wrong order
StrictRenderSlot(dialog.$slots.actions, [ConfirmButton, CancelButton]);
```

### Literal Type Slots

For slots returning string literals:

```ts
const Badge = defineComponent({
  slots: {} as SlotsType<{
    status: () => "success" | "error" | "warning";
  }>
});

const badge = new Badge();

// ✅ Valid: literal value in tuple
StrictRenderSlot(badge.$slots.status, ["success"]);

// ❌ Error: wrong literal
StrictRenderSlot(badge.$slots.status, ["invalid"]);
```

### Literal Arrays (With Limitation)

For slots returning arrays of literals:

```ts
const Tags = defineComponent({
  slots: {} as SlotsType<{
    categories: () => ("tech" | "design" | "marketing")[];
  }>
});

const tags = new Tags();

// ✅ Valid: array of valid literals
StrictRenderSlot(tags.$slots.categories, ["tech", "design"]);
StrictRenderSlot(tags.$slots.categories, []);

// ⚠️ Limitation: Cannot reject wrong literals without `as const`
// TypeScript widens ["invalid"] to string[] before type checking
StrictRenderSlot(tags.$slots.categories, ["invalid"]); // Accepted (type widening)

// ✅ Use `as const` for strict literal checking
// @ts-expect-error wrong literal
StrictRenderSlot(tags.$slots.categories, ["invalid" as const]);
```

**Limitation explanation:**
- TypeScript automatically widens array literals: `["foo"]` becomes `string[]`
- The type system cannot distinguish `["foo"]` from `["bar"]` after widening
- For strict literal validation, use `as const` or define the slot with a tuple type instead

### HTML Element Slots

Strict discrimination between different HTML element types:

```ts
const Container = defineComponent({
  slots: {} as SlotsType<{
    icon: () => HTMLSpanElement;
    icons: () => HTMLSpanElement[];
  }>
});

const container = new Container();

// ✅ Valid: correct element type
StrictRenderSlot(container.$slots.icon, [
  document.createElement("span") as HTMLSpanElement
]);

// ❌ Error: wrong element type (despite structural compatibility)
StrictRenderSlot(container.$slots.icon, [
  document.createElement("div") as HTMLDivElement
]);

// ✅ Valid: array of correct elements
StrictRenderSlot(container.$slots.icons, [
  document.createElement("span") as HTMLSpanElement
]);

// ❌ Error: array of wrong elements
StrictRenderSlot(container.$slots.icons, [
  document.createElement("div") as HTMLDivElement
]);
```

**Note:** HTMLElement discrimination uses exact type equality checking to prevent structural typing issues where `HTMLDivElement` would otherwise be compatible with `HTMLSpanElement`.

### Mixed Types

Combine different types in slot definitions:

```ts
const Card = defineComponent({
  slots: {} as SlotsType<{
    // Union of string and component
    content: () => string | typeof TextBlock;
    
    // Tuple of literal and component
    header: () => ["title", typeof IconComponent];
  }>
});

const card = new Card();

// ✅ Valid: string
StrictRenderSlot(card.$slots.content, ["Hello"]);

// ✅ Valid: component
StrictRenderSlot(card.$slots.content, [TextBlock]);

// ✅ Valid: literal + component tuple
StrictRenderSlot(card.$slots.header, ["title", IconComponent]);
```

## Type Checking Behavior

### Primitive Types (string, number, boolean, etc.)
- **Check**: Expected type extends provided type
- **Behavior**: Allows type widening (e.g., `"foo"` → `string`)
- **Trade-off**: Cannot reject wrong literals without `as const`

### Object Types (Components, HTMLElements, etc.)
- **Check**: Exact type equality using `IsExactlyEqual` helper
- **Behavior**: Strict discrimination, no structural compatibility
- **Benefit**: Prevents accidental type mismatches

### Empty Arrays
- **Special case**: Always accepted for array slots
- **Type**: Inferred as `never[]` by TypeScript
- **Reason**: Practically useful for conditional rendering

### Tuples
- **Check**: Exact type and length matching
- **Behavior**: Must match signature precisely
- **Use case**: Fixed-size content with specific types

## Implementation Details

The `StrictRenderSlot` function uses TypeScript's conditional types and generic inference:

1. **First Overload** (Single Values):
   - Filters out array types (returns `never`)
   - Wraps single values in tuples
   - Captures exact type via generic parameter `U`

2. **Second Overload** (Arrays & Tuples):
   - Detects tuple types: `R extends readonly [any, ...any[]]`
   - Handles empty arrays: `[UE] extends [never]`
   - Primitive arrays: Covariant check `E extends UE`
   - Object arrays: Exact equality check `IsExactlyEqual<UE, E>`

The `IsExactlyEqual` type helper ensures true type equality:

```ts
type IsExactlyEqual<A, B> = 
  (<T>() => T extends A ? 1 : 2) extends <T>() => T extends B ? 1 : 2
    ? true
    : false;
```

This prevents structural typing issues where TypeScript would consider `HTMLDivElement` assignable to `HTMLSpanElement`.

## Best Practices

### Use Tuples for Fixed Content

When slot content has a specific structure:

```ts
// ✅ Good: Explicit tuple type
slots: {} as SlotsType<{
  toolbar: () => [typeof BackButton, typeof Title, typeof MenuButton];
}>

// ❌ Less clear: Array type
slots: {} as SlotsType<{
  toolbar: () => (typeof BackButton | typeof Title | typeof MenuButton)[];
}>
```

### Use Non-Empty Tuples for Required Content

When at least one element is mandatory:

```ts
// ✅ Good: Non-empty tuple enforces presence
slots: {} as SlotsType<{
  items: () => [typeof Item, ...Array<typeof Item>];
}>

// ⚠️ Less safe: Empty array allowed
slots: {} as SlotsType<{
  items: () => (typeof Item)[];
}>
```

### Use `as const` for Literal Validation

When you need strict literal checking in arrays:

```ts
// With as const
const tags = ["tag1", "tag2"] as const;
StrictRenderSlot(slot, [...tags]); // Preserves literal types

// Without as const
StrictRenderSlot(slot, ["tag1", "tag2"]); // Widened to string[]
```

### Document Limitations

When using literal arrays without `as const`, document the widening behavior:

```ts
// Note: Without `as const`, wrong literals won't be caught at compile time
slots: {} as SlotsType<{
  tags: () => ("tag1" | "tag2" | "tag3")[];
}>
```

## Testing

The slots module includes comprehensive type tests covering:

- Array slots with components
- Single-value slots requiring tuples
- Non-empty slots (non-conventional pattern)
- Tuple slots with exact matching
- String and string union slots
- Literal and literal union slots
- Literal arrays (with widening limitations)
- Mixed literal and component tuples
- HTML element single and array slots
- Type discrimination (rejecting wrong types)

Run tests:

```sh
pnpm test
```

## Related

- [Vue RFC 733 - Strict Slot Rendering](https://github.com/vuejs/rfcs/discussions/733)
- [Vue SlotsType documentation](https://vuejs.org/api/utility-types.html#slotstype)
- TypeScript Handbook: [Conditional Types](https://www.typescriptlang.org/docs/handbook/2/conditional-types.html)
- TypeScript Handbook: [Tuple Types](https://www.typescriptlang.org/docs/handbook/2/objects.html#tuple-types)
