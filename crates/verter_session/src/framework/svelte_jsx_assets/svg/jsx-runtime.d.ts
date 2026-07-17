// The Verter-owned SVG JSX namespace for the Svelte IDE TSX projection.
//
// A `.svelte` component declaring `<svelte:options namespace="svg" />`
// interprets its WHOLE template in the SVG namespace. Verter projects such a
// component with the per-file pragma `/** @jsxImportSource @verter/svelte-jsx/svg */`,
// which directs TypeScript's automatic JSX runtime to consult the `JSX`
// namespace EXPORTED from this module (its `/svg/jsx-runtime` subpath) —
// overriding the project-level `jsxImportSource` for that file only.
//
// The namespace is SVG-ONLY: `IntrinsicElements` derives from the official
// SVG-keyed subset of `svelte/elements`' `SvelteHTMLElements`, adapting only
// the implicit JSX child channel documented below. It REPLACES the HTML table — an
// HTML-only attribute on an svg element FAILS, proving the svg table is in
// effect (there is no catch-all intrinsic index). `Element` /
// component adaptation is namespace-invariant.
//
// Types-only: there is no runtime jsx factory here. This file is the SINGLE
// hand-written content authority; `verter_session` mirrors it in-crate and
// byte-pins the mirror.

import type { Snippet } from "svelte";
import type { SvelteHTMLElements } from "svelte/elements";

// Svelte 5 publishes its SVG tag contracts as the SVG-keyed portion of
// `SvelteHTMLElements`. Keep the namespace closed while inheriting every
// authored attribute, event, and `currentTarget` detail from the official package.
type SvelteSVGElementNames =
  | "svg"
  | "a"
  | "animate"
  | "animateMotion"
  | "animateTransform"
  | "circle"
  | "clipPath"
  | "defs"
  | "desc"
  | "ellipse"
  | "feBlend"
  | "feColorMatrix"
  | "feComponentTransfer"
  | "feComposite"
  | "feConvolveMatrix"
  | "feDiffuseLighting"
  | "feDisplacementMap"
  | "feDistantLight"
  | "feDropShadow"
  | "feFlood"
  | "feFuncA"
  | "feFuncB"
  | "feFuncG"
  | "feFuncR"
  | "feGaussianBlur"
  | "feImage"
  | "feMerge"
  | "feMergeNode"
  | "feMorphology"
  | "feOffset"
  | "fePointLight"
  | "feSpecularLighting"
  | "feSpotLight"
  | "feTile"
  | "feTurbulence"
  | "filter"
  | "foreignObject"
  | "g"
  | "image"
  | "line"
  | "linearGradient"
  | "marker"
  | "mask"
  | "metadata"
  | "mpath"
  | "path"
  | "pattern"
  | "polygon"
  | "polyline"
  | "radialGradient"
  | "rect"
  | "stop"
  | "switch"
  | "symbol"
  | "text"
  | "textPath"
  | "tspan"
  | "use"
  | "view";

export namespace JSX {
  // A projected element evaluates to a rendered snippet result — the same
  // shape the `{@render}`/`Snippet` machinery produces.
  type Element = ReturnType<Snippet>;

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

  // Private fallback for class-shaped foreign components.
  interface ElementAttributesProperty {
    $props: {};
  }

  // The official SVG-keyed intrinsic subset. This REPLACES the HTML table.
  // Only the implicit JSX child channel is adapted: Svelte markup children
  // are values/elements, not the explicit forwarded Snippet prop represented
  // by `DOMAttributes.children`.
  type IntrinsicElements = {
    [Name in SvelteSVGElementNames]: Omit<SvelteHTMLElements[Name], "children"> & {
      children?: unknown;
    };
  };
}
