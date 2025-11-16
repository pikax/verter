
/**
 * These are the props used in the template or public API of the component.
 * They have optional props made optional.
 */
export type MakePublicProps<T extends Record<PropertyKey, any>> =
  T extends import("vue").DefineProps<infer P, any>
    ? T extends import("vue").DefineProps<P, infer K extends keyof P>
      ? P & Partial<Pick<P, K>>
      : T
    : T;

/**
 * These are the internal props of the component.
 * They have all props required.
 * 
 * They are accessed inside the component implementation.
 */
export type MakeInternalProps<T extends Record<PropertyKey, any>> =
  T extends import("vue").DefineProps<infer P, any>
    ? T extends import("vue").DefineProps<P, infer K extends keyof P>
      ? P & Required<Pick<P, K>>
      : T
    : T;
