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
// component tags check their props through `ElementAttributesProperty
// { $props }` against the class-shaped component synth's `$props` member.
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

  // Component tags are class-shaped through the synth; the empty bound keeps
  // every projected component assignable as an element class.
  // eslint-disable-next-line @typescript-eslint/no-empty-object-type
  interface ElementClass {}

  // Component props are checked against the synth's `$props` member: a
  // `.svelte` component (and an imported `.vue` component) exposes `$props`
  // on its class-shaped synth, so the JSX prop bag checks against it.
  interface ElementAttributesProperty {
    $props: {};
  }

  // Intrinsic (lowercase) elements and their attributes are Svelte-true —
  // sourced from `svelte/elements`, never Vue's intrinsic table.
  interface IntrinsicElements extends SvelteHTMLElements {}
}
