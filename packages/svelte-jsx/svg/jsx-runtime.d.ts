// The Verter-owned SVG JSX namespace for the Svelte IDE TSX projection.
//
// A `.svelte` component declaring `<svelte:options namespace="svg" />`
// interprets its WHOLE template in the SVG namespace. Verter projects such a
// component with the per-file pragma `/** @jsxImportSource @verter/svelte-jsx/svg */`,
// which directs TypeScript's automatic JSX runtime to consult the `JSX`
// namespace EXPORTED from this module (its `/svg/jsx-runtime` subpath) —
// overriding the project-level `jsxImportSource` for that file only (D-ae(a)).
//
// The namespace is SVG-ONLY: `IntrinsicElements` is the SVG element set, each
// typed by `svelte/elements`' exported `SVGAttributes` against the matching DOM
// instance type. It REPLACES the HTML table — an HTML-only element or an
// HTML-only attribute on an svg element FAILS, proving the svg table is in
// effect (there is no catch-all intrinsic index). `Element` /
// `ElementAttributesProperty` / `ElementClass` are namespace-invariant (the
// snippet-result element shape + the component `$props` contract).
//
// Types-only: there is no runtime jsx factory here. This file is the SINGLE
// hand-written content authority; `verter_session` mirrors it in-crate and
// byte-pins the mirror.

import type { Snippet } from "svelte";
import type { SVGAttributes } from "svelte/elements";

export namespace JSX {
  // A projected element evaluates to a rendered snippet result — the same
  // shape the `{@render}`/`Snippet` machinery produces.
  type Element = ReturnType<Snippet>;

  // Component tags are class-shaped through the synth; the empty bound keeps
  // every projected component assignable as an element class.
  // eslint-disable-next-line @typescript-eslint/no-empty-object-type
  interface ElementClass {}

  // Component props are checked against the synth's `$props` member.
  interface ElementAttributesProperty {
    $props: {};
  }

  // The SVG intrinsic element set — Svelte-true attributes from
  // `svelte/elements`' `SVGAttributes`, keyed by the SVG tag names against
  // their DOM instance types. This REPLACES the HTML table (svg-only).
  interface IntrinsicElements {
    svg: SVGAttributes<SVGSVGElement>;
    a: SVGAttributes<SVGAElement>;
    circle: SVGAttributes<SVGCircleElement>;
    clipPath: SVGAttributes<SVGClipPathElement>;
    defs: SVGAttributes<SVGDefsElement>;
    desc: SVGAttributes<SVGDescElement>;
    ellipse: SVGAttributes<SVGEllipseElement>;
    feBlend: SVGAttributes<SVGFEBlendElement>;
    feColorMatrix: SVGAttributes<SVGFEColorMatrixElement>;
    feComponentTransfer: SVGAttributes<SVGFEComponentTransferElement>;
    feComposite: SVGAttributes<SVGFECompositeElement>;
    feConvolveMatrix: SVGAttributes<SVGFEConvolveMatrixElement>;
    feDiffuseLighting: SVGAttributes<SVGFEDiffuseLightingElement>;
    feDisplacementMap: SVGAttributes<SVGFEDisplacementMapElement>;
    feFlood: SVGAttributes<SVGFEFloodElement>;
    feGaussianBlur: SVGAttributes<SVGFEGaussianBlurElement>;
    feImage: SVGAttributes<SVGFEImageElement>;
    feMerge: SVGAttributes<SVGFEMergeElement>;
    feMorphology: SVGAttributes<SVGFEMorphologyElement>;
    feOffset: SVGAttributes<SVGFEOffsetElement>;
    feTile: SVGAttributes<SVGFETileElement>;
    feTurbulence: SVGAttributes<SVGFETurbulenceElement>;
    filter: SVGAttributes<SVGFilterElement>;
    foreignObject: SVGAttributes<SVGForeignObjectElement>;
    g: SVGAttributes<SVGGElement>;
    image: SVGAttributes<SVGImageElement>;
    line: SVGAttributes<SVGLineElement>;
    linearGradient: SVGAttributes<SVGLinearGradientElement>;
    marker: SVGAttributes<SVGMarkerElement>;
    mask: SVGAttributes<SVGMaskElement>;
    path: SVGAttributes<SVGPathElement>;
    pattern: SVGAttributes<SVGPatternElement>;
    polygon: SVGAttributes<SVGPolygonElement>;
    polyline: SVGAttributes<SVGPolylineElement>;
    radialGradient: SVGAttributes<SVGRadialGradientElement>;
    rect: SVGAttributes<SVGRectElement>;
    stop: SVGAttributes<SVGStopElement>;
    symbol: SVGAttributes<SVGSymbolElement>;
    text: SVGAttributes<SVGTextElement>;
    textPath: SVGAttributes<SVGTextPathElement>;
    tspan: SVGAttributes<SVGTSpanElement>;
    use: SVGAttributes<SVGUseElement>;
  }
}
