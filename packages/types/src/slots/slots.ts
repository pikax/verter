type IsExactlyEqual<A, B> = (<T>() => T extends A ? 1 : 2) extends <
  T
>() => T extends B ? 1 : 2
  ? true
  : false;

/**
 * Strictly render the slot, see https://github.com/vuejs/rfcs/discussions/733
 * @param slot render function
 * @param children
 */
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
    : ReturnType<T>
): any;

export declare function strictRenderSlot<T extends (...args: any[]) => any, U>(
  slot: T,
  children: ReturnType<T> extends infer R
    ? R extends readonly [any, ...any[]]
      ? R // Tuple: exact match
      : R extends Array<infer E>
      ? U extends Array<infer UE>
        ? [UE] extends [never]
          ? U // Empty array: UE is never
          : E extends
              | string
              | number
              | boolean
              | symbol
              | bigint
              | null
              | undefined
          ? E extends UE
            ? U // For primitives: accept if expected extends provided (allows widening)
            : never
          : UE extends E
          ? IsExactlyEqual<UE, E> extends true
            ? U // For objects: use exact equality check
            : never
          : never
        : never
      : never
    : ReturnType<T>
): any;

export declare function renderSlotJSX<T>(
  slot: T
): T extends (...args: infer A) => any ? (cb: (...a: A) => any) => any : never;

/**
 * Converts Vue slot types into JSX-compatible component types for rendering slots as components.
 *
 * This utility type transforms a slots object (typically from `$slots` or `SlotsType`) into an object
 * where each slot becomes a component constructor that can be used in JSX/TSX templates.
 *
 * The transformation:
 * - Slot functions with props `(props: P) => any` become `{ new(): { $props: P } }`
 * - Slot functions without props `() => any` become `{ new(): { $props: {} } }`
 * - Non-function slots fallback to `{ new(): { $props: {} } }`
 *
 * Use case: Render typed slots as JSX components while preserving prop type safety.
 *
 * @example
 * ```ts
 * const Component = defineComponent({
 *   slots: {} as SlotsType<{
 *     default: (props: { msg: string }) => any;
 *     header: (props: { title: string }) => any;
 *     footer: () => any;
 *   }>
 * });
 *
 * type Slots = SlotsToRender<typeof Component.$slots>;
 * // Result: {
 * //   default: { new(): { $props: { msg: string } } };
 * //   header: { new(): { $props: { title: string } } };
 * //   footer: { new(): { $props: {} } };
 * // }
 *
 * // Usage in JSX:
 * const RenderSlots = {} as Slots;
 * <RenderSlots.default msg="hello" />
 * <RenderSlots.header title="Page Title" />
 * <RenderSlots.footer />
 * ```
 *
 * @typeParam T - The slots type object, typically from `$slots` or defined via `SlotsType`
 */
export type SlotsToRender<T> = {
  [K in keyof T]: T[K] extends (props: infer P) => any
    ? { new (): { $props: P & {} } }
    : T[K] extends () => any
    ? { new (): { $props: {} } }
    : { new (): { $props: {} } };
};

/**
 * Extracts the slot props (arguments) from a component instance for a given slot name.
 *
 * This helper is used by the component-type plugin to provide correct typing
 * for scoped slot props when rendering slots in Vue templates.
 *
 * @template T - The component instance type (must have `$slots` property)
 * @template N - The slot name (must be a key of `$slots`)
 * @param component - The component instance to extract slot arguments from
 * @param slotName - The name of the slot to extract arguments for
 * @returns The first parameter type of the slot function (slot props)
 *
 * @example Basic usage with defineComponent
 * ```typescript
 * import { defineComponent, SlotsType } from "vue";
 *
 * const MyComponent = defineComponent({
 *   slots: {} as SlotsType<{
 *     default: (props: { msg: string }) => any;
 *     header: (props: { title: string }) => any;
 *     footer: () => any;
 *   }>,
 * });
 *
 * // In generated component-type code:
 * function Comp1() {
 *   return new MyComponent();
 * }
 *
 * // Extract slot props type
 * const defaultProps = extractArgumentsFromRenderSlot(Comp1(), "default");
 * // defaultProps has type: { msg: string }
 *
 * const headerProps = extractArgumentsFromRenderSlot(Comp1(), "header");
 * // headerProps has type: { title: string }
 *
 * const footerProps = extractArgumentsFromRenderSlot(Comp1(), "footer");
 * // footerProps has type: undefined (no props for this slot)
 * ```
 *
 * @example Use case in template type checking
 * ```typescript
 * // When the component-type plugin encounters:
 * // <MyComponent>
 * //   <template #default="{ msg }">{{ msg }}</template>
 * // </MyComponent>
 *
 * // It generates code like:
 * const slotProps = extractArgumentsFromRenderSlot(new MyComponent(), "default");
 * // This provides type information for the `{ msg }` destructuring
 * ```
 *
 * @see {@link file://packages/core/src/v5/process/script/plugins/component-type/component-type.ts} - Component type plugin that uses this helper
 */
export declare function extractArgumentsFromRenderSlot<
  T extends { $slots: { [K in N]: any } },
  N extends string
>(component: T, slotName: N): Parameters<T["$slots"][N]>[0];

