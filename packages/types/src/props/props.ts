declare const DefaultsKey: unique symbol;
export type PropsWithDefaults<P, D extends keyof P> = P & { [DefaultsKey]?: D };

// /**
//  * Extracts the keys of properties that can be undefined (optional properties).
//  *
//  * @template T - The object type to extract optional keys from
//  * @returns Union of keys for properties where undefined is in the type
//  *
//  * @example
//  * ```ts
//  * type Props = { foo?: string; bar: number; baz?: boolean };
//  * type Optional = FindDefaultsKey<Props>; // 'foo' | 'baz'
//  * ```
//  */
// export type FindDefaultsKey<T> = {
//   [K in keyof T]-?: undefined extends T[K] ? K : never;
// }[keyof T];

// /**
//  * Resolves props and defaults information from a macro return type.
//  * Analyzes the macro structure to determine which properties have defaults.
//  *
//  * @template T - The macro return type to analyze
//  * @returns Object with 'type' (props type) and 'defaults' (keys with defaults)
//  *
//  * @example
//  * ```ts
//  * const setup = () => createMacroReturn({
//  *   props: { value: { foo: '', bar: 0 }, type: {} as { foo?: string; bar: number } }
//  * });
//  * type Info = ResolveDefaultsPropsFromMacro<ExtractProps<...>>;
//  * // { type: { foo?: string; bar: number }, defaults: 'foo' }
//  * ```
//  */
// export type ResolveDefaultsPropsFromMacro<T> = T extends ExtractMacroProps<
//   infer M
// >
//   ? M extends {
//       props: infer PM extends MacroReturnType<any, any>;
//     }
//     ? M extends {
//         withDefaults: {
//           type: [any, infer D];
//         };
//       }
//       ? { type: ResolveFromMacroReturn<PM>; defaults: keyof D }
//       : {
//           type: ResolveFromMacroReturn<PM>;
//           defaults: FindDefaultsKey<ResolveFromMacroReturn<PM>>;
//         }
//     : T extends {
//         props: MacroReturnObject<infer V, infer O>;
//       }
//     ? {
//         type: V;
//         defaults: PropsObjectExtractDefaults<O>;
//       }
//     : T
//   : T;

// /**
//  * Extracts the keys of properties that have default values from a props object definition.
//  *
//  * This helper is used to identify which properties in a Vue props object have defaults,
//  * which affects their optionality in the component's public API.
//  *
//  * @template T - The props object type to analyze (can be string array or object with default properties)
//  * @returns Union of property keys that have defaults, or `string` for string array format
//  *
//  * @example
//  * ```ts
//  * // Object format with defaults
//  * type PropsObj = {
//  *   name: { type: String; default: 'Anonymous' };
//  *   age: { type: Number };
//  *   active: { type: Boolean; default: true };
//  * };
//  * type WithDefaults = PropsObjectExtractDefaults<PropsObj>; // 'name' | 'active'
//  * ```
//  *
//  * @example
//  * ```ts
//  * // String array format (simplified props)
//  * type PropsArray = ['name', 'age'];
//  * type Defaults = PropsObjectExtractDefaults<PropsArray>; // string
//  * ```
//  */
// export type PropsObjectExtractDefaults<T> = T extends string[]
//   ? string
//   : {
//       [K in keyof T]: T[K] extends { default: any } ? K : "";
//     }[keyof T];

/**
 * Transforms component props for the public API (component usage).
 *
 * - Props with defaults become optional (can be omitted when using the component)
 * - Props without defaults remain optional (with `| undefined` type)
 * - Preserves readonly nature of props
 *
 * @template T - The props type (from defineProps or macro)
 * @returns Transformed props suitable for component public API
 *
 * @remarks
 * This type has two main paths:
 * 1. **Macro path** (preferred): Uses ResolveDefaultsPropsFromMacro to identify props
 *    with defaults and makes them optional via Omit + mapped type approach
 * 2. **DefineProps fallback**: For Vue's DefineProps type, uses Partial<Pick<P, K>>
 *    where K are the keys with defaults. Note: Partial is correct here, NOT Required.
 *    The intersection P & Partial<Pick<P, K>> maintains the correct optionality.
 *
 * @example
 * ```ts
 * const props = defineProps({
 *   id: Number,                              // No default
 *   name: { type: String, default: 'Anonymous' }, // With default
 * });
 * type Public = MakePublicProps<typeof props>;
 * // Public API: { readonly id?: number | undefined; readonly name?: string }
 * // name is optional (can omit), id is optional (can be undefined)
 * ```
 */
export type MakePublicProps<T extends Record<PropertyKey, any>> =
  T extends PropsWithDefaults<infer P, infer D>
    ? MakeBooleanOptional<Omit<P, D>> & {
        [K in keyof P as K extends D ? K : never]?: P[K] | undefined;
      }
    : // TODO handle `defineComponent` and other types
      MakeBooleanOptional<T>;

