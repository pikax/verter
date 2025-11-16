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
  | "expose";
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
>(o: T): { [MacroKey]: T };


export type ExtractMacroReturn<T> = T extends { [MacroKey]: infer R } ? R : never;