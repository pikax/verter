// @ai-generated - Synthetic `const` type-parameter typeinfo fixture.

export declare function makeRoute<const T extends readonly { path: string }[]>(routes: T): T;

export function constRouteCall() {
  return makeRoute([{ path: "/home" }, { path: "/about" }]);
}
export type ConstRouteResult = ReturnType<typeof constRouteCall>;

export declare function makeStrings<const T extends readonly string[]>(values: T): T;
export function constStringsCall() {
  return makeStrings(["a", "b", "c"]);
}
export type ConstStringsResult = ReturnType<typeof constStringsCall>;

// Without `const` modifier the same call widens; this contrastive helper is
// only here as the negative-comparison shape (not exercised directly, but
// documents the *purpose* of `const T` for any future cross-check).
export declare function makeStringsWide<T extends readonly string[]>(values: T): T;