// ResolveDefaultsPropsFromMacro<T> extends {
//   type: infer P;
//   defaults: infer D;
// }
//   ? D extends keyof P
//     ? Omit<P, D> & {
//         [K in keyof P as K extends D ? K : never]?: P[K] | undefined;
//       }
//     : P
//   : T extends [infer PP, any]
//   ? MakeBooleanOptional<PP>
//   : MakeBooleanOptional<T>;

/**
 * Makes boolean props with defaults optional in the type system.
 *
 * Vue automatically provides a default value of `false` for boolean props that aren't passed,
 * so they should be optional in the public API. This type takes a DefineProps type and makes
 * the boolean props (indicated by the BKeys parameter) optional with `| undefined`.
 *
 * @template T - The DefineProps type to transform
 * @returns Type with boolean props made optional, or T if not DefineProps
 *
 * @example
 * ```ts
 * type Props = DefineProps<
 *   { name: string; active: boolean; visible: boolean },
 *   'active' | 'visible'
 * >;
 * type Result = MakeBooleanOptional<Props>;
 * // Result: { name: string; active?: boolean | undefined; visible?: boolean | undefined }
 * ```
 *
 * @remarks
 * This is used internally by MakePublicProps to handle Vue's special boolean prop behavior.
 * If T is not a DefineProps type, it returns T unchanged.
 */
export type MakeBooleanOptional<T> = T extends import("vue").DefineProps<
  T,
  infer BKeys extends keyof T
>
  ? Omit<T, BKeys> & {
      [K in keyof T as K extends BKeys ? K : never]?: T[K] | undefined;
    }
  : T;

/**
 * Extracts the keys representing boolean props with defaults from a DefineProps type.
 *
 * In Vue, boolean props default to `false` if not provided, so they're treated as optional.
 * This helper extracts the second type parameter of `DefineProps<T, BKeys>`, which represents
 * keys of boolean props that have default values.
 *
 * @template T - The DefineProps type to extract boolean keys from
 * @returns A union of keys that are boolean props with defaults, or `never` if none
 *
 * @example
 * ```ts
 * type Props = DefineProps<{ name: string; active: boolean }, 'active'>;
 * type BoolKeys = ExtractBooleanKeys<Props>;
 * // Result: 'active'
 * ```
 *
 * @example
 * ```ts
 * type Props = DefineProps<
 *   { a: string; b: boolean; c: boolean },
 *   'b' | 'c'
 * >;
 * type BoolKeys = ExtractBooleanKeys<Props>;
 * // Result: 'b' | 'c'
 * ```
 */
export type ExtractBooleanKeys<T> = T extends import("vue").DefineProps<
  T,
  infer BKeys extends keyof T
>
  ? BKeys
  : never;

/**
 * Transforms component props for internal usage within the component implementation.
 *
 * Unlike the public API (MakePublicProps), internal props have different requirements:
 * - Props with defaults are **required** (always defined, never undefined)
 * - Props without defaults are **required** (must be present, though may be undefined)
 * - All props are accessible and type-safe within the component
 *
 * This ensures that inside the component, TypeScript knows which props have been
 * populated by Vue (either from user input or defaults) vs which might be undefined.
 *
 * @template T - The props type (from defineProps or macro)
 * @returns Transformed props suitable for internal component implementation
 *
 * @example
 * ```ts
 * const props = defineProps({
 *   id: Number,                                   // No default
 *   name: { type: String, default: 'Anonymous' }, // With default
 * });
 * type Internal = MakeInternalProps<typeof props>;
 * // Internal usage: { readonly id: number; readonly name: string }
 * // id is required (number, not undefined), name is required (always string)
 * ```
 *
 * @example
 * ```ts
 * // Inside component implementation
 * const props = defineProps({
 *   count: { type: Number, default: 0 },
 *   label: String,
 * });
 * // props.count is always number (has default)
 * // props.label is string (required prop)
 * console.log(props.count.toFixed(2)); // Safe - always defined
 * console.log(props.label.toUpperCase()); // Safe - always string
 * ```
 *
 * @remarks
 * This type is used for the internal component implementation perspective,
 * whereas MakePublicProps is used for the external component usage perspective.
 * The key difference is that props with defaults are required internally (always defined)
 * but optional externally (can be omitted when using the component).
 */
export type MakeInternalProps<T extends Record<PropertyKey, any>> =
  T extends PropsWithDefaults<infer P, infer D>
    ? Omit<P, D> & {
        [K in keyof P as K extends D ? K : never]-?: P[K] | undefined;
      }
    : // TODO handle `defineComponent` and other types
      MakeBooleanOptional<T>;
