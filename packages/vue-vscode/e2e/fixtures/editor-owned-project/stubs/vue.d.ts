export type PublicProps = {};
export type HTMLAttributes = Record<string, unknown>;
export type ShallowUnwrapRef<T> = T;
export type NativeElements = Record<string, unknown>;
export type GlobalDirectives = Record<string, unknown>;
export type Directive<T = any, V = any, M extends string = string> = unknown;
export interface VNode {}
export declare const Comment: unique symbol;
export declare const Fragment: unique symbol;
export type Ref<T> = { value: T };
export type ExtractPropTypes<T> = T;
export declare function defineComponent<T extends object>(options: T): T;

declare global {
  namespace JSX {
    interface Element extends VNode {}
    interface ElementClass {
      $props: {};
    }
    interface ElementAttributesProperty {
      $props: {};
    }
    interface IntrinsicElements {
      [name: string]: Record<string, unknown>;
    }
  }
}
