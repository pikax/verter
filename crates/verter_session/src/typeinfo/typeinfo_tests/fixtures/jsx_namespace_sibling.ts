// @ai-generated - JSX global-namespace sibling-resolution regression fixture.
//
// A namespace member type (`IntrinsicElements.div`) references a
// namespace-LOCAL sibling alias (`Common`) declared in the SAME
// `declare global { namespace JSX { ... } }` block. Resolving
// `JSX.IntrinsicElements["div"]` must dereference the unqualified
// `Common` reference through the global-augmentation sibling scope
// `(Global, "JSX.Common")` — parity with the file-scope
// `declare namespace JSX` sibling-binding path.

export {};

declare global {
  namespace JSX {
    type Common = { id?: string };
    interface IntrinsicElements {
      div: Common;
    }
  }
}

export type DivIntrinsic = JSX.IntrinsicElements["div"];
