// The Verter-owned MathML JSX namespace for the Svelte IDE TSX projection.
//
// A `.svelte` component declaring `<svelte:options namespace="mathml" />`
// interprets its WHOLE template in the MathML namespace. Verter projects such a
// component with the per-file pragma `/** @jsxImportSource @verter/svelte-jsx/mathml */`,
// which directs TypeScript's automatic JSX runtime to consult the `JSX`
// namespace EXPORTED from this module (its `/mathml/jsx-runtime` subpath).
//
// `svelte/elements` ships NO MathML types, so this is a Verter-owned v1 table:
// a CLOSED MathML tag set, each typed by a hand-written `MathMLAttributes`
// base (aria + global attributes) plus the official `DOMAttributes` event base.
// values are typed with `unknown`/structured primitives — never `any` — so the
// table remains a real (if permissive) contract. It REPLACES the HTML table
// (mathml-only; no catch-all intrinsic index), so an HTML element FAILS,
// proving the mathml table is in effect.
//
// Types-only. This file is the SINGLE hand-written content authority;
// `verter_session` mirrors it in-crate and byte-pins the mirror.

import type { Snippet } from "svelte";
import type { DOMAttributes } from "svelte/elements";

// A Verter-owned MathML attribute base. Svelte's official DOMAttributes owns
// lowercase events and their typed `currentTarget`; only MathML/global fields
// are maintained here.
interface MathMLAttributes extends DOMAttributes<MathMLElement> {
  children?: unknown;
  class?: string | undefined | null;
  id?: string | undefined | null;
  style?: string | undefined | null;
  dir?: "ltr" | "rtl" | undefined | null;
  displaystyle?: boolean | undefined | null;
  mathbackground?: string | undefined | null;
  mathcolor?: string | undefined | null;
  mathsize?: string | undefined | null;
  scriptlevel?: number | string | undefined | null;
  // ARIA + data-* pass-through.
  role?: string | undefined | null;
  [dataAttr: `data-${string}`]: unknown;
  [ariaAttr: `aria-${string}`]: unknown;
}

export namespace JSX {
  // A projected element evaluates to a rendered snippet result.
  type Element = ReturnType<Snippet>;

  type ElementType =
    | keyof IntrinsicElements
    | import("svelte").Component<any, any, any>
    | ((props: any) => Element)
    | (abstract new (...args: never[]) => ElementClass);

  type LibraryManagedAttributes<Component, FallbackProps> =
    Component extends import("svelte").Component<infer Props, any, any> ? Props : FallbackProps;

  // eslint-disable-next-line @typescript-eslint/no-empty-object-type
  interface ElementClass {}

  // Private fallback for class-shaped foreign components.
  interface ElementAttributesProperty {
    $props: {};
  }

  // The MathML intrinsic element set (closed, v1). Each MathML tag is typed by
  // the Verter-owned `MathMLAttributes` base. This REPLACES the HTML table.
  interface IntrinsicElements {
    math: MathMLAttributes;
    annotation: MathMLAttributes;
    "annotation-xml": MathMLAttributes;
    maction: MathMLAttributes;
    merror: MathMLAttributes;
    mfrac: MathMLAttributes;
    mi: MathMLAttributes;
    mmultiscripts: MathMLAttributes;
    mn: MathMLAttributes;
    mo: MathMLAttributes;
    mover: MathMLAttributes;
    mpadded: MathMLAttributes;
    mphantom: MathMLAttributes;
    mprescripts: MathMLAttributes;
    mroot: MathMLAttributes;
    mrow: MathMLAttributes;
    ms: MathMLAttributes;
    mspace: MathMLAttributes;
    msqrt: MathMLAttributes;
    mstyle: MathMLAttributes;
    msub: MathMLAttributes;
    msubsup: MathMLAttributes;
    msup: MathMLAttributes;
    mtable: MathMLAttributes;
    mtd: MathMLAttributes;
    mtext: MathMLAttributes;
    mtr: MathMLAttributes;
    munder: MathMLAttributes;
    munderover: MathMLAttributes;
    semantics: MathMLAttributes;
  }
}
