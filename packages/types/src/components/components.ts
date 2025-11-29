import { defineComponent } from "vue";

export type GetVueComponent<T> = T extends { new (): infer I }
  ? I
  : T extends (...args: any) => infer R
  ? void extends R
    ? typeof import("vue").Comment
    : R extends Array<any>
    ? typeof import("vue").Fragment
    : HTMLElement
  : T extends HTMLElement
  ? T
  : never;

/**
 * Helper to maintain the actual type of the name and inheritAttrs options
 * @param o
 */
export declare function DefineOptions<
  T extends { name?: TName; inheritAttrs?: TAttr },
  TName extends string,
  TAttr extends boolean
>(o: T): T;

// Helper to retrieve component instance with merged props
//  NOTE this cannot work with generic components due to TS limitations
// for generics we need to improve it
// TODO add more overloads
export declare function retrieveInstance<
  C extends { new (): { $props: {} } },
  P extends import("vue").ExtractPublicPropTypes<InstanceType<C>["$props"]>
>(
  comp: C,
  props: P
): Omit<InstanceType<C>, "$props"> & { $props: InstanceType<C>["$props"] & P };

// Helper to omit keys where the value is never
type OmitNever<T> = {
  [K in keyof T as [T[K]] extends [never] ? never : K]: T[K];
};

// Internal helper that extracts components but leaves never for non-components
type ExtractComponentsRaw<T> = {
  [K in keyof T]: T[K] extends GetVueComponent<T[K]>
    ? T[K]
    : T[K] extends Record<string, any>
      ? OmitNever<ExtractComponentsRaw<T[K]>>
      : never;
};

// Extracts only Vue components from an object, deeply removing non-component properties
export type ExtractComponents<T> = OmitNever<ExtractComponentsRaw<T>>;

const foo = {
  Comp: defineComponent({}),
  b: 1,
  Teleport: {} as typeof import("vue").Teleport,

  deep: {
    Comp2: defineComponent({}),
    not: "a component",
  },
  deeper: {
    even: {
      not: "a component",
      Comp3: defineComponent({}),
    },
  },
};

const extracted = {} as ExtractComponents<typeof foo>;

extracted.Comp;
extracted.deep.Comp2;
extracted.deeper.even.Comp3;
// @ts-expect-error - b is not a component
extracted.extracted.extracted.b;
// @ts-expect-error - Teleport is not included because it's not a component constructor
extracted.deep.not;
// @ts-expect-error - Teleport is not included because it's not a component constructor
extracted.deeper.even.not;
