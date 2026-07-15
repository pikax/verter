// Vendored fixture source bytes for the U2.JSX-era oracle lift rows
// (`jsx.ts`), split out of `oracle_query_specs_vendored_sources.rs` to keep
// each vendored-sources file under the production line-size guard.
// `include!`'d by `oracle_query_specs.rs` immediately after the
// module-augmentation vendored-sources file (the registry is the source-byte
// authority; the guard `inlined_registry_source_is_byte_identical_to_fixture_files`
// pins the const byte-identical to its on-disk `fixtures/jsx.ts` sibling).

/// Vendored source bytes of `/fixtures/jsx.ts` (the registry is the source-byte
/// authority). Inlined verbatim (PURE owned `&'static str`); the guard
/// `inlined_registry_source_is_byte_identical_to_fixture_files` asserts
/// byte-identity with `fixtures/jsx.ts`.
#[allow(dead_code)]
pub(crate) const JSX_SOURCE: &str = r#"// @ai-generated - Synthetic JSX-namespace typeinfo fixture.
//
// JSX is parsed as type-syntax-only here. We never write `<div />` tags
// because the file is a `.ts` (not `.tsx`) — instead, every JSX-shaped
// surface is encoded as a normal TypeScript type that references the
// `JSX.IntrinsicElements` and `JSX.Element` namespace members. This
// covers the resolver paths used by component factories, FC equivalents,
// and parametric intrinsic-element lookup, without depending on JSX
// emit modes.

export {};

declare global {
  namespace JSX {
    interface IntrinsicElements {
      div: { id?: string; className?: string };
      span: { title?: string };
    }
    interface Element {
      __element_brand__: true;
    }
  }
}

// 1) JSX.IntrinsicElements lookup — `JSX.IntrinsicElements["div"]` is the
//    declared shape `{ id?: string; className?: string }`.
export type DivIntrinsic = JSX.IntrinsicElements["div"];
export type SpanIntrinsic = JSX.IntrinsicElements["span"];

// 2) Component-prop inference through a typed factory. The factory is
//    declared (no body) — Verter resolves the generic shape via the
//    `props: P` parameter that ties P to the component's props.
export declare function createElement<P>(
  component: (props: P) => JSX.Element,
  props: P,
): JSX.Element;

export function MyComponent(props: { label: string }): JSX.Element {
  void props;
  return { __element_brand__: true } as JSX.Element;
}

// `createElement(MyComponent, ...)` — TS7 infers P = `{ label: string }`.
// We expose the inferred parameter type via `Parameters<...>[1]`.
export type CreateElementForMyComponent = typeof createElement<{ label: string }>;
export type InferredPropsForMyComponent = Parameters<typeof createElement<{ label: string }>>[1];

// 3) `React.FC<P>` equivalent — adds `children?: unknown` to every props
//    shape, returns `JSX.Element`. Standard React FC contract.
export type FC<P> = (props: P & { children?: unknown }) => JSX.Element;

export type LabelFC = FC<{ label: string }>;
export type LabelFCProps = Parameters<LabelFC>[0];

// 4) Generic intrinsic-element lookup — `JSX.IntrinsicElements[Tag]`
//    parameterised on `Tag extends keyof JSX.IntrinsicElements`. This
//    is a parametric Pick — Verter must preserve the `Tag` parameter
//    until the call site supplies it.
export type IntrinsicPropsFor<Tag extends keyof JSX.IntrinsicElements> = JSX.IntrinsicElements[Tag];
export type DivPropsViaIndex = IntrinsicPropsFor<"div">;
export type SpanPropsViaIndex = IntrinsicPropsFor<"span">;

// Aliases the Rust test resolves by name. Kept separate so each contract
// is independently checked.
export type IntrinsicKeys = keyof JSX.IntrinsicElements;

// 5) A second `declare global { namespace JSX { interface IntrinsicElements {...}}}`
//    block. TS7 declaration-merging unions every member from every block —
//    the resulting `IntrinsicElements` interface surfaces `div`, `span`,
//    AND `customCard` together.
declare global {
  namespace JSX {
    interface IntrinsicElements {
      customCard: { variant?: "primary" | "secondary" };
    }
  }
}

export type CustomCardIntrinsic = JSX.IntrinsicElements["customCard"];

// 6) `JSX.Element` directly projected — the resolved shape must match the
//    declared interface members `{ __element_brand__: true }`.
export type ElementShape = JSX.Element;
"#;
