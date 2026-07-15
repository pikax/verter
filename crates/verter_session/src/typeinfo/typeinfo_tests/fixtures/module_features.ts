// @ai-generated - Synthetic module-feature typeinfo fixture.
//
// Covers:
//   * Single-level `namespace Geometry { ... }` member resolution.
//   * Nested `namespace A.B.C { ... }` deep member resolution.
//   * `declare global { interface GlobalContract { ... } }` augmenting a
//     locally-declared global interface.
//
// Module augmentation (`declare module "./..."`), `typeof import("./...")`,
// and `export = ` ambient module interop live in companion fixtures
// because they need separate canonical files.

// Force this file to be a module so `declare global` is valid.
export {};

declare global {
  // A locally-declared global contract that this file augments.
  // Tests resolve `GlobalContract` from any file in the project — the
  // augmented surface includes properties contributed here.
  interface GlobalContract {
    coreId: string;
  }
}

declare global {
  interface GlobalContract {
    coreFlag: boolean;
  }
}

// Single-level namespace. `Geometry.Point` is the canonical example.
export namespace Geometry {
  export type Point = { x: number; y: number };
  export type Vector = Point;
}

// Nested namespace `A.B.C` — deep member resolution must walk the chain.
export namespace Layer {
  export namespace Inner {
    export namespace Leaf {
      export type Value = { tag: "leaf"; depth: number };
    }
  }
}

// Aliases that consumers will resolve through. These exercise the
// "resolve by alias name" path against namespace-qualified definitions.
export type GeometryPoint = Geometry.Point;
export type GeometryVector = Geometry.Vector;
export type LeafValue = Layer.Inner.Leaf.Value;

// `GlobalContractAlias` projects the global interface via a local alias
// so the test can request it by a stable resolver-symbol name.
export type GlobalContractAlias = GlobalContract;

// Namespace + interface name-merging. `Connector` is BOTH:
//   * an interface (the type) → `{ id: string }`
//   * a namespace (the value/type container) exposing `Kind` and `VERSION`
// In TS7 the two declarations merge: `Connector` as a type refers to the
// interface shape, while `Connector.Kind` / `Connector.VERSION` reach into
// the namespace.
export interface Connector {
  id: string;
}
export namespace Connector {
  export type Kind = "internal" | "external";
  export const VERSION = "1.0" as const;
}

// Aliases the resolver requests by name.
export type ConnectorShape = Connector;
export type ConnectorKind = Connector.Kind;
export type ConnectorVersion = typeof Connector.VERSION;
