import { ModelToEmits, ModelToProps } from "../model";
import { MakeInternalProps, MakePublicProps } from "../props";

declare const MacroKey: unique symbol;

/**
 * Valid macros from vue setup
 */
export type ReturnMacros =
  | "props"
  | "emits"
  | "slots"
  | "options"
  | "model"
  | "expose"
  | "withDefaults";
/**
 * Normal macros, model holds special structure
 */
export type RegularMacros = Exclude<ReturnMacros, "model">;
/**
 * Helper to create macro return by type
 */
export type MacroReturnType<V, T> = { value: V; type: T };

/**
 * Helper to create macro return by object
 */
export type MacroReturnObject<V, T> = { value: V; object: T };

/**
 * Helper to create macro return by type or object
 */
export type MacroReturn<V, T> = MacroReturnType<V, T> | MacroReturnObject<V, T>;

/**
 * Create macro return helper, to be used in setup return types
 */
export declare function createMacroReturn<
  T extends Partial<
    Record<RegularMacros, MacroReturn<any, any>> & {
      model: Record<string, MacroReturn<any, any>>;
    }
  >
>(o: T): CreateMacroReturn<T>;

export type CreateMacroReturn<T> = { [MacroKey]: T };

/**
 * Extracts the macro metadata from a return type created by `createMacroReturn`.
 *
 * @template T - The type to extract from
 * @returns The macro metadata object, or `never` if not present
 *
 * @example
 * ```ts
 * const setup = () => createMacroReturn({ props: { value: ..., type: ... } });
 * type Macros = ExtractMacroReturn<ReturnType<typeof setup>>;
 * // Macros = { props: { value: ..., type: ... } }
 * ```
 */
export type ExtractMacroReturn<T> = T extends { [MacroKey]: infer R }
  ? R
  : never;

/**
 * Extracts a specific macro type from macro metadata.
 *
 * @template T - The macro metadata object
 * @template R - The macro name to extract ("props" | "emits" | "slots" | "options" | "model" | "expose")
 * @template F - Fallback type if macro is not present (defaults to `never`)
 * @returns The extracted macro type, or fallback if not present
 *
 * @example
 * ```ts
 * type Macros = { props: MacroReturn<Props, Props> };
 * type Props = ExtractMacro<Macros, "props">;
 * type Emits = ExtractMacro<Macros, "emits", () => void>; // Returns () => void (fallback)
 * ```
 */
export type ExtractMacro<T, R extends ReturnMacros, F = never> = T extends {
  [K in R]: infer M;
}
  ? M
  : F;

/**
 * Extracts the props macro from macro metadata.
 *
 * @template T - The macro metadata object
 * @returns The props macro, or `{}` if not present
 *
 * @example
 * ```ts
 * const setup = () => createMacroReturn({
 *   props: { value: { foo: string }, type: { foo: string } }
 * });
 * type Props = ExtractProps<ExtractMacroReturn<ReturnType<typeof setup>>>;
 * ```
 */
export type ExtractProps<T> = (ExtractMacro<T, "props", {}> extends infer P
  ? { props: P }
  : {}) &
  (ExtractMacro<T, "withDefaults", {}> extends infer D ? { defaults: D } : {});

/**
 * Extracts the emits macro from macro metadata.
 *
 * @template T - The macro metadata object
 * @returns The emits macro, or `() => void` if not present
 *
 * @example
 * ```ts
 * const setup = () => createMacroReturn({
 *   emits: { value: emitFn, type: EmitsType }
 * });
 * type Emits = ExtractEmits<ExtractMacroReturn<ReturnType<typeof setup>>>;
 * ```
 */
export type ExtractEmits<T> = ExtractMacro<T, "emits", () => void>;

export type ToEmitValue<T> = T extends { value: infer V } ? V : () => void;

/**
 * Extracts the slots macro from macro metadata.
 *
 * @template T - The macro metadata object
 * @returns The slots macro, or `{}` if not present
 *
 * @example
 * ```ts
 * const setup = () => createMacroReturn({
 *   slots: { value: slotsObj, type: SlotsType }
 * });
 * type Slots = ExtractSlots<ExtractMacroReturn<ReturnType<typeof setup>>>;
 * ```
 */
export type ExtractSlots<T> = ExtractMacro<T, "slots", {}>;

export type SlotsToSlotType<T> = T extends {
  type: infer ST extends Record<string, any>;
}
  ? import("vue").SlotsType<ST>
  : T extends { value: infer SV extends Record<string, any> }
  ? import("vue").SlotsType<SV>
  : {};

/**
 * Extracts the options macro from macro metadata.
 *
 * @template T - The macro metadata object
 * @returns The options macro, or `{}` if not present
 *
 * @example
 * ```ts
 * const setup = () => createMacroReturn({
 *   options: { value: optionsObj, type: OptionsType }
 * });
 * type Options = ExtractOptions<ExtractMacroReturn<ReturnType<typeof setup>>>;
 * ```
 */
export type ExtractOptions<T> = ExtractMacro<T, "options", {}>;

