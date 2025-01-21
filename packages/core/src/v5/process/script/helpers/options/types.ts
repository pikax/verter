import { ModelRef as $V_ModelRef } from "vue";

/* __VERTER_IMPORTS__
[
  {
    "from": "vue",
    "asType": true,
    "items": [
      { name: "ModelRef", alias: "$V_ModelRef" }
    ]
  }
]
/__VERTER_IMPORTS__ */

// __VERTER__START__

/**
 * Utility for extracting the parameters from a function overload (for typed emits)
 * https://github.com/microsoft/TypeScript/issues/32164#issuecomment-1146737709
 */
export type $V_OverloadParameters<T extends (...args: any[]) => any> =
  Parameters<$V_OverloadUnion<T>>;

export type $V_OverloadProps<TOverload> = Pick<TOverload, keyof TOverload>;

export type $V_OverloadUnionRecursive<
  TOverload,
  TPartialOverload = unknown
> = TOverload extends (...args: infer TArgs) => infer TReturn
  ? TPartialOverload extends TOverload
    ? never
    :
        | $V_OverloadUnionRecursive<
            TPartialOverload & TOverload,
            TPartialOverload &
              ((...args: TArgs) => TReturn) &
              $V_OverloadProps<TOverload>
          >
        | ((...args: TArgs) => TReturn)
  : never;

export type $V_OverloadUnion<TOverload extends (...args: any[]) => any> =
  Exclude<
    $V_OverloadUnionRecursive<(() => never) & TOverload>,
    TOverload extends () => never ? never : () => never
  >;

export type $V_UnionToIntersection<U> = (
  U extends any ? (k: U) => void : never
) extends (k: infer I) => void
  ? I
  : never;

export type $V_EmitMapToProps<T> = T extends [
  infer E extends string,
  ...infer A
]
  ? { [K in `on${Capitalize<E>}`]?: (...args: A) => void }
  : never;

export type $V_ModelToProps<T> = {
  [K in keyof T]: T[K] extends $V_ModelRef<infer C>
    ? C
    : T[K] extends $V_ModelRef<infer C> | undefined
    ? C | undefined
    : null;
};

export type $V_ModelToEmits<T> = {} extends T
  ? () => any
  : {
      [K in keyof T]-?: (
        event: `update:${K & string}`,
        arg: T[K] extends $V_ModelRef<infer C>
          ? C
          : T[K] extends $V_ModelRef<infer C> | undefined
          ? C | undefined
          : unknown
      ) => any;
    }[keyof T];

export type $V_MakeOptionalIfUndefined<T> = {
  [K in keyof T as undefined extends T[K] ? K : never]?: T[K];
} extends infer Optional
  ? Omit<T, keyof Optional> & Optional
  : {};

/**
 * Helper to maintain the actual type of the name and inheritAttrs options
 * @param o
 */
export declare function $V_DefineOptions<
  T extends { name?: TName; inheritAttrs?: TAttr },
  TName extends string,
  TAttr extends boolean
>(o: T): T;

export type $V_NormaliseComponents<T extends Record<string, any>> = {
  [K in keyof T]: T[K] extends infer C
    ? C extends { new (): infer I }
      ? I
      : C extends (...args: any) => infer R
      ? void extends R
        ? typeof import("vue").Comment
        : R extends Array<any>
        ? typeof import("vue").Fragment
        : HTMLElement
      : C extends HTMLElement
      ? C
      : $V_NormaliseComponents<T>
    : never;
};
