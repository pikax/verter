
/**
 * Type helper that extracts the key and value types from a loop iterable (array or object).
 *
 * This is the type-level equivalent of the `extractLoops` function, useful when you need
 * to compute the loop types without calling the function.
 *
 * @template T - The iterable type (array or object)
 *
 * @example Array type
 * ```typescript
 * type Result = ExtractLoopsResult<string[]>;
 * // Result = { key: number; value: string }
 * ```
 *
 * @example Object type
 * ```typescript
 * type Result = ExtractLoopsResult<{ a: number; b: string }>;
 * // Result = { key: "a"; value: number } | { key: "b"; value: string }
 * ```
 */
export type ExtractLoopsResult<T> = T extends Array<infer V>
  ? { key: number; value: V }
  : T extends Record<any, any>
    ? { [K in keyof T]: { key: K; value: T[K] } }[keyof T]
    : never;

/**
 * Extracts the key and value types from a loop iterable (array or object).
 *
 * This helper is used by the component-type plugin to provide correct typing
 * for `v-for` loop variables in Vue templates.
 *
 * @example Array iteration
 * ```typescript
 * // For: v-for="(item, index) in items"
 * type Items = { id: number; name: string }[];
 * type Result = ReturnType<typeof extractLoops<Items>>;
 * // Result = { key: number; value: { id: number; name: string } }
 * // - key corresponds to the index
 * // - value corresponds to the item
 * ```
 *
 * @example Object iteration
 * ```typescript
 * // For: v-for="(value, key) in obj"
 * type Obj = { a: number; b: string };
 * type Result = ReturnType<typeof extractLoops<Obj>>;
 * // Result = { key: "a"; value: number } | { key: "b"; value: string }
 * // - key is the literal property name
 * // - value is the property value type
 * ```
 *
 * @see {@link file://packages/core/src/v5/process/script/plugins/component-type/component-type.ts} - Component type plugin that uses this helper
 */
export declare function extractLoops<T extends Array<any>>(
  element: T
): T extends Array<infer V>
  ? { key: number; value: V }
  : { key: number; value: unknown };

/**
 * Extracts the key and value types from an object for loop iteration.
 *
 * @template T - The object type to extract loop information from
 * @param element - The object to iterate over
 * @returns A union of `{ key: K; value: T[K] }` for each property K in T
 *
 * @example
 * ```typescript
 * type Config = { host: string; port: number; debug: boolean };
 * type Result = ReturnType<typeof extractLoops<Config>>;
 * // Result = { key: "host"; value: string }
 * //        | { key: "port"; value: number }
 * //        | { key: "debug"; value: boolean }
 * ```
 */
export declare function extractLoops<T extends Record<any, any>>(
  element: T
): {
  [K in keyof T]: {
    key: K;
    value: T[K];
  };
}[keyof T];
