export type $V_CanCapitalize<T extends string> = T extends "-" | "_" | "$" | ""
  ? "x"
  : Capitalize<T>;

export type $V_PascalToKebabComponent<T extends string> =
  T extends `${infer Left}${infer Right}${infer Rest}`
    ? Left extends $V_CanCapitalize<Left>
      ? Right extends $V_CanCapitalize<Right>
        ? // both capitalised
          `${Left}-${Right}${$V_PascalToKebabComponent<Rest>}`
        : // right is lower-case but left is upper
          `${Left}${Right}${$V_PascalToKebabComponent<Rest>}`
      : // left is lower-case
      Right extends $V_CanCapitalize<Right>
      ? `${Left}-${Right}${$V_PascalToKebabComponent<Rest>}`
      : `${Left}${Right}${$V_PascalToKebabComponent<Rest>}`
    : T;

export type $V_CamelToKebabComponent<T extends string> =
  T extends `${infer Left}${infer Right}${infer Rest}`
    ? Left extends Lowercase<Left>
      ?
          | `${Left}${Right}${Rest}`
          | `${Left}${Right}${$V_PascalToKebabComponent<`${Rest}`>}`
          | `${Left}${Right}${Lowercase<$V_PascalToKebabComponent<`${Rest}`>>}`
      : `${Lowercase<Left>}${Right}${Rest}`
    : T;

export type $V_NormalisePascal<T extends string> = T extends Capitalize<T>
  ? $V_PascalToKebabComponent<T> extends infer Pascal extends string
    ? Pascal | Lowercase<Pascal>
    : T
  : T;

export type $V_NormaliseComponentKey<T extends string> =
  | $V_NormalisePascal<T>
  | $V_CamelToKebabComponent<T>
  | T;
