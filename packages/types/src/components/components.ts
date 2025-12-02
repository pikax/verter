/**
 * Vue Component Type Utilities
 *
 * This module provides type utilities for working with Vue components in TypeScript.
 * These helpers enable type-safe component extraction, instance retrieval, and
 * prop enhancement for use in the Verter Vue Language Server.
 *
 * @module components
 */

import { Prettify } from "../setup";

/**
 * Extracts the instance type from a Vue component definition.
 *
 * This utility type handles different component definition patterns:
 * - Class-based components (constructors): Returns the instance type
 * - Functional components: Determines return type (Comment, Fragment, or HTMLElement)
 * - HTMLElement: Returns the element type directly
 * - Non-component types: Returns `never`
 *
 * @example Class-based component
 * ```ts
 * const MyComponent = defineComponent({ ... });
 * type Instance = GetVueComponent<typeof MyComponent>;
 * // Instance is the component's instance type
 * ```
 *
 * @example Functional component returning void (renders Comment)
 * ```ts
 * const FnComp = () => {};
 * type Instance = GetVueComponent<typeof FnComp>;
 * // Instance is typeof Comment
 * ```
 *
 * @example Functional component returning array (renders Fragment)
 * ```ts
 * const FnComp = () => [h('div'), h('span')];
 * type Instance = GetVueComponent<typeof FnComp>;
 * // Instance is typeof Fragment
 * ```
 *
 * @typeParam T - The component definition or element type to extract from
 */
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
 * Helper function to maintain literal types for `name` and `inheritAttrs` options.
 *
 * When using `defineOptions()` in Vue, TypeScript may widen literal types.
 * This helper preserves the exact string literal type for `name` and the
 * exact boolean literal type for `inheritAttrs`.
 *
 * @example
 * ```ts
 * // Without DefineOptions, 'name' would be widened to `string`
 * const options = DefineOptions({
 *   name: 'MyComponent',      // Type is 'MyComponent', not string
 *   inheritAttrs: false       // Type is false, not boolean
 * });
 * ```
 *
 * @typeParam T - The options object type
 * @typeParam TName - The literal string type for the `name` option
 * @typeParam TAttr - The literal boolean type for the `inheritAttrs` option
 * @param o - The options object
 * @returns The same options object with preserved literal types
 */
export declare function DefineOptions<
  T extends { name?: TName; inheritAttrs?: TAttr },
  TName extends string,
  TAttr extends boolean
>(o: T): T;

/**
 * Retrieves a component instance type with merged props.
 *
 * This helper creates a type that represents a component instance where additional
 * props have been merged with the component's existing `$props`. Useful for
 * type-checking component usage with additional bound props.
 *
 * **Limitations:**
 * - Does not work with generic components due to TypeScript limitations
 * - Future versions may add more overloads for better generic support
 *
 * @example
 * ```ts
 * const MyButton = defineComponent({
 *   props: { label: String }
 * });
 *
 * type ButtonWithDisabled = ReturnType<typeof retrieveInstance<
 *   typeof MyButton,
 *   { disabled: boolean }
 * >>;
 * // ButtonWithDisabled.$props includes both 'label' and 'disabled'
 * ```
 *
 * @typeParam C - The component constructor type (must have `$props`)
 * @typeParam P - The additional props to merge
 * @param comp - The component constructor
 * @param props - The additional props to merge
 * @returns The component instance type with merged props
 */
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

/**
 * Extracts a single Vue component from a value, or recursively extracts components from nested structures.
 *
 * This is the workhorse type used by `ExtractComponents`. It determines whether a value is:
 * - A valid Vue component: Returns the original type `T`
 * - An array of records: Recursively extracts components from array elements
 * - A plain record: Recursively extracts components from the object
 * - Non-component primitive: Returns `never`
 *
 * @example Direct component
 * ```ts
 * const Button = defineComponent({ ... });
 * type Extracted = ExtractComponent<typeof Button>;
 * // Returns typeof Button (it's a valid component)
 * ```
 *
 * @example Nested object
 * ```ts
 * const modules = {
 *   Button: defineComponent({ ... }),
 *   config: 'string'
 * };
 * type Extracted = ExtractComponent<typeof modules>;
 * // Returns { Button: typeof Button } (config is filtered out)
 * ```
 *
 * @typeParam T - The value to extract component(s) from
 */
export type ExtractComponent<T> = GetVueComponent<T> extends never
  ? T extends Array<infer U>
    ? U extends Record<string, any>
      ? ExtractComponents<U, never>
      : never
    : T extends Record<string, any>
    ? ExtractComponents<T, never>
    : never
  : T;

/**
 * Runtime helper function for extracting components from an object.
 *
 * This function is used at runtime to filter an object and return only
 * the properties that are valid Vue components. The type system uses
 * `ExtractComponents<T>` to provide accurate typing.
 *
 * @example
 * ```ts
 * import * as allExports from './components';
 *
 * // Filter to only get actual Vue components
 * const components = extractComponents(allExports);
 * // components only includes valid Vue component exports
 * ```
 *
 * @typeParam T - The object type to extract components from
 * @param t - The object containing potential Vue components
 * @returns An object containing only the valid Vue component properties
 */
export declare function extractComponents<T>(t: T): ExtractComponents<T>;

/**
 * Enhances an element or component type with additional props.
 *
 * This utility type is used during template type generation to add
 * bound props to element/component types. It handles different input types:
 *
 * - **Constructor where instance already extends P**: Returns the instance type as-is
 * - **Constructor with instance type**: Returns instance type intersected with props
 * - **Other constructors**: Returns `InstanceType<T> & P`
 * - **Non-constructors**: Returns `T & P` (direct intersection)
 *
 * This is primarily used by the `ComponentTypePlugin` to generate accurate
 * types for template elements with their bound attributes and props.
 *
 * @example HTML element with props
 * ```ts
 * type EnhancedDiv = enhanceElementWithProps<HTMLDivElement, { class: string }>;
 * // Result: HTMLDivElement & { class: string }
 * ```
 *
 * @example Vue component with props
 * ```ts
 * const MyButton = defineComponent({ props: { label: String } });
 * type Enhanced = enhanceElementWithProps<typeof MyButton, { disabled: boolean }>;
 * // Result: InstanceType<typeof MyButton> & { disabled: boolean }
 * ```
 *
 * @typeParam T - The element or component type to enhance
 * @typeParam P - The props type to add
 * @param t - The element or component (used for type inference)
 * @param p - The props object (used for type inference)
 * @returns The enhanced type with merged props
 */
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
