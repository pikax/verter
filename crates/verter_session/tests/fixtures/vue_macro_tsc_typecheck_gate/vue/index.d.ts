/**
 * Minimal Vue declaration contract required by generated Verter TSC carriers.
 * This is deliberately structural and hermetic: it validates generated
 * TypeScript without depending on a workspace Vue installation.
 */
export type PublicProps = {
  key?: PropertyKey;
  ref?: unknown;
};

/**
 * CLOSED member set, mirroring the real `@vue/runtime-dom` shape.
 *
 * This must NOT carry an index signature. Real `HTMLAttributes` has neither an
 * index signature nor a `data-*` member, and the whole point of the widened
 * parent-facing props surface is that an attribute NO element accepts stays an
 * error. A `[name: string]: unknown` here accepts everything, so this gate
 * would structurally be unable to observe that rejection — it would type-check
 * a carrier that had degraded into an open surface exactly as happily as a
 * correct one.
 */
export interface HTMLAttributes {
  class?: unknown;
  style?: unknown;
  id?: string;
  title?: string;
  tabindex?: number;
  onClick?: (event: unknown) => void;
}

/** Anchor-only members, to keep the map tag-discriminating. */
export interface AnchorHTMLAttributes extends HTMLAttributes {
  href?: string;
}

/**
 * The tag -> attribute-surface map generated carriers index to project a
 * component's resolved attribute-fallthrough surface onto its parent-facing
 * props type. Vue publishes this from `@vue/runtime-dom`; the carrier's
 * `__Verter_RootElementAttrs<Tag>` helper degrades to `{}` for a tag that is
 * not a key, so this hermetic map only needs the tags the gate's own fixtures
 * render.
 */
export interface IntrinsicElementAttributes {
  a: AnchorHTMLAttributes;
  div: HTMLAttributes;
  span: HTMLAttributes;
}

export interface Ref<T = unknown> {
  value: T;
}

export type ShallowUnwrapRef<T> = {
  [K in keyof T]: T[K] extends Ref<infer Value> ? Value : T[K];
};

export type ExtractPropTypes<RuntimeProps extends Record<string, unknown>> = {
  [K in keyof RuntimeProps]?: unknown;
};

/**
 * The instance carries `$props` typed as `PublicProps`, mirroring the real
 * `DefineComponent` shape closely enough for the generated STUB carriers
 * (Options-API / scriptless / declaration-empty) to be checkable here: those
 * stubs preserve the component's own surface through
 * `InstanceType<C>["$props"]` and subtract `keyof` of it from the inherited
 * element arm. Without a `$props` member the subtraction operand would not
 * resolve and the stub carriers could not be gated at all.
 */
export declare function defineComponent<const Options extends object>(
  options: Options,
): Options & { new (): { $props: PublicProps } };
