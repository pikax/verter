import { UnionToIntersection } from "../helpers";

/**
 * Extracts model information from a ModelRef type.
 * If T is a ModelRef<G, N, S>, it returns an object with:
 * - type: G (the type of the model value)
 * - setter: S (the type of the setter function)
 * - name: N (the custom name, or the default name D if N is string)
 * If T is undefined, it returns { type: undefined } this is to allow union typing for undefined.
 */
type ExtractModelInfo<T, D extends string> = T extends import("vue").ModelRef<
  any,
  infer N extends string,
  infer G,
  infer S
>
  ? { type: G; setter: S; name: string extends N ? D : N }
  : undefined extends T
  ? { type: undefined }
  : T extends import("vue").ModelRef<infer TT>
  ? { type: TT; name: "modelValue" }
  : never;

/**
 * Maps an object type T of ModelRef properties to their model information.
 */
export type ModelToModelInfo<T> = {
  [K in keyof T]: ExtractModelInfo<T[K], K & string>;
};

/**
 * Converts model definitions into emit function types.
 * For each model property, it creates an emit function for the update event.
 * The event name is `update:${modelName}` where modelName is the custom name if provided, otherwise the property key.
 * The argument type is derived from the model's type.
 */
export type ModelToEmits<T> = {} extends T
  ? () => any
  : ModelToModelInfo<T> extends infer O
  ? UnionToIntersection<
      {
        [K in keyof O]-?: O[K] extends { name: never }
          ? never
          : (
              event: `update:${O[K] extends { name: infer N extends string }
                ? N
                : K & string}`,
              ...args: O[K] extends { type: infer C }
                ? C extends never
                  ? any[]
                  : [arg: C]
                : any[]
            ) => any;
      }[keyof O]
    >
  : never;

/**
 * Converts model definitions into Vue Props types.
 * For each model property, it creates a prop with the model's type.
 * The prop name is the custom name if provided, otherwise the property key.
 * Properties with model info of never are excluded.
 */
export type ModelToProps<T> = ModelToModelInfo<T> extends infer O
  ? {
      [K in keyof O as O[K] extends never
        ? never
        : O[K] extends { name: infer N extends string }
        ? N
        : K & string]: O[K] extends { type: infer C } ? C : never;
    }
  : never;
