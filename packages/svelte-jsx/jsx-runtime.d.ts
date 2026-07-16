// The Verter-owned JSX namespace for the Svelte IDE TSX projection.
//
// A `.svelte.tsx` projection opens with `/** @jsxImportSource @verter/svelte-jsx */`,
// which directs TypeScript's automatic JSX runtime to consult the `JSX`
// namespace EXPORTED from this module — overriding the project-level
// `jsxImportSource: "vue"` for that file only.
//
// The namespace is Svelte-true: intrinsic elements and their attributes are
// typed by `svelte/elements`' `SvelteHTMLElements` (lowercase event
// attributes — `onclick`, `onchange`, `onintrostart` — NOT Vue's/React's
// camelCase table), an element evaluates to `ReturnType<Snippet>`, and
// native component tags are admitted through Svelte 5's callable `Component`
// contract and `LibraryManagedAttributes`; `$props` is retained only for the
// private class-shaped foreign-component adapter.
//
// Types-only: there is no runtime jsx factory here — the projection is never
// executed, only type-checked. This file is the SINGLE hand-written content
// authority; `verter_session` mirrors it in-crate and byte-pins the mirror.

import type { Snippet } from "svelte";
import type { SvelteHTMLElements } from "svelte/elements";

export namespace JSX {
  // A projected element evaluates to a rendered snippet result — the same
  // shape the `{@render}`/`Snippet` machinery produces.
  type Element = ReturnType<Snippet>;

  // Svelte 5 components are callable `(internals, props) => exports`, not
  // JSX functions whose first parameter is the prop bag. Admit that native
  // callable here and let `LibraryManagedAttributes` select its second
  // generic as the use-site props. Legacy/class-shaped components remain an
  // adapter-only input for mixed-framework templates; this namespace never
  // changes either component's public module type.
  type ElementType =
    | keyof IntrinsicElements
    | import("svelte").Component<any, any, any>
    | ((props: any) => Element)
    | (abstract new (...args: never[]) => ElementClass);

  type LibraryManagedAttributes<Component, FallbackProps> =
    Component extends import("svelte").Component<infer Props, any, any> ? Props : FallbackProps;

  // Empty element-instance bound for the private class-shaped adapter.
  // eslint-disable-next-line @typescript-eslint/no-empty-object-type
  interface ElementClass {}

  // Private fallback for a class-shaped foreign component. Native Svelte 5
  // Components use `LibraryManagedAttributes` above.
  interface ElementAttributesProperty {
    $props: {};
  }

  // Intrinsic (lowercase) elements and their attributes are Svelte-true —
  // sourced from `svelte/elements`, never Vue's intrinsic table.
  type IntrinsicElements = {
    [Name in keyof SvelteHTMLElements]: SvelteHTMLElements[Name];
  };
}
