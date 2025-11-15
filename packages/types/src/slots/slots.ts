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
      : R extends U
      ? [U]
      : R
    : ReturnType<T>
): any;
export declare function StrictRenderSlot<T extends (...args: any[]) => any, U>(
  slot: T,
  children: ReturnType<T> extends infer R
    ? R extends Array<any>
      ? R
      : ReturnType<T>
    : ReturnType<T>
): any;