/**
 * Extracts the model macro from macro metadata.
 *
 * @template T - The macro metadata object
 * @returns The model macro, or `{}` if not present
 *
 * @example
 * ```ts
 * const setup = () => createMacroReturn({
 *   model: {
 *     modelValue: { value: ref, type: string },
 *     count: { value: ref, type: number }
 *   }
 * });
 * type Model = ExtractModel<ExtractMacroReturn<ReturnType<typeof setup>>>;
 * ```
 */
export type ExtractModel<T> = ExtractMacro<T, "model", {}>;

export type ToModelType<T> = T extends {
  type: infer TT extends Record<string, any>;
}
  ? TT
  : T extends { value: infer TV extends Record<string, any> }
  ? TV
  : {};

/**
 * Extracts the expose macro from macro metadata.
 *
 * @template T - The macro metadata object
 * @returns The expose macro, or `{}` if not present
 *
 * @example
 * ```ts
 * const setup = () => createMacroReturn({
 *   expose: { value: { focus: () => {} }, type: { focus: () => void } }
 * });
 * type Expose = ExtractExpose<ExtractMacroReturn<ReturnType<typeof setup>>>;
 * ```
 */
export type ExtractExpose<T> = ExtractMacro<T, "expose", {}>;

export type ExposeToVueExposeKey<T> = T extends {
  type: infer TT extends Record<string, any>;
}
  ? keyof TT
  : T extends { value: infer TV extends Record<string, any> }
  ? keyof TV
  : keyof {};

/**
 * Normalizes a macro return type into a structured object with all macro types.
 *
 * This utility extracts and organizes all macros from a `createMacroReturn` result
 * into a consistent object shape, providing defaults for missing macros.
 *
 * @template T - The return type from `createMacroReturn`
 * @returns An object containing all extracted macro types:
 *   - `props`: Extracted props with defaults (default: `{}`)
 *   - `emits`: Extracted emits function (default: `() => void`)
 *   - `slots`: Extracted slots (default: `{}`)
 *   - `options`: Extracted options (default: `{}`)
 *   - `model`: Extracted model bindings (default: `{}`)
 *   - `expose`: Extracted exposed methods/properties (default: `{}`)
 *
 * @example
 * ```ts
 * const setup = () => createMacroReturn({
 *   props: { value: { id: number }, type: PropsType },
 *   emits: { value: emitFn, type: EmitsType },
 *   model: { modelValue: { value: ref, type: string } }
 * });
 *
 * type Normalized = NormaliseMacroReturn<ReturnType<typeof setup>>;
 * // {
 * //   props: { value: { id: number }, type: PropsType, defaults: {} },
 * //   emits: { value: emitFn, type: EmitsType },
 * //   slots: {},
 * //   options: {},
 * //   model: { modelValue: { value: ref, type: string } },
 * //   expose: {}
 * // }
 * ```
 */
export type NormaliseMacroReturn<T> = ExtractMacroReturn<T> extends infer R
  ? {
      props: ExtractProps<R>;
      emits: ExtractEmits<R>;
      slots: ExtractSlots<R>;
      options: ExtractOptions<R>;
      model: ExtractModel<R>;
      expose: ExtractExpose<R>;
    }
  : {};

export type MacroReturnToInstance<
  T,
  Public extends boolean = false
> = NormaliseMacroReturn<T> extends {
  props: infer P extends ExtractProps<any>;
  emits: infer E extends ExtractEmits<any>;
  slots: infer S extends ExtractSlots<any>;
  options: infer O extends ExtractOptions<any>;
  model: infer M extends ExtractModel<any>;
  expose: infer X extends ExtractExpose<any>;
}
  ? //import("vue").ComponentPublicInstance & 
  {
      // todo data should be the bindings from setup, unless is production build
      $data: {};
      $props: P
         /* &
        ModelToProps<ToModelType<M>>;
      $emit: ToEmitValue<E> & ModelToEmits<ToModelType<M>>;
      $slots: S; */
    }
  : //   P["props"],
    //   {},
    //   {},
    //   {},
    //   {},
    //   {},
    //   {},
    //   P["defaults"],
    //   Public,
    //   O["value"],
    //   {},
    //   SlotsToSlotType<S>,
    //   ExposeToVueExposeKey<X>,
    //   // todo check what's typeRef
    //   {},
    //   //todo check typeEl
    //   any
    // > & { $emits: E["value"] }
    false;

export type MacroToInternalInstance<T> = NormaliseMacroReturn<T> extends {
  props: infer P extends ExtractProps<any>;
  emits: infer E extends ExtractEmits<any>;
  slots: infer S extends ExtractSlots<any>;
  options: infer O extends ExtractOptions<any>;
  model: infer M extends ExtractModel<any>;
  expose: infer X extends ExtractExpose<any>;
}
  ? import("vue").ComponentInternalInstance & {
      emit: Emit;
      // todo others
    }
  : import("vue").ComponentInternalInstance;

type T = MacroReturnToInstance<CreateMacroReturn<{
  props: { value: { foo: string }; type: { foo: string } };
  emits: (e: "update", val: number) => void;
  slots: { value: { default: () => string } };
  options: { value: { customOption: boolean } };
  model: { modelValue: { value: string; type: string } };
  expose: { value: { focus: () => void }; type: { focus: () => void } };
}>>;

const t = {} as T;
t.$props.foo



declare const P: {
  props: { value: { foo: string }; type: { foo: string } };
};

declare const PP: MakePublicProps<typeof P>;

PP



