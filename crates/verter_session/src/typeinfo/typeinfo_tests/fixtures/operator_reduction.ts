// @ai-generated - Named-symbol operator-reduction bridge fixture.

export interface KeySurface {
  id: string;
  count: number;
}

// Concrete indexed access: object resolves to a real surface, so the shared
// IndexedAccess reducer produces the terminal member type.
export type ConcreteLookup = KeySurface["id"];

// `keyof` over a concrete surface: reduces to the member-name literal union.
export type ConcreteKeys = keyof KeySurface;

// Symbolic indexed access: the object stays an open type parameter, so the
// shared reducer cannot resolve a concrete surface and MUST preserve the
// IndexedAccess carrier rather than fabricate a member type.
export type SymbolicLookup<T> = T["id"];
