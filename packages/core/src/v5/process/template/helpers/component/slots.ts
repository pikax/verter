
/**
 * Strictly render the slot, see https://github.com/vuejs/rfcs/discussions/733
 * @param slot render function 
 * @param children 
 */
export declare function $V_StrictRenderSlot<
  T extends (...args: any[]) => any,
  Single extends boolean = ReturnType<T> extends Array<any> ? false : true
>(slot: T, children: Single extends true ? [ReturnType<T>] : ReturnType<T>): any;
