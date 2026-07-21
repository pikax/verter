// Minimal @verter/types stub for TSGO resolution.
// Synced as a virtual file when the project doesn't have @verter/types installed.
// Contains only the exports referenced by generated TSX.
// Keep in sync with: packages/typescript-plugin/src/helpers/verterTypesStub.ts

import type {
  ShallowUnwrapRef,
  ComponentObjectPropsOptions,
  EmitsOptions,
  ComponentTypeEmits,
  PropType,
  ComponentOptionsMixin,
  ComputedOptions,
  MethodOptions,
  ComponentOptionsBase,
  Comment,
  Directive,
  Fragment,
  GlobalComponents,
  GlobalDirectives,
  HTMLAttributes,
  NativeElements,
} from "vue";

// ── Core types ──────────────────────────────────────────────────

// setup/setup.ts
export type Prettify<T> = T extends { (...args: any[]): any } ? T : { [K in keyof T]: T[K] } & {};
export declare function createMacroReturn<T>(): T;

// helpers/helpers.ts
export type OmitConstructorSignature<T> = { [K in keyof T]: T[K] };

// IDE template helpers
export declare function globalComponentsNav(): GlobalComponents;
export type GlobalComponentType<N> = N extends keyof GlobalComponents
  ? GlobalComponents[N]
  : unknown;
export type GlobalComponentKebabType<N, K extends string> = N extends keyof GlobalComponents
  ? GlobalComponents[N]
  : K extends keyof GlobalComponents
    ? GlobalComponents[K]
    : K extends keyof import("vue/jsx-runtime").JSX.IntrinsicElements
      ? (props: import("vue/jsx-runtime").JSX.IntrinsicElements[K]) => any
      : (props: Record<string, any>) => any;
export type ExtractRenderComponent<T> = T extends { new (...args: any[]): infer I }
  ? I extends { $props: any }
    ? T
    : I extends HTMLElement
      ? (props: {}) => I
      : I
  : T extends (...args: any) => infer R
    ? void extends R
      ? typeof Comment
      : R extends Array<any>
        ? typeof Fragment
        : HTMLElement
    : T extends HTMLElement
      ? (props: {}) => T
      : T extends keyof GlobalComponents
        ? ExtractRenderComponent<GlobalComponents[T]>
        : T extends keyof NativeElements
          ? (props: NativeElements[T]) => JSX.Element
          : (props: {}) => JSX.Element;
export declare function extractRenderComponent<T extends string>(t: T): ExtractRenderComponent<T>;
export declare function extractRenderComponent<T>(t: T): ExtractRenderComponent<T>;
export type ExtractComponentProps<T> = T extends { new (): infer I }
  ? ExtractComponentProps<I>
  : T extends { $props: infer P }
    ? P
    : T extends HTMLElement
      ? HTMLAttributes
      : T extends (p: infer P) => any
        ? P
        : {};
export declare function enhanceElementWithProps<T, P>(el: T, props: P): T & P;
export declare function instantiateComponent<T, P>(
  comp: T,
  props: P,
): T extends { new (...args: any[]): infer I } ? I : T extends (...args: any[]) => infer R ? R : T;
export declare function extractArgumentsFromRenderSlot<
  TSlots extends Record<string, any>,
  N extends keyof TSlots & string,
>(
  component: { $slots: TSlots },
  slotName: N,
): TSlots[N] extends ((...args: infer P) => any) | undefined ? P[0] : never;
export type ExtractLeafElement<T> = T extends HTMLElement
  ? T
  : T extends { $el: infer E }
    ? ExtractLeafElement<E>
    : T extends { new (): infer I }
      ? ExtractLeafElement<I>
      : never;
export type ExtractDirectives<T> = {
  [K in keyof T as T[K] extends Directive<any, any, any, any>
    ? K extends `v${Capitalize<string>}`
      ? K
      : never
    : never]: T[K];
};
export declare function runCustomDirective<
  TInstance,
  TDirective extends Directive<ExtractLeafElement<TInstance>>,
>(
  instance: TInstance,
  directive: TDirective,
): ExtractLeafElement<TInstance> extends infer El extends HTMLElement
  ? TDirective extends Directive<infer TElement, infer TValue, infer M extends string>
    ? El extends TElement
      ? (
          instance: TInstance,
          value: TValue,
          arg: string | undefined,
          modifiers: { [K in M]?: true },
        ) => void
      : (
          instance: TElement,
          value: TValue,
          arg: string | undefined,
          modifiers: { [K in M]?: true },
        ) => void
    : false
  : false;
export declare function retrieveSetupDirectives<T>(
  o: T,
): ExtractDirectives<T> extends infer D
  ? ExtractDirectives<Omit<GlobalDirectives, keyof D>> & D
  : ExtractDirectives<GlobalDirectives>;
export type IsExactlyEqual<A, B> =
  (<T>() => T extends A ? 1 : 2) extends <T>() => T extends B ? 1 : 2 ? true : false;
