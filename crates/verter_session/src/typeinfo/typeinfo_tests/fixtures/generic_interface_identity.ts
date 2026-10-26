export interface Empty<T> {}
export interface DefaultEmpty<T = string> {}
export interface Pair<T, U = T> {}
export interface Tag<T> {
  kind: T;
}
export type Conflict = Tag<"a"> & { kind: "b" };
export type ConflictIsNever = Tag<"a"> & { kind: "b" } extends never ? true : false;
export type NullIntersection = Empty<string> & null;
export type UndefinedIntersection = Empty<string> & undefined;
export type DefaultState = DefaultEmpty & { count: number };
export type PartialState = Pair<number> & { count: number };
export type DefaultNested = [
  DefaultEmpty & { count: number },
  { state: Pair<number> & { count: number } },
];
export interface Full<T> {
  value: T;
}
export interface Plain {
  label: string;
}
export interface Child<T> extends Full<T> {
  own: T[];
}
export type Transparent<T> = T extends string ? { selected: T } : { selected: number };
export type State<T> = Empty<T> & { count: number };
export type Nested<T> = [Empty<T> & { count: number }, { state: Empty<T> & { count: number } }];
export type Nonempty<T> = Full<T> & { count: number };
export type Ordinary = Plain & { count: number };
export type Operators<T> = [Transparent<T> & Empty<T>, Child<T>["value"], keyof Child<T>];
export type Equivalent<T> =
  State<T> extends { count: number } ? ({ count: number } extends State<T> ? true : false) : false;
export type NestedEquivalent<T> =
  Nested<T> extends [{ count: number }, { state: { count: number } }]
    ? [{ count: number }, { state: { count: number } }] extends Nested<T>
      ? true
      : false
    : false;
