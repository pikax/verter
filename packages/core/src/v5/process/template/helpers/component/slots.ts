export declare function $V_StrictRenderSlot<
  T extends (...args: any[]) => any,
  Single extends boolean = ReturnType<T> extends Array<any> ? false : true
>(slot: T, o: Single extends true ? [ReturnType<T>] : ReturnType<T>): any;