export declare function strictRenderSlot<T extends (...args: any[]) => any, U>(
  slot: T,
  child: ReturnType<T> extends infer R
    ? R extends Array<any>
      ? never
      : R extends string
        ? [R]
        : R extends U
          ? [U]
          : R
    : ReturnType<T>,
): any;
export declare function strictRenderSlot<T extends (...args: any[]) => any, U>(
  slot: T,
  children: ReturnType<T> extends infer R
    ? R extends readonly [any, ...any[]]
      ? R
      : R extends Array<infer E>
        ? U extends Array<infer UE>
          ? [UE] extends [never]
            ? U
            : E extends string | number | boolean | symbol | bigint | null | undefined
              ? E extends UE
                ? U
                : never
              : UE extends E
                ? IsExactlyEqual<UE, E> extends true
                  ? U
                  : never
                : never
          : never
        : never
    : ReturnType<T>,
): any;
export declare function checkRequiredSlots<T>(
  slots: T,
  provided: { [K in keyof T as undefined extends T[K] ? never : K]: true },
): void;

// instance/instance.ts
export type PublicInstanceFromMacro<
  Props,
  Emits,
  Expose,
  Slots,
  Attrs,
  El extends Element = Element,
> = {
  $props: Props;
  $emit: Emits;
  $slots: Slots;
  $attrs: Attrs;
  $el: El;
} & Props &
  Expose;

// vue/vue.ts
export declare function shallowUnwrapRef<T>(obj: T): ShallowUnwrapRef<T>;

// ── Local utility types (used by Box helpers) ───────────────────

type Data = Record<string, unknown>;
type DefaultFactory<T> = (props: Data) => T | null | undefined;
type DefineModelOptions<T = any, G = T, S = T> = {
  get?: (v: T) => G;
  set?: (v: S) => any;
};
type InferDefault<P, T> = ((props: P) => T & {}) | (T extends NativeType ? T : never);
type InferDefaults<T> = {
  [K in keyof T]?: InferDefault<T, T[K]>;
};
type NativeType = null | undefined | number | string | boolean | symbol | Function;
interface PropOptions<T = any, D = T> {
  type?: PropType<T> | true | null;
  required?: boolean;
  default?: D | DefaultFactory<D> | null | undefined | object;
  validator?(value: unknown, props: Data): boolean;
}

// ── Box helpers (vue/vue.macros.ts) ─────────────────────────────

// defineProps
export declare function defineProps_Box<PropNames extends string = string>(
  props: PropNames[],
): PropNames[];
export declare function defineProps_Box<
  PP extends ComponentObjectPropsOptions = ComponentObjectPropsOptions,
>(props: PP): PP;
export declare function defineProps_Box<TypeProps>(): TypeProps;

// withDefaults
export declare function withDefaults_Box<T, Defaults extends InferDefaults<T>>(
  props: T,
  defaults: Defaults,
): [T, Defaults];

// defineEmits
export declare function defineEmits_Box<EE extends string = string>(emitOptions: EE[]): EE[];
export declare function defineEmits_Box<E extends EmitsOptions = EmitsOptions>(emitOptions: E): E;
export declare function defineEmits_Box<T extends ComponentTypeEmits>(): T;

// defineOptions
export declare function defineOptions_Box<
  RawBindings = {},
  D = {},
  C extends ComputedOptions = {},
  M extends MethodOptions = {},
  Mixin extends ComponentOptionsMixin = ComponentOptionsMixin,
  Extends extends ComponentOptionsMixin = ComponentOptionsMixin,
  InheritAttrs extends true | false = true,
  T = Record<string, any>,
>(
  options?: T &
    ComponentOptionsBase<{}, RawBindings, D, C, M, Mixin, Extends, {}> & {
      props?: never;
      emits?: never;
      expose?: never;
      slots?: never;
      inheritAttrs?: InheritAttrs;
    },
): T;

// defineModel
export declare function defineModel_Box<T, M extends PropertyKey = string, G = T, S = T>(
  options: ({ default: any } | { required: true }) & PropOptions<T> & DefineModelOptions<T, G, S>,
): ({ default: any } | { required: true }) & PropOptions<T> & DefineModelOptions<T, G, S>;
export declare function defineModel_Box<T, M extends PropertyKey = string, G = T, S = T>(
  options?: PropOptions<T> & DefineModelOptions<T, G, S>,
): PropOptions<T> & DefineModelOptions<T, G, S>;
export declare function defineModel_Box<T, M extends PropertyKey = string, G = T, S = T>(
  name: string,
  options: ({ default: any } | { required: true }) & PropOptions<T> & DefineModelOptions<T, G, S>,
): [string, ({ default: any } | { required: true }) & PropOptions<T> & DefineModelOptions<T, G, S>];
export declare function defineModel_Box<T, M extends PropertyKey = string, G = T, S = T>(
  name: string,
  options?: PropOptions<T> & DefineModelOptions<T, G, S>,
): [string, PropOptions<T> & DefineModelOptions<T, G, S>];

// defineExpose
export declare function defineExpose_Box<Exposed extends Record<string, any> = Record<string, any>>(
  exposed?: Exposed,
): Exposed;

// defineSlots
export declare function defineSlots_Box<S extends Record<string, any> = Record<string, any>>(): S;

declare module "vue" {
  // Guarantee the augmentable GlobalComponents surface exists on EVERY Vue
  // version (Vue <3.5 ships no GlobalComponents export); user/UI-kit
  // augmentations merge in, and absence stays fail-closed `unknown`.
  interface GlobalComponents {}
}
