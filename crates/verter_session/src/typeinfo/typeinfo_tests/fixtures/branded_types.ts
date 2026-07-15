// @ai-generated - Synthetic branded / nominal-typing typeinfo fixture.

// (1) String brand via intersection.
export type UserId = string & { readonly __brand: "UserId" };

// (2) Numeric brand via intersection.
export type Cents = number & { readonly __cents: true };

// (3) `unique symbol` brand carried in a generic wrapper.
export declare const idBrand: unique symbol;
export type IdBranded<T> = T & { readonly [idBrand]: T };

// A concrete instantiation of the unique-symbol brand wrapper.
export type AccountId = IdBranded<string>;

// (4) Brand projection via key access — recover the literal brand tag.
export type UserIdBrandTag = UserId["__brand"];

// (5) Phantom type — two-parameter brand carrier.
export type Phantom<P, T> = T & { readonly __phantom: P };
export type EmailString = Phantom<"email", string>;

// (6) Branded type guard. `narrowUserId` produces a narrowed `UserId` after
// the guard succeeds; we resolve the alias of the unique return path to keep
// the test surface deterministic.
export declare function isUserId(x: string): x is UserId;
export function narrowUserId(value: string): UserId | undefined {
  if (isUserId(value)) {
    return value;
  }
  return undefined;
}
export type NarrowedUserId = ReturnType<typeof narrowUserId>;

// (7) Numeric brand tag projection — parallel to (4), recovers a
// boolean-literal brand tag instead of a string-literal one.
export type CentsBrandTag = Cents["__cents"];

// (8) Symbol-key value projection — recovers the value at the unique-symbol
// brand slot of a concretely-instantiated branded wrapper.
export type AccountIdBrandValue = AccountId[typeof idBrand];

// (9) Double-brand intersection — combines a string brand with a numeric
// brand. The primitive carriers (`string` and `number`) are disjoint at the
// structural level.
export type UserIdCentsBoth = UserId & Cents;
