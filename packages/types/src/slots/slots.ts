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
export declare function StrictRenderSlot<T extends (...args: any[]) => any, U>(
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

export declare function StrictRenderSlot<T extends (...args: any[]) => any, U>(
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
