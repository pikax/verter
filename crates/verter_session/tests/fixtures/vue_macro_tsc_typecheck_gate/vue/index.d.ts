/**
 * Minimal Vue declaration contract required by generated Verter TSC carriers.
 * This is deliberately structural and hermetic: it validates generated
 * TypeScript without depending on a workspace Vue installation.
 */
export type PublicProps = {
  key?: PropertyKey;
  ref?: unknown;
};

export interface HTMLAttributes {
  [name: string]: unknown;
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

export declare function defineComponent<const Options extends object>(
  options: Options,
): Options & { new (): {} };
