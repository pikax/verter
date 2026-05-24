// @ai-generated - Synthetic template-literal pattern-matching fixture.

export type SplitOn<S extends string, D extends string> = S extends `${infer H}${D}${infer T}`
  ? [H, ...SplitOn<T, D>]
  : [S];

export type DotSplitAbc = SplitOn<"a.b.c", ".">;

export type StripPrefix<S extends string, P extends string> = S extends `${P}${infer Rest}`
  ? Rest
  : S;
export type StripOnPrefix<S> = S extends `on${infer Rest}` ? Uncapitalize<Rest> : S;
export type StripOnClick = StripOnPrefix<"onClick">;
export type StripOnUnused = StripOnPrefix<"submit">;

export type EventHandlers<T extends string> = {
  [K in T as `on${Capitalize<K>}`]: (payload: K) => void;
};
export type CounterHandlers = EventHandlers<"inc" | "dec">;

export type StaticDigit<S extends string> = S extends `${infer D extends number}` ? D : never;
export type Digit42 = StaticDigit<"42">;
