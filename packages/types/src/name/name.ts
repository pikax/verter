/**
 * Utility to check if a character can be capitalized.
 * Handles special cases for "-", "_", "$", and empty strings.
 */
export type CanCapitalize<T extends string> = T extends "-" | "_" | "$" | ""
  ? "x"
  : Capitalize<T>;

/**
 * Converts PascalCase strings to kebab-case.
 * Handles transitions between uppercase and lowercase letters.
 */
export type PascalToKebab<T extends string> =
  T extends `${infer Left}${infer Right}${infer Rest}`
    ? Left extends CanCapitalize<Left>
      ? Right extends CanCapitalize<Right>
        ? // both capitalised
          `${Left}-${Right}${PascalToKebab<Rest>}`
        : // right is lower-case but left is upper
          `${Left}${Right}${PascalToKebab<Rest>}`
      : // left is lower-case
      Right extends CanCapitalize<Right>
      ? `${Left}-${Right}${PascalToKebab<Rest>}`
      : `${Left}${Right}${PascalToKebab<Rest>}`
    : T;

/**
 * Converts camelCase strings to kebab-case.
 */
export type CamelToKebab<T extends string> =
  T extends `${infer Left}${infer Right}${infer Rest}`
    ? Left extends Lowercase<Left>
      ?
          | `${Left}${Right}${Rest}`
          | `${Left}${Right}${PascalToKebab<`${Rest}`>}`
          | `${Left}${Right}${Lowercase<PascalToKebab<`${Rest}`>>}`
      : `${Lowercase<Left>}${Right}${Rest}`
    : T;

/**
 * Normalises string to PascalCase.
 */
export type NormalisePascal<T extends string> = T extends Capitalize<T>
  ? PascalToKebab<T> extends infer Pascal extends string
    ? Pascal | Lowercase<Pascal>
    : T
  : T;

// Vue's hyphenate: adds '-' before uppercase (non-initial) and lowercases all
type _HyphenateImpl<
  S extends string,
  IsStart extends boolean
> = S extends `${infer F}${infer R}`
  ? F extends Lowercase<F>
    ? `${F}${_HyphenateImpl<R, false>}`
    : F extends Uppercase<F>
    ? IsStart extends true
      ? `${Lowercase<F>}${_HyphenateImpl<R, false>}`
      : `-${Lowercase<F>}${_HyphenateImpl<R, false>}`
    : `${F}${_HyphenateImpl<R, false>}`
  : "";

/**
 * Converts a string to kebab-case by adding hyphens before uppercase letters
 * and lowercasing all letters.
 * Distributes over union types.
 */
export type Hyphenate<T extends string> = _HyphenateImpl<T, true>;

// Vue's camelize: remove '-' and uppercase following word char
export type Camelize<T extends string> = T extends `${infer L}-${infer R}`
  ? R extends `${infer C}${infer Rest}`
    ? `${L}${Uppercase<C>}${Camelize<Rest>}`
    : L
  : T;
