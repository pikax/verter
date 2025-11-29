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

export type SlotsToRender<T> = {
  [K in keyof T]: T[K] extends (props: infer P) => any
    ? { new (): { $props: P & {} } }
    : T[K] extends () => any
      ? { new (): { $props: {} } }
      : { new (): { $props: {} } };
};
