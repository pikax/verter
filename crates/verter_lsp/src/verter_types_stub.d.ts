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
} from "vue";

// ── Core types ──────────────────────────────────────────────────

// setup/setup.ts
export type Prettify<T> = T extends { (...args: any[]): any } ? T : { [K in keyof T]: T[K] } & {};
export declare function createMacroReturn<T>(): T;

// helpers/helpers.ts
export type OmitConstructorSignature<T> = { [K in keyof T]: T[K] };

// components/components.ts
export type ExtractComponentProps<T> = T extends { new (): infer I }
  ? { [K in keyof I]: I[K] }
  : {};
export declare function enhanceElementWithProps<T, P>(el: T, props: P): T & P;

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
