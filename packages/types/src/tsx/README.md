# TSX Type Augmentations

This folder contains type-only augmentations that improve TypeScript/JSX (TSX) developer experience for Vue 3 inside this monorepo.

What you get:
- `v-slot` on Vue components: The callback receives the component instance so you can access `$slots` and instance properties with full inference, and optionally return a typed slots object.
- `v-slot` on native elements: The callback receives the correct `HTMLElement` subtype (e.g., `HTMLInputElement` for `input`).
- `onVue:*` lifecycle attributes: Type-safe, IDE-friendly attributes for common Vue lifecycle hooks that expose a typed VNode with `ctx.proxy` narrowed to the component instance.

These are type augmentations only; they do not attach runtime listeners.

## Files
- `tsx.tsx`: Declares global JSX augmentations:
  - `v-slot` on `JSX.IntrinsicClassAttributes<T>` for components
  - `onVue:*` lifecycle attributes (e.g., `onVue:mounted`, `onVue:before-update`, etc.)
  - Imports generated HTML attributes mapping from `tsx.attributes.ts`
- `tsx.attributes.ts` (generated): Adds `v-slot` to all HTML*Attributes interfaces with correct element instance types.
- `tsx.spec.tsx`: Type-check tests validating the augmentations (no runtime execution).

## Usage (in tests/examples)
Make sure the TSX types are loaded in the file that uses them:

```tsx
import "./tsx";

const Comp = defineComponent({
  slots: {} as SlotsType<{
    default: (p: { msg: string }) => any;
  }>,
  setup() {
    return { id: "comp-1" };
  },
});

// v-slot on components: callback receives the instance, may return slots
<Comp
  v-slot={(c) => {
    // c is the component instance (inferred)
    c.$slots.default({ msg: "hello" });
    return { default: ({ msg }: { msg: string }) => <div>{msg}</div> };
  }}
/>

// v-slot on elements: callback receives the correct element instance
<input v-slot={(el) => { /* el: HTMLInputElement */ el.value; }} />

// Lifecycle attributes: typed VNode, instance available via ctx.proxy
<Comp
  onVue:mounted={(vnode) => {
    vnode.ctx.proxy.id; // string
  }}
  onVue:before-update={(current, old) => {
    current.ctx.proxy.id;
    old.ctx.proxy.id;
  }}
/>
```

## Regenerating HTML attribute mappings
`tsx.attributes.ts` is generated from upstream `.d.ts` files to stay in sync with DOM types.

- Generator script: `packages/types/scripts/generate-tsx-attributes.js`
- It reads:
  - `@vue/runtime-dom/dist/runtime-dom.d.ts` to discover `HTML*Attributes` interfaces
  - `typescript/lib/lib.dom.d.ts` to resolve `HTML*Element` types
- It handles pnpm monorepo virtual store paths and irregular name mappings automatically.

Run the generator from the repo root or package directory:

```powershell
node packages/types/scripts/generate-tsx-attributes.js
```

When to regenerate:
- After upgrading Vue or TypeScript versions
- When upstream DOM or runtime-dom typings change

## Testing
All tests are type-check only (no runtime). From `packages/types`:

```powershell
pnpm test
```

Notes:
- `tsconfig.test.json` enables TSX (`jsx: "preserve"`).
- Tests assert inference via `assertType` and intentionally include `@ts-expect-error` cases.

## Limitations
- `onVue:*` attributes are type-only helpers; they do not register runtime lifecycle hooks. Use them for editor hints and compile-time checks.
- JSX attribute name transformations at runtime do not affect these augmentations since they are only consumed by the TypeScript type system.
