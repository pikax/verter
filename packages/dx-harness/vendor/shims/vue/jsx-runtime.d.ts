// Minimal JSX runtime namespace so a generated IDE artifact compiled with
// `jsxImportSource: "vue"` resolves its JSX surface under the hermetic baseline.
// Permissive on purpose: the differential validates type identity, not the
// shim's own JSX strictness.

export namespace JSX {
  interface Element {}
  interface ElementClass {
    $props: Record<string, unknown>;
  }
  interface ElementChildrenAttribute {
    $slots: Record<string, unknown>;
  }
  interface IntrinsicElements {
    [name: string]: Record<string, unknown>;
  }
}

export {};
