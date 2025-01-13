export type CanCapitalize<T extends string> = T extends "-" | "_" | "$" | ""
  ? "x"
  : Capitalize<T>;

export type PascalToKebabComponent<T extends string> =
  T extends `${infer Left}${infer Right}${infer Rest}`
    ? Left extends CanCapitalize<Left>
      ? Right extends CanCapitalize<Right>
        ? // both capitalised
          `${Left}-${Right}${PascalToKebabComponent<Rest>}`
        : // right is lower-case but left is upper
          `${Left}${Right}${PascalToKebabComponent<Rest>}`
      : // left is lower-case
      Right extends CanCapitalize<Right>
      ? `${Left}-${Right}${PascalToKebabComponent<Rest>}`
      : `${Left}${Right}${PascalToKebabComponent<Rest>}`
    : T;

export type CamelToKebabComponent<T extends string> =
  T extends `${infer Left}${infer Right}${infer Rest}`
    ? Left extends Lowercase<Left>
      ?
          | `${Left}${Right}${Rest}`
          | `${Left}${Right}${PascalToKebabComponent<`${Rest}`>}`
          | `${Left}${Right}${Lowercase<PascalToKebabComponent<`${Rest}`>>}`
      : `${Lowercase<Left>}${Right}${Rest}`
    : T;

export type NormalisePascal<T extends string> = T extends Capitalize<T>
  ? PascalToKebabComponent<T> extends infer Pascal extends string
    ? Pascal | Lowercase<Pascal>
    : T
  : T;

export type NormaliseComponentKey<T extends string> =
  | NormalisePascal<T>
  | CamelToKebabComponent<T>
  | T;
