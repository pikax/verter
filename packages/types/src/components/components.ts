import { Prettify } from "../setup";

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

/**
 * Extracts only valid Vue component types from an object, deeply removing non-component properties.
 *
 * This utility type recursively traverses an object structure and:
 * - Keeps properties that are valid Vue components (class constructors, functional components, HTMLElements)
 * - Recursively processes nested objects to extract components at any depth
 * - Removes properties that are not Vue components (primitives, non-component objects, etc.)
 *
 * Use case: Extract renderable components from a build context or module exports,
 * filtering out non-component values like constants, utilities, or configuration objects.
 *
 * @example
 * ```ts
 * const context = {
 *   MyButton: defineComponent({ ... }),
 *   MyInput: defineComponent({ ... }),
 *   config: { theme: 'dark' },
 *   utils: {
 *     helper: () => {},
 *     NestedComp: defineComponent({ ... })
 *   }
 * };
 *
 * type Components = ExtractComponents<typeof context>;
 * // Result: {
 * //   MyButton: typeof MyButton;
 * //   MyInput: typeof MyInput;
 * //   utils: {
 * //     NestedComp: typeof NestedComp;
 * //   }
 * // }
 * ```
 *
 * @typeParam T - The object type to extract components from
 */
// export type ExtractComponents<T, Default = {}> = Prettify<
//   OmitNever<{
//     [K in keyof T]: ExtractComponent<T[K]>;
//   }>
// > extends infer O
//   ? [{}] extends [O]
//     ? Default
//     : O
//   : never;
export type ExtractComponents<T, Default = {}> = {
  [K in keyof T as ExtractComponent<T[K]> extends never
    ? never
    : K]: ExtractComponent<T[K]>;
} extends infer O
  ? [{}] extends [O]
    ? Default
    : O
  : never;

export type ExtractComponent<T> = GetVueComponent<T> extends never
  ? T extends Array<infer U>
    ? U extends Record<string, any>
      ? ExtractComponents<U, never>
      : never
    : T extends Record<string, any>
    ? ExtractComponents<T, never>
    : never
  : T;

export declare function extractComponents<T>(t: T): ExtractComponents<T>;

export declare function enhanceElementWithProps<T, P>(
  t: T,
  p: P
): T extends { new (): infer I extends P }
  ? I
  : T extends { new (): infer I }
  ? I & P
  : T extends { new (): any }
  ? InstanceType<T> & P
  : T & P;
