// Curated semantic oracle - fallthrough attrs.
//
// The intended Vue semantics of fallthrough attrs: `$attrs` carries the inherited
// HTML attributes a component does NOT declare as props - here an `id` and a native
// `onClick` handler - which fall through onto the single root element. The native
// `onClick` parameter must stay a DOM `MouseEvent`.
//
// Each anchor's query target is the LAST identifier on its line.

interface FallthroughAttrs {
  id?: string;
  onClick?: (event: MouseEvent) => void;
}

declare const attrs: FallthroughAttrs;

const rootId = attrs.id; // @dx-anchor attrs.id
const rootClick = attrs.onClick; // @dx-anchor attrs.onClick

export { rootClick, rootId };
