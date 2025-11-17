import { ExtractHidden } from "../helpers";
import { defineProps_Box, withDefaults_Box } from "../vue/vue.macros";
import { MacroReturn, MacroReturnObject, MacroReturnType } from "../setup";

type FindDefaultsKey<T> = {
  [K in keyof T]-?: undefined extends T[K] ? K : never;
}[keyof T];

/** -- test for FindDefaultsKey --
type AA = {
  foo?: string;
  test?: number;
  bar: number;
};

// 'foo' | 'test'
type R = FindDefaultsKey<AA>

declare const r  : FindDefaultsKey<AA>;

*/

type ResolveFromMacroReturn<T> = T extends MacroReturnType<any, infer TT>
  ? TT
  : T extends MacroReturnObject<infer V, any>
  ? V
  : T;

type ResolveDefaultsPropsFromMacro<T> = T extends MacroReturn<any, any>
  ? T extends {
      value: infer V;
      defaults: {
        type: [any, infer D];
      };
    }
    ? {
        type: V;
        defaults: keyof D;
      }
    : {
        type: ResolveFromMacroReturn<T>;
        defaults: FindDefaultsKey<ResolveFromMacroReturn<T>>;
      }
  : T;

/**
 * These are the props used in the template or public API of the component.
 * They have optional props made optional.
 */
export type MakePublicProps<T extends Record<PropertyKey, any>> =
  ResolveDefaultsPropsFromMacro<T> extends {
    type: infer P;
    defaults: infer D;
  }
    ? D extends keyof P
      ? Omit<P, D> & {
          [K in keyof P as K extends D ? K : never]?: P[K] | undefined;
        }
      : P & { a: D }
    : ResolveFromMacroReturn<T> extends infer P
    ? P // TODO add more tests to cover this case, after having tests we can implement this correctly
    : T extends import("vue").DefineProps<infer P, any>
    ? T extends import("vue").DefineProps<P, infer K extends keyof P>
      ? P & Required<Pick<P, K>>
      : T
    : T;

export type MakePublicPropsBak<T extends Record<PropertyKey, any>> =
  T extends import("vue").DefineProps<infer P, any>
    ? T extends import("vue").DefineProps<P, infer K extends keyof P>
      ? P & Partial<Pick<P, K>>
      : T
    : T;

/**
 * These are the internal props of the component.
 * They have all props required.
 *
 * They are accessed inside the component implementation.
 */
export type MakeInternalProps<T extends Record<PropertyKey, any>> =
  T extends import("vue").DefineProps<infer P, any>
    ? T extends import("vue").DefineProps<P, infer K extends keyof P>
      ? P & Required<Pick<P, K>>
      : T
    : T;

// --- Test ---

import { createMacroReturn, ExtractMacroReturn, ExtractProps } from "../setup";

const setupReturn = createMacroReturn({
  props: {
    value: {
      foo: "" as string,
      bar: 0 as number,
    },
    type: {} as {
      foo?: string;
      bar: number;
    },
  },
});

type Extracted = ExtractProps<ExtractMacroReturn<typeof setupReturn>>;
type Props = MakePublicProps<Extracted>;
declare const aaa: ResolveDefaultsPropsFromMacro<Extracted>;
declare const foo: Props;

foo.bar; // number
foo.foo; // string | undefined

const propsBoxed = defineProps_Box<{
  foo?: string;
  bar: number | undefined;
}>();

const dd = withDefaults(
  defineProps<{
    foo?: string;
    bar: number | undefined;
  }>(),
  {
    foo: "default",
  }
);

const defaultsBoxed = withDefaults_Box(
  defineProps<ExtractHidden<typeof propsBoxed>>(),
  {
    foo: "default",
  }
);
const props = withDefaults(defaultsBoxed[0], defaultsBoxed[1]);

type A = typeof props extends import("vue").DefineProps<any, any> ? {} : never;

const setupReturn2 = createMacroReturn({
  props: {
    value: props,
    type: props as typeof props,
    defaults: {
      value: props,
      type: {} as ExtractHidden<typeof defaultsBoxed>,
    },
  },
});

type Extracted2 = ExtractProps<ExtractMacroReturn<typeof setupReturn2>>;

declare const e: Extracted2;

e.defaults.type;

type Props2 = MakePublicProps<Extracted2>;
declare const fff: ResolveDefaultsPropsFromMacro<Extracted2>;

declare const foo2: Props2;

foo2.bar; // number
foo2.foo; // string | undefined

