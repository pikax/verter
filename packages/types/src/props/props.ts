import { extractHiddenPatch, UniqueKey } from "../helpers/helpers";
import {
  createMacroReturn,
  ExtractMacroReturn,
  ExtractProps,
  MacroReturn,
  MacroReturnObject,
  MacroReturnType,
} from "../setup";
import { withDefaults_Box } from "../vue";

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
export type ResolveDefaultsPropsFromMacro<T> = T extends ExtractProps<infer M>
  ? M extends {
      props: infer PM extends MacroReturnType<any, any>;
    }
    ? M extends {
        withDefaults: {
          type: [any, infer D];
        };
      }
      ? { type: ResolveFromMacroReturn<PM>; defaults: keyof D }
      : {
          type: ResolveFromMacroReturn<PM>;
          defaults: FindDefaultsKey<ResolveFromMacroReturn<PM>>;
        }
    : T extends {
        props: MacroReturnObject<infer V, infer O>;
      }
    ? {
        type: V;
        defaults: PropsObjectExtractDefaults<O>;
      }
    : T
  : T;

export type PropsObjectExtractDefaults<T> = T extends string[]
  ? string
  : {
      [K in keyof T]: T[K] extends { default: any } ? K : "";
    }[keyof T];

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
  ResolveDefaultsPropsFromMacro<T> extends {
    type: infer P;
    defaults: infer D;
  }
    ? D extends keyof P
      ? Omit<P, D> & {
          [K in keyof P as K extends D ? K : never]?: P[K] | undefined;
        }
      : P
    : T extends [infer PP, any]
    ? MakeBooleanOptional<PP>
    : MakeBooleanOptional<T>;

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
 * These are the internal props of the component.
 * They have all props required.
 *
 * They are accessed inside the component implementation.
 */
export type MakeInternalProps<T extends Record<PropertyKey, any>> =
  ResolveDefaultsPropsFromMacro<T> extends {
    type: infer P;
    defaults: infer D;
  }
    ? D extends keyof P
      ? Omit<P, D> & {
          [K in keyof P as K extends D ? K : never]-?: P[K] | undefined;
        }
      : P
    : T;
