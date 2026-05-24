// @ai-generated - Synthetic utility-edge typeinfo fixture.

export type Base = { a: number; b: string; c: boolean };

export type PickNever = Pick<Base, never>;
export type OmitNever = Omit<Base, never>;
export type OmitAll = Omit<Base, keyof Base>;
export type PickAll = Pick<Base, keyof Base>;

export type Optional = { a?: string; b?: number };
export type RequiredOptional = Required<Optional>;
export type ReadonlyRequiredOptional = Readonly<Required<Optional>>;

export type Nullable = string | null | undefined;
export type NonNullablePrim = NonNullable<Nullable>;
export type ExtractStringOnly = Extract<string | number | boolean, string>;
export type ExcludeNumberOnly = Exclude<string | number | boolean, number>;
