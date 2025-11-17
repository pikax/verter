import { MacroReturn, MacroReturnObject, MacroReturnType } from "../setup";

/**
 * Extracts the keys of properties that can be undefined (optional properties).
 * 
 * @template T - The object type to extract optional keys from
 * @returns Union of keys for properties where undefined is in the type
 * 
 * @example
 * ```ts
 * type Props = { foo?: string; bar: number; baz?: boolean };
 * type Optional = FindDefaultsKey<Props>; // 'foo' | 'baz'
 * ```
 */
export type FindDefaultsKey<T> = {
  [K in keyof T]-?: undefined extends T[K] ? K : never;
}[keyof T];

/**
 * Resolves the underlying type from a MacroReturn wrapper.
 * Extracts the type parameter from MacroReturnType or the value from MacroReturnObject.
 * 
 * @template T - The type to unwrap
 * @returns The unwrapped type, or T if not a MacroReturn
 * 
 * @example
 * ```ts
 * type Wrapped = MacroReturnType<{ foo: string }, "Props">;
 * type Unwrapped = ResolveFromMacroReturn<Wrapped>; // { foo: string }
 * ```
 */
export type ResolveFromMacroReturn<T> = T extends MacroReturnType<any, infer TT>
  ? TT
  : T extends MacroReturnObject<infer V, any>
  ? V
  : T;

/**
 * Resolves props and defaults information from a macro return type.
 * Analyzes the macro structure to determine which properties have defaults.
 * 
 * @template T - The macro return type to analyze
 * @returns Object with 'type' (props type) and 'defaults' (keys with defaults)
 * 
 * @example
 * ```ts
 * const setup = () => createMacroReturn({
 *   props: { value: { foo: '', bar: 0 }, type: {} as { foo?: string; bar: number } }
 * });
 * type Info = ResolveDefaultsPropsFromMacro<ExtractProps<...>>;
 * // { type: { foo?: string; bar: number }, defaults: 'foo' }
 * ```
 */
export type ResolveDefaultsPropsFromMacro<T> = T extends MacroReturn<any, any>
  ? T extends {
      value: infer V;
      defaults: {
        type: [any, infer D];
      };
    }
    ? {
        type: V;
        defaults: keyof D;
      }
    : {
        type: ResolveFromMacroReturn<T>;
        defaults: FindDefaultsKey<ResolveFromMacroReturn<T>>;
      }
  : T;

/**
 * These are the props used in the template or public API of the component.
 * They have optional props made optional.
 */
export type MakePublicProps<T extends Record<PropertyKey, any>> =
  ResolveDefaultsPropsFromMacro<T> extends {
    type: infer P;
    defaults: infer D;
  }
    ? D extends keyof P
      ? Omit<P, D> & {
          [K in keyof P as K extends D ? K : never]?: P[K] | undefined;
        }
      : P & { a: D }
    : ResolveFromMacroReturn<T> extends infer P
    ? P // TODO add more tests to cover this case, after having tests we can implement this correctly
    : T extends import("vue").DefineProps<infer P, any>
    ? T extends import("vue").DefineProps<P, infer K extends keyof P>
      ? P & Required<Pick<P, K>>
      : T
    : T;

export type MakePublicPropsBak<T extends Record<PropertyKey, any>> =
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

